//! The only UniFFI boundary in the repository.

use native_workspace_application::{AppCommand, AppError, AppSnapshot, Application};
use native_workspace_domain::DocumentId;
use std::{path::PathBuf, sync::Mutex};

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn minimap_glyphs() -> Vec<u8> {
    native_workspace_syntax::minimap_glyphs().to_vec()
}

#[derive(Clone, uniffi::Record)]
pub struct SyntaxToken {
    pub start_utf16: u64,
    pub end_utf16: u64,
    pub kind: u8,
}

#[uniffi::export]
pub fn highlight_document(file_name: String, text: String) -> Vec<SyntaxToken> {
    native_workspace_syntax::highlight(&file_name, &text)
        .into_iter()
        .map(|token| SyntaxToken {
            start_utf16: token.start_utf16 as u64,
            end_utf16: token.end_utf16 as u64,
            kind: token.kind.code(),
        })
        .collect()
}

#[derive(Clone, uniffi::Record)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub dirty: bool,
    pub external_change: bool,
}

#[derive(Clone, uniffi::Record)]
pub struct DocumentSnapshot {
    pub id: String,
    pub title: String,
    pub content: String,
    pub dirty: bool,
    pub external_change: bool,
}

#[derive(Clone, uniffi::Record)]
pub struct OpenedDocument {
    pub revision: u64,
    pub document: DocumentSnapshot,
}

#[derive(Clone, uniffi::Record)]
pub struct RecentWorkspace {
    pub root: String,
    pub name: String,
}

#[derive(Clone, uniffi::Record)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub dirty: bool,
    pub snippet: Option<String>,
    pub location: Option<SearchLocation>,
}

#[derive(Clone, uniffi::Record)]
pub struct SearchLocation {
    pub line: u64,
    pub column: u64,
    pub start_char: u64,
    pub end_char: u64,
    pub start_utf16: u64,
    pub end_utf16: u64,
}

#[derive(Clone, uniffi::Record)]
pub struct WorkspaceSnapshot {
    pub diagnostic_operation: u64,
    pub revision: u64,
    pub counter: u64,
    pub workspace_name: Option<String>,
    pub workspace_root: Option<String>,
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub search_in_progress: bool,
    pub directories: Vec<String>,
    pub documents: Vec<DocumentSummary>,
    pub active_document: Option<DocumentSnapshot>,
}

