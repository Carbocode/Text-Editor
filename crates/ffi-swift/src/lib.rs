//! The only UniFFI boundary in the repository.

use native_workspace_application::{AppCommand, AppError, AppSnapshot, Application};
use native_workspace_domain::DocumentId;
use std::{path::PathBuf, sync::Mutex};

uniffi::setup_scaffolding!();

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
pub struct WorkspaceApplication {
    inner: Mutex<Application>,
}

#[uniffi::export]
impl WorkspaceApplication {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Application::new()),
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

    pub fn poll_workspace_changes(&self) -> Result<Option<WorkspaceSnapshot>, WorkspaceError> {
        self.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
    }

    pub fn open_document(&self, id: String) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.dispatch(AppCommand::OpenDocument(Self::parse_id(id)?))
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
