//! The frontend-facing runtime. Every frontend drives exactly one `Session`
//! and renders its snapshots; none implements threading, optimistic
//! updates, deferral or error bookkeeping of its own.
//!
//! Commands run according to `AppCommand::schedule`:
//! - inline commands execute on the caller's thread;
//! - renames, moves, deletions, OS file opens and refreshes run on a worker that owns the
//!   `Application` meanwhile, so the UI never waits on the filesystem.
//!
//! While the worker runs, the published snapshot is a projection: the same
//! reducer applied to a copy of the state. Commands follow
//! `AppCommand::while_busy`: edits show at once and are replayed, other
//! commands are deferred and replayed in order, remapped if a relocation
//! failed.

use crate::{
    Activity, AppCommand, AppError, AppEvent, AppSnapshot, AppState, Application,
    DiagnosticObserver, PathFilterResult, Relocation, SidebarDropTrace,
    command::{BusyPolicy, Schedule},
    path_filter::PathFilter,
};
use native_workspace_domain::DocumentId;
use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

/// Relocations confirmed within this time never show a progress indicator.
const SLOW_RELOCATION: Duration = Duration::from_millis(300);

pub struct Session {
    last_git_poll: Instant,
    terminals: crate::terminal::Terminals,
    /// `None` while the worker owns the application.
    core: Option<Application>,
    published: AppSnapshot,
    /// The state the published snapshot shows while the worker runs.
    projection: Option<AppState>,
    worker: Option<Worker>,
    deferred: Vec<AppCommand>,
    /// The core revision the published snapshot reflects.
    core_revision: u64,
    /// Distinguishes failures reported after an already-running job started.
    error_version: u64,
    path_filter: PathFilter,
    /// The latest requested name filter, which may be ahead of the result.
    filter_query: String,
    observer: DiagnosticObserver,
    #[cfg(debug_assertions)]
    next_job_delay: Duration,
}

struct Worker {
    reply: Receiver<Reply>,
    schedule: Schedule,
    /// Replays deferred commands rather than running a user command.
    replay: bool,
    error_version: u64,
    relocation: Option<Relocation>,
    started: Instant,
    trace: SidebarDropTrace,
}