impl From<AppSnapshot> for WorkspaceSnapshot {
    fn from(snapshot: AppSnapshot) -> Self {
        Self {
            diagnostic_operation: snapshot.diagnostic_operation,
            revision: snapshot.revision,
            counter: snapshot.diagnostic_counter,
            workspace_name: snapshot.workspace_name,
            workspace_root: snapshot
                .workspace_root
                .map(|root| root.to_string_lossy().into_owned()),
            recent_workspaces: snapshot
                .recent_workspaces
                .into_iter()
                .map(|item| RecentWorkspace {
                    root: item.root.to_string_lossy().into_owned(),
                    name: item.name,
                })
                .collect(),
            search_query: snapshot.search_query.0,
            search_results: snapshot
                .search_results
                .into_iter()
                .map(|result| SearchResult {
                    id: result.id.as_str().to_owned(),
                    title: result.title,
                    dirty: result.dirty,
                    snippet: result.snippet,
                    location: result.location.map(|location| SearchLocation {
                        line: location.line,
                        column: location.column,
                        start_char: location.start_char,
                        end_char: location.end_char,
                        start_utf16: location.start_utf16,
                        end_utf16: location.end_utf16,
                    }),
                })
                .collect(),
            search_in_progress: snapshot.search_in_progress,
            directories: snapshot.directories,
            documents: snapshot
                .documents
                .into_iter()
                .map(|doc| DocumentSummary {
                    id: doc.id.as_str().to_owned(),
                    title: doc.title,
                    dirty: doc.dirty,
                    external_change: doc.external_change,
                })
                .collect(),
            active_document: snapshot.active_document.map(|doc| DocumentSnapshot {
                id: doc.id.as_str().to_owned(),
                title: doc.title,
                content: doc.content,
                dirty: doc.dirty,
                external_change: doc.external_change,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WorkspaceError {
    #[error("no workspace is open")]
    WorkspaceNotOpen,
    #[error("document not found: {0}")]
    DocumentNotFound(String),
    #[error("document is not open: {0}")]
    DocumentNotOpen(String),
    #[error("save modified documents before changing workspaces")]
    UnsavedChanges,
    #[error("document changed on disk: {0}")]
    ExternalChanges(String),
    #[error("invalid document name")]
    InvalidDocumentId,
    #[error("a folder cannot be moved inside itself")]
    InvalidMove,
    #[error("filesystem error: {0}")]
    Filesystem(String),
    #[error("metadata storage error: {0}")]
    Storage(String),
    #[error("diagnostic counter overflow")]
    CounterOverflow,
    #[error("application revision overflow")]
    RevisionOverflow,
    #[error("application lock poisoned")]
    LockPoisoned,
}

impl From<AppError> for WorkspaceError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::InvalidMove => Self::InvalidMove,
            AppError::InvalidEntryName => Self::InvalidDocumentId,
            AppError::WorkspaceNotOpen => Self::WorkspaceNotOpen,
            AppError::DocumentNotFound(id) => Self::DocumentNotFound(id),
            AppError::DocumentNotOpen(id) => Self::DocumentNotOpen(id),
            AppError::UnsavedChanges => Self::UnsavedChanges,
            AppError::ExternalChanges(id) => Self::ExternalChanges(id),
            AppError::Filesystem(error) => Self::Filesystem(error.to_string()),
            AppError::Storage(error) => Self::Storage(error.to_string()),
            AppError::CounterOverflow => Self::CounterOverflow,
            AppError::RevisionOverflow => Self::RevisionOverflow,
        }
    }
}

#[derive(uniffi::Object)]
pub struct WorkspaceDropTrace {
    inner: native_workspace_application::SidebarDropTrace,
}

#[uniffi::export]
impl WorkspaceDropTrace {
    pub fn preview_hidden(&self) {
        self.inner.preview_hidden();
    }
    pub fn handler_finished(&self, accepted: bool) {
        self.inner.handler_finished(accepted);
    }
    pub fn sidebar_ready(&self) {
        self.inner.sidebar_ready();
    }
    pub fn confirmed(&self, success: bool) {
        self.inner.confirmed(success);
    }
}

#[derive(uniffi::Object)]
pub struct WorkspaceApplication {
    inner: Mutex<Application>,
    diagnostic_observer: std::sync::OnceLock<native_workspace_application::DiagnosticObserver>,
}

#[uniffi::export]
impl WorkspaceApplication {
    pub fn diagnostic_ui_heartbeat(&self) {
        if let Some(observer) = self.diagnostic_observer.get() {
            observer.ui_heartbeat();
        }
    }
    pub fn begin_sidebar_drop(&self, origin: u8) -> std::sync::Arc<WorkspaceDropTrace> {
        std::sync::Arc::new(WorkspaceDropTrace {
            inner: native_workspace_application::SidebarDropTrace::begin(
                self.diagnostic_observer.get().cloned().unwrap_or_default(),
                origin,
            ),
        })
    }

    pub fn move_entry_traced(
        &self,
        path: String,
        directory: String,
        trace: std::sync::Arc<WorkspaceDropTrace>,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?
            .dispatch_sidebar_move(Self::parse_id(path)?, directory, false, trace.inner.clone())
            .map(Into::into)
            .map_err(Into::into)
    }
    #[uniffi::constructor]
    pub fn new() -> Self {
        let mut app = Application::new();
        app.enable_devtools("swiftui");
        let diagnostic_observer = std::sync::OnceLock::new();
        let observer = app.diagnostic_observer();
        if observer.available() {
            let _ = diagnostic_observer.set(observer);
        }
        Self {
            inner: Mutex::new(app),
            diagnostic_observer,
        }
    }

    pub fn open_devtools(&self) -> bool {
        if self.diagnostic_observer.get().is_none() {
            let Ok(mut app) = self.inner.try_lock() else {
                return false;
            };
            app.start_devtools("swiftui");
            let observer = app.diagnostic_observer();
            if !observer.available() {
                return false;
            }
            let _ = self.diagnostic_observer.set(observer);
        }
        if let Some(observer) = self.diagnostic_observer.get() {
            observer.open_console();
            return true;
        }
        false
    }

    pub fn devtools_available(&self) -> bool {
        cfg!(feature = "devtools")
    }

    pub fn devtools_enabled(&self) -> bool {
        self.diagnostic_observer
            .get()
            .is_some_and(|observer| observer.enabled())
    }

    pub fn diagnostic_snapshot_applied(&self, operation: u64, revision: u64, duration_ns: u64) {
        if let Some(observer) = self.diagnostic_observer.get() {
            observer.snapshot_applied(operation, revision, duration_ns);
        }
    }

    pub fn snapshot(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        Ok(app.snapshot().into())
    }

    pub fn enable_persistence(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let mut app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        app.enable_persistence_default()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn increment(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::IncrementDiagnostic)
    }

    pub fn set_search_query(&self, query: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::QueueSearchQuery(query))
    }

    pub fn poll_search_results(&self) -> Result<Option<WorkspaceSnapshot>, WorkspaceError> {
        self.dispatch_if_changed(AppCommand::PollSearchResults)
    }

    pub fn open_workspace(&self, path: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::OpenWorkspace(PathBuf::from(path)))
    }

