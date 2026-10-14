//! Native GTK/Qt adapter. A drop projects paths immediately; the complete core
//! command and its snapshot run on a worker. Polls and edits never join that worker.
use crate::{
    AppCommand, AppError, AppSnapshot, Application, DiagnosticObserver, SidebarDropTrace,
    move_target, remap_command, renamed_path,
};
use native_workspace_domain::DocumentId;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub struct NativeApplication {
    core: Option<Application>,
    cached: AppSnapshot,
    observer: DiagnosticObserver,
    worker: Option<Receiver<Reply>>,
    relocation: Option<(DocumentId, DocumentId)>,
    pending_target: Option<DocumentId>,
    deferred: Vec<AppCommand>,
    background_error: Option<String>,
    move_trace: Option<SidebarDropTrace>,
    #[cfg(debug_assertions)]
    next_drop_delay: std::time::Duration,
}
struct Reply {
    core: Application,
    snapshot: AppSnapshot,
    error: Option<String>,
}
impl Default for NativeApplication {
    fn default() -> Self {
        Self::new()
    }
}
impl NativeApplication {
    pub fn new() -> Self {
        Self::with_application(Application::new())
    }
    pub fn with_application(core: Application) -> Self {
        Self {
            cached: core.snapshot(),
            observer: core.diagnostic_observer(),
            core: Some(core),
            worker: None,
            relocation: None,
            pending_target: None,
            deferred: Vec::new(),
            background_error: None,
            move_trace: None,
            #[cfg(debug_assertions)]
            next_drop_delay: std::time::Duration::ZERO,
        }
    }
    /// Explicit native smoke-test hook. The delay runs only on the worker.
    #[cfg(debug_assertions)]
    pub fn delay_next_drop_for_smoke(&mut self, duration: std::time::Duration) {
        self.next_drop_delay = duration;
    }
    pub fn snapshot(&self) -> AppSnapshot {
        self.cached.clone()
    }
    pub fn has_pending_move(&self) -> bool {
        self.worker.is_some()
    }
    pub fn pending_move_target(&self) -> Option<&DocumentId> {
        self.pending_target.as_ref()
    }
    pub fn diagnostic_observer(&self) -> DiagnosticObserver {
        self.observer.clone()
    }
    pub fn devtools_enabled(&self) -> bool {
        self.observer.enabled()
    }
    pub fn devtools_supported(&self) -> bool {
        cfg!(feature = "devtools")
    }
    pub fn diagnostic_snapshot_applied(&self, operation: u64, revision: u64, duration: u64) {
        self.observer
            .snapshot_applied(operation, revision, duration);
    }
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
    pub fn open_devtools(&self) {
        self.observer.open_console();
    }
    pub fn enable_persistence_default(&mut self) -> Result<AppSnapshot, AppError> {
        let result = self
            .core
            .as_mut()
            .expect("persistence is initialized before drops")
            .enable_persistence_default()?;
        self.cached = result.clone();
        Ok(result)
    }
    pub fn take_background_error(&mut self) -> Option<String> {
        self.background_error.take().or_else(|| {
            self.core
                .as_mut()
                .and_then(Application::take_background_error)
        })
    }
    pub fn dispatch_sidebar_move(
        &mut self,
        id: DocumentId,
        directory: String,
        _optimistic: bool,
        trace: SidebarDropTrace,
    ) -> Result<AppSnapshot, AppError> {
        self.dispatch_sidebar_move_with_edit(id, directory, trace, None)
    }
    /// Commit any local Qt editor text on the same worker before relocation,
    /// so the drop callback never flushes a buffer through the core synchronously.
    pub fn dispatch_sidebar_move_with_edit(
        &mut self,
        id: DocumentId,
        directory: String,
        trace: SidebarDropTrace,
        edit: Option<(DocumentId, String)>,
    ) -> Result<AppSnapshot, AppError> {
        if self.worker.is_some() {
            return Err(AppError::InvalidMove);
        }
        let target = move_target(&id, &directory)?;
        if !self
            .core
            .as_ref()
            .unwrap()
            .validate_relocation(&id, &target)?
        {
            return Ok(self.snapshot());
        }
        if let Some((id, content)) = &edit {
            project_edit(&mut self.cached, id, content);
        }
        self.relocation = Some((id.clone(), target.clone()));
        self.pending_target = Some(target.clone());
        self.move_trace = Some(trace.clone());
        self.cached.diagnostic_operation = trace.context().map_or(0, |context| context.operation);
        project_paths(&mut self.cached, &id, &target);
        self.cached.revision += 1;
        let mut core = self.core.take().unwrap();
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        #[cfg(debug_assertions)]
        let delay = std::mem::take(&mut self.next_drop_delay);
        std::thread::spawn(move || {
            #[cfg(debug_assertions)]
            std::thread::sleep(delay);
            let result = (|| {
                if let Some((id, content)) = edit {
                    core.dispatch(AppCommand::ChangeDocument { id, content })?;
                }
                core.dispatch_sidebar_move(id, directory, false, trace)
            })();
            let (snapshot, error) = match result {
                Ok(snapshot) => (snapshot, None),
                Err(error) => (core.snapshot(), Some(error.to_string())),
            };
            let _ = tx.send(Reply {
                core,
                snapshot,
                error,
            });
        });
        Ok(self.snapshot())
    }
    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        if self.worker.is_some() {
            if !matches!(
                command,
                AppCommand::PollWorkspaceChanges | AppCommand::PollSearchResults
            ) {
                // Local typing stays visible while commands await the confirmed path.
                if let AppCommand::ChangeDocument { id, content } = &command {
                    project_edit(&mut self.cached, id, content);
                    self.cached.revision += 1;
                }
                self.deferred.push(command);
            }
            return Ok(self.snapshot());
        }
        if let AppCommand::MoveEntryOptimistic { id, directory } = command {
            return self.dispatch_sidebar_move(id, directory, true, SidebarDropTrace::default());
        }
        let result = self.core.as_mut().unwrap().dispatch(command)?;
        self.cached = result.clone();
        Ok(result)
    }
    pub fn dispatch_if_changed(
        &mut self,
        command: AppCommand,
    ) -> Result<Option<AppSnapshot>, AppError> {
        if let Some(worker) = &self.worker {
            let reply = match worker.try_recv() {
                Ok(reply) => reply,
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => {
                    self.background_error =
                        Some("Move worker stopped before returning the application".into());
                    return Ok(None);
                }
            };
            self.worker = None;
            let mut commands = std::mem::take(&mut self.deferred);
            if let Some((source, target)) = self.relocation.take() {
                let success = reply.error.is_none();
                if let Some(trace) = self.move_trace.take() {
                    trace.confirmed(success);
                }
                self.pending_target = None;
                if !success {
                    project_paths(&mut self.cached, &target, &source);
                    commands = commands
                        .into_iter()
                        .map(|c| remap_command(c, &target, &source))
                        .collect();
                }
            }
            if let Some(error) = reply.error {
                self.background_error = Some(error);
            }
            if commands.is_empty() {
                self.cached = reply.snapshot;
                self.core = Some(reply.core);
            } else {
                // Saves, reloads and document opens may perform I/O too. Replay
                // queued commands off-thread without replacing local typed text.
                let (tx, rx) = mpsc::channel();
                self.worker = Some(rx);
                std::thread::spawn(move || {
                    let mut core = reply.core;
                    let mut error = None;
                    for command in commands {
                        if let Err(failure) = core.dispatch(command) {
                            error = Some(failure.to_string());
                        }
                    }
                    let snapshot = core.snapshot();
                    let _ = tx.send(Reply {
                        core,
                        snapshot,
                        error,
                    });
                });
            }
            return Ok(Some(self.snapshot()));
        }
        let result = self.core.as_mut().unwrap().dispatch_if_changed(command)?;
        if let Some(snapshot) = &result {
            self.cached = snapshot.clone();
        }
        Ok(result)
    }
}
fn project_paths(snapshot: &mut AppSnapshot, source: &DocumentId, target: &DocumentId) {
    let affected = |path: &str| {
        path == source.as_str()
            || path
                .strip_prefix(source.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
    };
    let remap = |id: &mut DocumentId, title: &mut String| {
        if affected(id.as_str()) {
            *id = DocumentId::parse(renamed_path(id.as_str(), source, target)).unwrap();
            *title = id.as_str().to_owned();
        }
    };
    for doc in &mut snapshot.documents {
        remap(&mut doc.id, &mut doc.title);
    }
    snapshot.documents.sort_by(|a, b| a.id.cmp(&b.id));
    for directory in &mut snapshot.directories {
        if affected(directory) {
            *directory = renamed_path(directory, source, target);
        }
    }
    snapshot.directories.sort();
    for result in &mut snapshot.search_results {
        remap(&mut result.id, &mut result.title);
    }
    if let Some(doc) = &mut snapshot.active_document {
        remap(&mut doc.id, &mut doc.title);
    }
}

fn project_edit(snapshot: &mut AppSnapshot, id: &DocumentId, content: &str) {
    if let Some(doc) = &mut snapshot.active_document
        && &doc.id == id
    {
        doc.content = content.to_owned();
        doc.dirty = true;
    }
    if let Some(doc) = snapshot.documents.iter_mut().find(|doc| &doc.id == id) {
        doc.dirty = true;
    }
}
