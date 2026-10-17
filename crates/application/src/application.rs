//! The synchronous application engine: command → events → state.
//!
//! [`Application::execute`] is the single pipeline every command goes
//! through: resolve the command into events (validating and performing I/O),
//! commit them through [`AppState::apply`], then schedule follow-up work.
//! Frontends never drive an `Application` directly; they use
//! [`crate::Session`], which owns the threading policy.

use crate::{
    AppCommand, AppError, AppEvent, AppSnapshot, AppState, DiagnosticObserver, SidebarDropTrace,
    diagnostics::{Context, Diagnostics, Outcome},
    search::{SearchJob, SearchService},
};
use native_workspace_filesystem::{RealFilesystem, WorkspaceFilesystem, WorkspaceWatcher};
use native_workspace_storage::{MemoryStorage, MetadataStorage, SqliteStorage};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

pub(crate) struct Application {
    state: AppState,
    pub(crate) filesystem: Arc<dyn WorkspaceFilesystem>,
    pub(crate) storage: Box<dyn MetadataStorage>,
    pub(crate) watcher: Option<WorkspaceWatcher>,
    pub(crate) last_reconcile: Instant,
    pub(crate) pending_reconcile: Option<crate::handlers::workspace::PendingReconcile>,
    pub(crate) search: SearchService,
    diagnostics: Diagnostics,
    /// The context of the command being executed.
    pub(crate) command_context: Context,
    /// The context of the search whose results were applied last.
    search_context: Context,
    /// Correlates the published snapshot with the operation that caused it.
    snapshot_operation: u64,
    pub(crate) drop_trace: Option<SidebarDropTrace>,
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Application {
    pub fn new() -> Self {
        Self::with_filesystem(Box::new(RealFilesystem))
    }

    pub fn with_filesystem(filesystem: Box<dyn WorkspaceFilesystem>) -> Self {
        let filesystem: Arc<dyn WorkspaceFilesystem> = filesystem.into();
        Self {
            state: AppState::default(),
            search: SearchService::new(Arc::clone(&filesystem)),
            filesystem,
            storage: Box::new(MemoryStorage::default()),
            watcher: None,
            last_reconcile: Instant::now(),
            pending_reconcile: None,
            diagnostics: Diagnostics::default(),
            command_context: Context::default(),
            search_context: Context::default(),
            snapshot_operation: 0,
            drop_trace: None,
        }
    }

    /// Attach the user's SQLite metadata database before processing commands.
    pub fn enable_persistence_default(&mut self) -> Result<(), AppError> {
        self.enable_persistence(SqliteStorage::open_default()?)
    }

    pub fn enable_persistence(&mut self, storage: SqliteStorage) -> Result<(), AppError> {
        let recent_workspaces = storage.recent_workspaces()?;
        self.commit(&[AppEvent::RecentWorkspacesLoaded { recent_workspaces }])?;
        self.storage = Box::new(storage);
        Ok(())
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let mut snapshot = self.state.snapshot();
        snapshot.diagnostic_operation = self.snapshot_operation;
        snapshot
    }

    /// Handlers and the runtime may inspect state; only this module owns
    /// mutable access to the authoritative state and its commit pipeline.
    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    pub fn revision(&self) -> u64 {
        self.state.revision
    }

    /// Executes a command and returns the resulting snapshot.
    #[cfg(test)]
    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        self.execute(command)?;
        Ok(self.snapshot())
    }

    /// Opt-in, best effort and compile-time removable. Never returns editor errors.
    pub fn enable_devtools(&mut self, frontend: &'static str) {
        self.diagnostics.enable(frontend);
    }

    pub fn start_devtools(&mut self, frontend: &'static str) {
        self.diagnostics.start(frontend);
        let context = self.diagnostics.context();
        self.emit_state_gauges(&context);
    }

    pub fn diagnostic_observer(&self) -> DiagnosticObserver {
        self.diagnostics.observer()
    }

    /// Executes a command: resolves it into events, commits them, and
    /// schedules any search it requires. A failed command changes nothing
    /// it has not committed.
    pub fn execute(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let context = self
            .drop_trace
            .as_ref()
            .and_then(SidebarDropTrace::context)
            .unwrap_or_else(|| self.diagnostics.context());
        self.command_context = context.clone();
        let span = context.span(command.diagnostic_name(), "dispatch");
        let revision = self.state.revision;
        let result = match command {
            AppCommand::OpenPaths(paths) => self.execute_open_paths(paths),
            AppCommand::ApplyReplacement => self.apply_replacement(),
            command => match command.steps(|| self.state.dirty_tabs()) {
                Some(steps) => self.execute_steps(steps),
                None => self.execute_one(command),
            },
        };
        self.record(&context, revision, &result);
        let outcome = match &result {
            Ok(events) if events.is_empty() => Outcome::Noop,
            Ok(_) => Outcome::Ok,
            Err(_) => Outcome::Error,
        };
        let count = result.as_ref().map_or(0, |events| events.len() as u64);
        span.end(outcome, self.state.revision, count);
        result
    }