    pub fn refresh_workspace(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::RefreshWorkspace)
    }

    pub fn move_entry(
        &self,
        path: String,
        directory: String,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::MoveEntry {
            id: Self::parse_id(path)?,
            directory,
        })
    }

    pub fn rename_entry(
        &self,
        path: String,
        new_name: String,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::RenameEntry {
            id: Self::parse_id(path)?,
            new_name,
        })
    }

    pub fn create_entry(
        &self,
        path: String,
        directory: bool,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::CreateEntry {
            id: Self::parse_id(path)?,
            directory,
        })
    }

    pub fn poll_workspace_changes(&self) -> Result<Option<WorkspaceSnapshot>, WorkspaceError> {
        self.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
    }

    pub fn open_document(&self, id: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::OpenDocument(Self::parse_id(id)?))
    }

    pub fn open_document_fast(&self, id: String) -> Result<OpenedDocument, WorkspaceError> {
        let id = Self::parse_id(id)?;
        let mut app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        let snapshot = app.dispatch(AppCommand::OpenDocument(id.clone()))?;
        let document = snapshot
            .active_document
            .ok_or_else(|| WorkspaceError::DocumentNotOpen(id.as_str().to_owned()))?;
        Ok(OpenedDocument {
            revision: snapshot.revision,
            document: DocumentSnapshot {
                id: document.id.as_str().to_owned(),
                title: document.title,
                content: document.content,
                dirty: document.dirty,
                external_change: document.external_change,
            },
        })
    }

    pub fn change_document(
        &self,
        id: String,
        content: String,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::ChangeDocument {
            id: Self::parse_id(id)?,
            content,
        })
    }

    pub fn save_document(&self, id: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::SaveDocument(Self::parse_id(id)?))
    }

    pub fn reload_document(&self, id: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::ReloadDocument(Self::parse_id(id)?))
    }

    pub fn overwrite_document(&self, id: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::OverwriteDocument(Self::parse_id(id)?))
    }
}

impl WorkspaceApplication {
    fn parse_id(id: String) -> Result<DocumentId, WorkspaceError> {
        DocumentId::parse(id).map_err(|_| WorkspaceError::InvalidDocumentId)
    }

    fn dispatch(&self, command: AppCommand) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let mut app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        app.dispatch(command).map(Into::into).map_err(Into::into)
    }

    fn dispatch_if_changed(
        &self,
        command: AppCommand,
    ) -> Result<Option<WorkspaceSnapshot>, WorkspaceError> {
        let mut app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        app.dispatch_if_changed(command)
            .map(|snapshot| snapshot.map(Into::into))
            .map_err(Into::into)
    }
}

impl Default for WorkspaceApplication {
    fn default() -> Self {
        Self::new()
    }
}