struct Reply {
    core: Application,
    error: Option<AppError>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self::with_application(Application::new())
    }

    /// Construct an isolated session using an injected filesystem adapter.
    pub fn with_filesystem(
        filesystem: Box<dyn native_workspace_filesystem::WorkspaceFilesystem>,
    ) -> Self {
        Self::with_application(Application::with_filesystem(filesystem))
    }

    pub(crate) fn with_application(core: Application) -> Self {
        Self {
            last_git_poll: Instant::now() - Duration::from_secs(5),
            terminals: crate::terminal::Terminals::new(core.live_terminals.clone()),
            published: core.snapshot(),
            core_revision: core.revision(),
            error_version: 0,
            observer: core.diagnostic_observer(),
            core: Some(core),
            projection: None,
            worker: None,
            deferred: Vec::new(),
            path_filter: PathFilter::new(),
            filter_query: String::new(),
            #[cfg(debug_assertions)]
            next_job_delay: Duration::ZERO,
        }
    }

    pub fn set_git_visible(&mut self, visible: bool) {
        if visible {
            self.last_git_poll = Instant::now() - Duration::from_secs(5);
        }
    }
    pub fn git_request(
        &mut self,
        action: &str,
        value: String,
        extra: String,
        flag: bool,
    ) -> Result<(), AppError> {
        match crate::git_request(action, value, extra, flag) {
            Ok(command) => self.dispatch(command),
            Err(error) => self.fail(error),
        }
    }
    pub fn git_resolve(&mut self, result: String) -> Result<(), AppError> {
        let conflict = self
            .published
            .git
            .inspection
            .as_ref()
            .and_then(|i| i.conflict.as_ref());
        let Some(conflict) = conflict else {
            return self.fail(AppError::Git("Open an unresolved conflict first.".into()));
        };
        self.dispatch(AppCommand::Git(crate::GitOperation::Resolve {
            path: conflict.path.clone(),
            expected: conflict.expected.clone(),
            result,
        }))
    }
    pub fn git_stage_lines(&mut self, indices: Vec<usize>) -> Result<(), AppError> {
        let result = (|| {
            let diff = self
                .published
                .git
                .diff
                .as_ref()
                .ok_or_else(|| AppError::Git("Open a text diff first.".into()))?;
            let patch = crate::git::patch::selected(diff, &indices)?;
            Ok(AppCommand::Git(crate::GitOperation::ApplyPatch {
                path: diff.path.clone(),
                expected: diff.text.clone(),
                patch,
                staged: diff.staged,
            }))
        })();
        match result {
            Ok(command) => self.dispatch(command),
            Err(error) => self.fail(error),
        }
    }
    pub fn terminal_snapshot(&self) -> Arc<crate::TerminalSnapshot> {
        self.terminals.snapshot()
    }

    pub fn terminal_command(&mut self, command: crate::TerminalCommand) -> Result<(), AppError> {
        if matches!(command, crate::TerminalCommand::New) && self.worker.is_some() {
            return Err(AppError::OperationInProgress);
        }
        self.terminals
            .dispatch(command, self.published.workspace_root())
    }

    pub fn has_running_terminals(&self) -> bool {
        self.terminals.running()
    }

    /// The latest published snapshot. Cheap: lists and text are shared.
    pub fn snapshot(&self) -> AppSnapshot {
        self.published.clone()
    }

    pub fn revision(&self) -> u64 {
        self.published.revision
    }

    /// Loads recent workspaces from the user's metadata database.
    pub fn enable_persistence_default(&mut self) -> Result<(), AppError> {
        let result = self.idle_core()?.enable_persistence_default();
        self.finish_inline(result.map(|()| Vec::new()))
    }

    /// Runs a command according to its schedule and publishes the result.
    /// The published snapshot also carries the error message, so adapters
    /// may ignore the returned error after rendering.
    pub fn dispatch(&mut self, command: AppCommand) -> Result<(), AppError> {
        self.dispatch_traced(command, SidebarDropTrace::default())
    }

    /// Like [`Session::dispatch`], correlating a sidebar drop's diagnostics.
    pub fn dispatch_traced(
        &mut self,
        command: AppCommand,
        trace: SidebarDropTrace,
    ) -> Result<(), AppError> {
        if let Some(worker) = &self.worker {
            let schedule = if self.published.activity.editing_paused {
                Schedule::Exclusive
            } else {
                worker.schedule
            };
            return self.while_busy(command.clone(), command.while_busy(schedule));
        }
        let git_poll = matches!(command, AppCommand::PollGit);
        let user_command = !matches!(
            command,
            AppCommand::PollGit | AppCommand::PollWorkspaceChanges | AppCommand::PollSearchResults
        );
        match command.schedule() {
            Schedule::Inline => {
                let result = self.idle_core()?.execute(command);
                let result = self.finish_inline(result);
                if user_command && result.is_ok() {
                    self.set_error(None);
                }
                result
            }
            Schedule::Relocation => {
                let planned = command
                    .relocation()
                    .expect("relocations request one")
                    .and_then(|relocation| self.idle_core()?.plan_relocation(relocation));
                match planned {
                    Ok(Some(relocation)) => {
                        self.start(vec![command], Schedule::Relocation, Some(relocation), trace);
                        Ok(())
                    }
                    Ok(None) => {
                        self.set_error(None);
                        trace.handler_finished(false);
                        Ok(())
                    }
                    Err(error) => {
                        trace.handler_finished(false);
                        self.fail(error)
                    }
                }
            }
            schedule @ (Schedule::Exclusive | Schedule::Background) => {
                self.start_job(vec![command], schedule, None, trace, git_poll);
                Ok(())
            }
        }
    }

    /// Uses the same validation as dispatch, without changing state or disk.
    pub fn can_move_entry(&self, id: DocumentId, directory: String) -> bool {
        let Some(core) = &self.core else { return false };
        AppCommand::MoveEntry { id, directory }
            .relocation()
            .and_then(Result::ok)
            .is_some_and(|relocation| matches!(core.plan_relocation(relocation), Ok(Some(_))))
    }

    /// Cheap drop feedback; the worker validates real locations before I/O.
    pub fn can_move_files(&self, paths: &[std::path::PathBuf], directory: &str) -> bool {
        let Some(core) = &self.core else { return false };
        let Ok(root) = core.workspace_root() else {
            return false;
        };
        if paths.is_empty() {
            return false;
        }
        let mut targets = std::collections::HashSet::new();
        paths.iter().all(|path| {
            let Some(value) = path.to_str() else {
                return false;
            };
            let Ok(external) = DocumentId::from_document_path(value) else {
                return false;
            };
            if !external.is_external() {
                return false;
            }
            let Ok(target) = DocumentId::child_of(directory, external.name()) else {
                return false;
            };
            path != &root.join(target.as_str())
                && targets.insert(target.clone())
                && core.validate_destination(&target).is_ok()
        })
    }

    /// Records an adapter-side failure, such as an unparsable path.
    pub fn report(&mut self, error: AppError) {
        let _ = self.fail(error);
    }

    /// Collects worker, watcher, search and filter results. Call it on a
    /// timer: every 16 ms while [`Session::wants_fast_poll`], else 250 ms.
    pub fn poll(&mut self) {
        self.terminals.poll();
        if self.worker.is_some() {
            self.collect_worker();
        } else {
            for command in [
                AppCommand::PollWorkspaceChanges,
                AppCommand::PollSearchResults,
            ] {
                let _ = self.dispatch(command);
            }
        }
        if self.worker.is_none()
            && self.published.workspace_root().is_some()
            && self.last_git_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_git_poll = Instant::now();
            let _ = self.dispatch(AppCommand::PollGit);
        }
        if let Some(result) = self.path_filter.poll() {
            self.publish_filter(result);
        }
    }

    /// Whether results are streaming in and should be polled promptly.
    pub fn wants_fast_poll(&self) -> bool {
        self.terminals.running()
            || self.worker.is_some()
            || self.published.search_in_progress
            || self.published.file_filter_in_progress
    }

    /// Filters the Files tree by name on a worker; the result is published
    /// by a later [`Session::poll`]. An empty query clears it at once.
    pub fn filter_files(&mut self, query: &str) {
        query.clone_into(&mut self.filter_query);
        let ready = self.path_filter.queue(
            query,
            self.published.documents.clone(),
            self.published.directories.clone(),
        );
        match ready {
            Some(result) => self.publish_filter(result),
            None if !self.published.file_filter_in_progress => {
                self.publish(|snapshot| snapshot.file_filter_in_progress = true);
            }
            None => {}
        }
    }

    /// Opt-in, best effort and compile-time removable.
    pub fn enable_devtools(&mut self, frontend: &'static str) {
        if let Some(core) = &mut self.core {
            core.enable_devtools(frontend);
            self.observer = core.diagnostic_observer();
        }
    }

    pub fn start_devtools(&mut self, frontend: &'static str) {
        if let Some(core) = &mut self.core {
            core.start_devtools(frontend);
            self.observer = core.diagnostic_observer();
        }
    }

    pub fn diagnostic_observer(&self) -> DiagnosticObserver {
        self.observer.clone()
    }

    pub fn devtools_supported() -> bool {
        cfg!(feature = "devtools")
    }

    /// Smoke-test hook: delays the next worker job on the worker thread.
    #[cfg(debug_assertions)]
    pub fn delay_next_background_job(&mut self, delay: Duration) {
        self.next_job_delay = delay;
    }

    fn idle_core(&mut self) -> Result<&mut Application, AppError> {
        self.core.as_mut().ok_or(AppError::OperationInProgress)
    }

    fn while_busy(&mut self, command: AppCommand, policy: BusyPolicy) -> Result<(), AppError> {
        match policy {
            BusyPolicy::Skip => Ok(()),
            BusyPolicy::Reject => self.fail(AppError::OperationInProgress),
            BusyPolicy::Defer => {
                let pause = command.schedule() == Schedule::Exclusive;
                self.deferred.push(command);
                if pause && !self.published.activity.editing_paused {
                    self.publish(|snapshot| snapshot.activity.editing_paused = true);
                }
                Ok(())
            }
            BusyPolicy::ProjectEdit => {
                if let AppCommand::ChangeDocument { id, content } = &command
                    && let Some(projection) = &mut self.projection
                    && projection
                        .documents
                        .get(id)
                        .and_then(|entry| entry.content())
                        .is_some_and(|current| **current != **content)
                {
                    projection.apply(&AppEvent::DocumentChanged {
                        id: id.clone(),
                        content: content.as_str().into(),
                    });
                    self.publish_projection();
                }
                self.deferred.push(command);
                Ok(())
            }
        }
    }

    /// Hands the application to a worker that runs `commands` in order.
    fn start(
        &mut self,
        commands: Vec<AppCommand>,
        schedule: Schedule,
        relocation: Option<Relocation>,
        trace: SidebarDropTrace,
    ) {
        self.start_job(commands, schedule, relocation, trace, false);
    }

    fn start_job(
        &mut self,
        commands: Vec<AppCommand>,
        schedule: Schedule,
        relocation: Option<Relocation>,
        trace: SidebarDropTrace,
        replay: bool,
    ) {
        let mut core = self
            .core
            .take()
            .expect("the worker starts from an idle core");
        let mut projection = self
            .projection
            .take()
            .unwrap_or_else(|| core.state().clone());
        if let Some(Relocation { source, target }) = &relocation {
            projection.apply(&AppEvent::EntryRelocated {
                id: source.clone(),
                target: target.clone(),
            });
        }
        self.projection = Some(projection);
        let (sender, reply) = mpsc::channel();
        #[cfg(debug_assertions)]
        let delay = std::mem::take(&mut self.next_job_delay);
        let job_trace = trace.clone();
        std::thread::spawn(move || {
            #[cfg(debug_assertions)]
            std::thread::sleep(delay);
            core.drop_trace = job_trace.is_active().then_some(job_trace);
            let mut error = None;
            // Keep the application (and its unsaved buffers) if a job panics.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for command in commands {
                    if let Err(failure) = core.execute(command) {
                        error = Some(failure);
                    }
                }
            }));
            if outcome.is_err() {
                error = Some(AppError::BackgroundTaskFailed);
            }
            core.drop_trace = None;
            let _ = sender.send(Reply { core, error });
        });
        self.worker = Some(Worker {
            reply,
            schedule,
            replay,
            error_version: self.error_version,
            relocation,
            started: Instant::now(),
            trace,
        });
        if let Some(context) = self
            .worker
            .as_ref()
            .and_then(|worker| worker.trace.context())
        {
            self.published.diagnostic_operation = context.operation;
        }
        self.publish_projection();
    }

    fn collect_worker(&mut self) {
        let worker = self.worker.as_ref().expect("collecting a running worker");
        let reply = match worker.reply.try_recv() {
            Ok(reply) => reply,
            Err(TryRecvError::Empty) => {
                let slow =
                    worker.relocation.is_some() && worker.started.elapsed() >= SLOW_RELOCATION;
                if slow != self.published.activity.relocation_slow {
                    self.publish(|snapshot| snapshot.activity.relocation_slow = slow);
                }
                return;
            }
            Err(TryRecvError::Disconnected) => Reply {
                // The worker panicked and took the application with it.
                core: Application::new(),
                error: Some(AppError::BackgroundTaskFailed),
            },
        };
        let worker = self.worker.take().expect("collecting a running worker");
        // A metadata failure may follow a committed relocation. Only an
        // uncommitted move rolls projected paths and deferred commands back.
        let failed = reply.error.is_some() && reply.core.revision() == self.core_revision;
        if worker.relocation.is_some() {
            worker.trace.confirmed(!failed);
        }
        self.core = Some(reply.core);
        let mut deferred = std::mem::take(&mut self.deferred);
        if failed && let Some(Relocation { source, target }) = &worker.relocation {
            // Deferred commands named the projected paths.
            deferred = deferred
                .into_iter()
                .map(|command| command.rebased(target, source))
                .collect();
        }
        if !deferred.is_empty() {
            // Saves and opens may perform I/O too: replay off the UI thread,
            // keeping locally typed text visible until they finish.
            let mut projection = self.core.as_ref().expect("returned").state().clone();
            for command in &deferred {
                if let AppCommand::ChangeDocument { id, content } = command
                    && projection
                        .documents
                        .get(id)
                        .is_some_and(|entry| entry.is_loaded())
                {
                    projection.apply(&AppEvent::DocumentChanged {
                        id: id.clone(),
                        content: content.as_str().into(),
                    });
                }
            }
            self.projection = Some(projection);
            let schedule = if deferred
                .iter()
                .any(|command| command.schedule() == Schedule::Exclusive)
            {
                Schedule::Exclusive
            } else {
                Schedule::Background
            };
            // A successful queued intention clears an older error. Failures from
            // this job or reported while it ran must survive the replay.
            let preserve_error =
                reply.error.is_some() || worker.error_version != self.error_version;
            self.start_job(
                deferred,
                schedule,
                None,
                SidebarDropTrace::default(),
                preserve_error,
            );
            if let Some(error) = reply.error {
                let _ = self.fail(error);
            }
            return;
        }
        self.projection = None;
        let core_snapshot = self.core.as_ref().expect("returned").snapshot();
        self.core_revision = core_snapshot.revision;
        // Replays keep the failure that preceded them visible.
        let error = match reply.error {
            Some(error) => Some(error.to_string()),
            None if worker.replay || worker.error_version != self.error_version => {
                self.published.error.clone()
            }
            None => None,
        };
        let unchanged = {
            let mut confirmed = self.merge(core_snapshot.clone());
            confirmed.activity = Activity::default();
            confirmed.error.clone_from(&error);
            confirmed.revision = self.published.revision;
            confirmed.diagnostic_operation = self.published.diagnostic_operation;
            // A confirmation matching the projection changes nothing on
            // screen; republishing it would copy every row again.
            confirmed
                == AppSnapshot {
                    activity: Activity::default(),
                    error: error.clone(),
                    ..self.published.clone()
                }
        };
        if unchanged {
            if self.published.activity != Activity::default() || self.published.error != error {
                self.publish(|snapshot| {
                    snapshot.activity = Activity::default();
                    snapshot.error = error;
                });
            }
        } else {
            let mut next = self.merge(core_snapshot);
            next.error = error;
            self.replace_published(next);
        }
    }

    /// Publishes the core state after an inline command, success or not:
    /// composite commands may have committed steps before failing.
    fn finish_inline(&mut self, result: Result<Vec<AppEvent>, AppError>) -> Result<(), AppError> {
        let core = self
            .core
            .as_ref()
            .expect("inline commands use the idle core");
        if core.revision() != self.core_revision {
            self.core_revision = core.revision();
            let next = self.merge(core.snapshot());
            self.replace_published(next);
        }
        match result {
            Ok(_) => Ok(()),
            Err(error) => self.fail(error),
        }
    }

    fn fail(&mut self, error: AppError) -> Result<(), AppError> {
        self.set_error(Some(error.to_string()));
        Err(error)
    }

    fn set_error(&mut self, error: Option<String>) {
        self.error_version = self.error_version.wrapping_add(1);
        if self.published.error != error {
            self.publish(|snapshot| snapshot.error = error);
        }
    }

    /// Adds session-owned fields to a core snapshot.
    fn merge(&self, mut snapshot: AppSnapshot) -> AppSnapshot {
        snapshot.file_filter = self.published.file_filter.clone();
        snapshot.file_filter_in_progress = self.published.file_filter_in_progress;
        snapshot.error = self.published.error.clone();
        snapshot.activity = self.activity();
        snapshot
    }

    fn activity(&self) -> Activity {
        match &self.worker {
            Some(worker) => Activity {
                busy: true,
                relocation: worker.relocation.clone(),
                relocation_slow: self.published.activity.relocation_slow,
                editing_paused: worker.schedule == Schedule::Exclusive
                    || self
                        .deferred
                        .iter()
                        .any(|command| command.schedule() == Schedule::Exclusive),
            },
            None => Activity::default(),
        }
    }

    fn publish_projection(&mut self) {
        let projection = self
            .projection
            .as_ref()
            .expect("a worker projects its state");
        let mut next = self.merge(projection.snapshot());
        next.diagnostic_operation = self.published.diagnostic_operation;
        self.replace_published(next);
    }

    fn publish_filter(&mut self, result: PathFilterResult) {
        self.publish(|snapshot| {
            snapshot.file_filter = Arc::new(result);
            snapshot.file_filter_in_progress = false;
        });
    }

    fn publish(&mut self, change: impl FnOnce(&mut AppSnapshot)) {
        let mut next = self.published.clone();
        change(&mut next);
        self.replace_published(next);
    }

    /// Every publication goes through here: one new revision, and the name
    /// filter follows path changes.
    fn replace_published(&mut self, mut next: AppSnapshot) {
        let Some(revision) = self.published.revision.checked_add(1) else {
            self.published.error = Some(AppError::RevisionOverflow.to_string());
            return;
        };
        next.revision = revision;
        let paths_changed = !Arc::ptr_eq(&next.documents, &self.published.documents)
            || !Arc::ptr_eq(&next.directories, &self.published.directories);
        let workspace_changed = next.workspace != self.published.workspace;
        self.published = next;
        if workspace_changed {
            self.filter_files("");
        } else if paths_changed && !self.filter_query.trim().is_empty() {
            // Refilter the new paths; the current matches stay until then.
            let query = self.filter_query.clone();
            self.filter_files(&query);
        }
    }
}