    fn execute_open_paths(&mut self, paths: Vec<PathBuf>) -> Result<Vec<AppEvent>, AppError> {
        let mut events = Vec::new();
        for path in paths {
            let commands = self.open_path_commands(&path)?;
            events.extend(self.execute_steps(commands)?);
        }
        Ok(events)
    }

    fn execute_steps(&mut self, steps: Vec<AppCommand>) -> Result<Vec<AppEvent>, AppError> {
        let mut events = Vec::new();
        for step in steps {
            events.extend(self.execute_one(step)?);
        }
        Ok(events)
    }

    pub(crate) fn execute_one(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let researches = command.researches();
        let mut events = self.resolve(command)?;
        let relocation_changes_filter = events
            .iter()
            .any(|event| matches!(event, AppEvent::EntryRelocated { .. }))
            && (!self.state.search_options.include.is_empty()
                || !self.state.search_options.exclude.is_empty());
        let research = (researches || relocation_changes_filter) && self.state.has_query();
        if research && !events.is_empty() {
            events.push(AppEvent::SearchStarted);
        }
        self.commit(&events)?;
        if events.iter().any(|event| {
            matches!(
                event,
                AppEvent::WorkspaceOpened { .. } | AppEvent::SearchCancelled
            )
        }) {
            self.search.cancel();
        }
        if events.iter().any(|event| {
            matches!(
                event,
                AppEvent::SearchQueryQueued { .. } | AppEvent::SearchConfigured { .. }
            )
        }) {
            self.queue_search(true);
        }
        // A running search still reads the old paths. Supersede it so late
        // batches cannot resurrect paths removed by a relocation. Completed
        // searches retain their matches without another filesystem scan.
        let relocated_while_searching = self.state.search_in_progress
            && events
                .iter()
                .any(|event| matches!(event, AppEvent::EntryRelocated { .. }));
        if relocated_while_searching && !research {
            self.queue_search(false);
        }
        if events
            .iter()
            .any(|event| matches!(event, AppEvent::SearchStarted))
            && !research
        {
            self.queue_search(false);
        }
        // A refresh that changed nothing still searches the disk again.
        if research {
            self.queue_search(false);
        }
        Ok(events)
    }

    /// The only place state changes: every event, one revision per commit.
    pub(crate) fn commit(&mut self, events: &[AppEvent]) -> Result<(), AppError> {
        if events.is_empty() {
            return Ok(());
        }
        let revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        for event in events {
            self.state.apply(event);
        }
        self.state.revision = revision;
        Ok(())
    }

    /// `stream` publishes matches as they are found; use it when the current
    /// results were cleared. Otherwise they stay until the search finishes.
    fn queue_search(&mut self, stream: bool) {
        let Some(workspace) = &self.state.workspace else {
            return;
        };
        if !self.state.has_query() {
            self.search.cancel();
            return;
        }
        self.search_context = self.command_context.clone();
        let generation = self.search.submit(SearchJob {
            diagnostic: self.search_context.clone(),
            root: workspace.root.clone(),
            documents: self.state.documents.entries().to_vec(),
            query: self.state.search_query.clone(),
            options: self.state.search_options.clone(),
            stream,
        });
        self.search_context.generation = generation;
        self.search_context
            .event("search.queued", "main", self.state.revision, generation);
    }

    pub(crate) fn search_delivered(&mut self, context: Context) {
        self.search_context = context;
    }

    pub(crate) fn workspace_root(&self) -> Result<&Path, AppError> {
        self.state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.as_path())
            .ok_or(AppError::WorkspaceNotOpen)
    }

    fn record(
        &mut self,
        context: &Context,
        revision: u64,
        result: &Result<Vec<AppEvent>, AppError>,
    ) {
        let events = match result {
            Ok(events) => events,
            Err(error) => {
                context.error(error.kind().diagnostic_name(), "main", self.state.revision);
                return;
            }
        };
        let from_search = events.iter().any(AppEvent::is_search_delivery);
        let facts = if from_search {
            self.search_context.clone()
        } else {
            context.clone()
        };
        if self.state.revision != revision {
            self.snapshot_operation = facts.operation;
        }
        for event in events {
            let (name, value) = event.diagnostic_fact();
            facts.event(name, "main", self.state.revision, value);
            if let Some(bytes) = event.buffer_bytes() {
                facts.event("document.buffer_bytes", "main", self.state.revision, bytes);
            }
        }
        if self.state.revision != revision {
            self.emit_state_gauges(&facts);
        }
    }

    fn emit_state_gauges(&self, context: &Context) {
        if !self.diagnostics.enabled() {
            return;
        }
        let state = &self.state;
        for (name, value) in [
            ("state.documents", state.documents.entries().len() as u64),
            ("state.directories", state.directories.len() as u64),
            ("state.search_results", state.search_results.len() as u64),
            ("state.search_pending", u64::from(state.search_in_progress)),
            ("state.revision", state.revision),
            ("state.search_generation", self.search.generation()),
            ("state.watcher_active", u64::from(self.watcher.is_some())),
        ] {
            context.event(name, "main", state.revision, value);
        }
    }
}
