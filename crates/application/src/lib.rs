//! Shared application commands, state, and owned snapshots.

use native_workspace_domain::{CounterOverflow, DiagnosticState, DocumentId, Workspace};
use native_workspace_filesystem::{FilesystemError, RealFilesystem, WorkspaceFilesystem};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    IncrementDiagnostic,
    OpenWorkspace(PathBuf),
    OpenDocument(DocumentId),
    CloseDocument(DocumentId),
    ChangeDocument { id: DocumentId, content: String },
    SaveDocument(DocumentId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSummary {
    pub id: DocumentId,
    pub title: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub title: String,
    pub content: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub diagnostic_counter: u64,
    pub workspace_name: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub documents: Vec<DocumentSummary>,
    pub active_document: Option<DocumentSnapshot>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("no workspace is open")]
    WorkspaceNotOpen,
    #[error("document not found: {0}")]
    DocumentNotFound(String),
    #[error("document is not open: {0}")]
    DocumentNotOpen(String),
    #[error("save modified documents before changing workspaces or closing them")]
    UnsavedChanges,
    #[error("diagnostic counter overflow")]
    CounterOverflow,
    #[error("application revision overflow")]
    RevisionOverflow,
    #[error(transparent)]
    Filesystem(#[from] FilesystemError),
}

#[derive(Debug)]
struct DocumentEntry {
    id: DocumentId,
    content: Option<String>,
    saved_content: Option<String>,
}

impl DocumentEntry {
    fn dirty(&self) -> bool {
        self.content != self.saved_content
    }
}

pub struct Application {
    revision: u64,
    diagnostic: DiagnosticState,
    filesystem: Box<dyn WorkspaceFilesystem>,
    workspace: Option<Workspace>,
    documents: Vec<DocumentEntry>,
    active_document: Option<DocumentId>,
}

impl Application {
    pub fn new() -> Self {
        Self::with_filesystem(Box::new(RealFilesystem))
    }

    pub fn with_filesystem(filesystem: Box<dyn WorkspaceFilesystem>) -> Self {
        Self {
            revision: 0,
            diagnostic: DiagnosticState::default(),
            filesystem,
            workspace: None,
            documents: Vec::new(),
            active_document: None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let active_document = self.active_document.as_ref().and_then(|id| {
            self.documents
                .iter()
                .find(|entry| &entry.id == id)
                .and_then(|entry| {
                    entry.content.as_ref().map(|content| DocumentSnapshot {
                        id: entry.id.clone(),
                        title: entry.id.as_str().to_owned(),
                        content: content.clone(),
                        dirty: entry.dirty(),
                    })
                })
        });
        AppSnapshot {
            revision: self.revision,
            diagnostic_counter: self.diagnostic.counter,
            workspace_name: self
                .workspace
                .as_ref()
                .map(|workspace| workspace.name.clone()),
            workspace_root: self
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone()),
            documents: self
                .documents
                .iter()
                .map(|entry| DocumentSummary {
                    id: entry.id.clone(),
                    title: entry.id.as_str().to_owned(),
                    dirty: entry.dirty(),
                })
                .collect(),
            active_document,
        }
    }

    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        let changed = match command {
            AppCommand::IncrementDiagnostic => {
                self.diagnostic
                    .increment()
                    .map_err(|CounterOverflow| AppError::CounterOverflow)?;
                true
            }
            AppCommand::OpenWorkspace(path) => {
                if self.documents.iter().any(DocumentEntry::dirty) {
                    return Err(AppError::UnsavedChanges);
                }
                let (workspace, ids) = self.filesystem.open_workspace(&path)?;
                self.documents = ids
                    .into_iter()
                    .map(|id| DocumentEntry {
                        id,
                        content: None,
                        saved_content: None,
                    })
                    .collect();
                self.workspace = Some(workspace);
                self.active_document = None;
                true
            }
            AppCommand::OpenDocument(id) => {
                let root = &self
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                let entry = self
                    .documents
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let was_loaded = entry.content.is_some();
                if !was_loaded {
                    let content = self.filesystem.read_document(root, &id)?;
                    entry.saved_content = Some(content.clone());
                    entry.content = Some(content);
                }
                let changed = !was_loaded || self.active_document.as_ref() != Some(&id);
                self.active_document = Some(id);
                changed
            }
            AppCommand::CloseDocument(id) => {
                let entry = self
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                if entry.dirty() {
                    return Err(AppError::UnsavedChanges);
                }
                if self.active_document.as_ref() == Some(&id) {
                    self.active_document = None;
                    true
                } else {
                    false
                }
            }
            AppCommand::ChangeDocument { id, content } => {
                self.workspace.as_ref().ok_or(AppError::WorkspaceNotOpen)?;
                let entry = self
                    .documents
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let current = entry
                    .content
                    .as_mut()
                    .ok_or_else(|| AppError::DocumentNotOpen(id.as_str().to_owned()))?;
                if *current == content {
                    false
                } else {
                    *current = content;
                    true
                }
            }
            AppCommand::SaveDocument(id) => {
                let root = &self
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                let entry = self
                    .documents
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let content = entry
                    .content
                    .as_ref()
                    .ok_or_else(|| AppError::DocumentNotOpen(id.as_str().to_owned()))?;
                if entry.dirty() {
                    self.filesystem.write_document(root, &id, content)?;
                    entry.saved_content = Some(content.clone());
                    true
                } else {
                    false
                }
            }
        };
        if changed {
            self.revision = next_revision;
        }
        Ok(self.snapshot())
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn id(name: &str) -> DocumentId {
        DocumentId::parse(name).unwrap()
    }

    #[test]
    fn diagnostic_command_remains_available() {
        let mut app = Application::new();
        let before = app.snapshot();
        let after = app.dispatch(AppCommand::IncrementDiagnostic).unwrap();
        assert_eq!(before.diagnostic_counter, 0);
        assert_eq!(after.diagnostic_counter, 1);
        assert_eq!(after.revision, 1);
    }

    #[test]
    fn workspace_edit_save_round_trip_and_dirty_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "original").unwrap();
        let mut app = Application::new();
        let listed = app
            .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        assert_eq!(listed.documents.len(), 1);
        assert_eq!(listed.documents[0].id, id("note.md"));
        let opened = app
            .dispatch(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        assert_eq!(opened.active_document.unwrap().content, "original");
        let changed = app
            .dispatch(AppCommand::ChangeDocument {
                id: id("note.md"),
                content: "edited".into(),
            })
            .unwrap();
        assert!(changed.documents[0].dirty);
        assert!(changed.active_document.unwrap().dirty);
        let saved = app
            .dispatch(AppCommand::SaveDocument(id("note.md")))
            .unwrap();
        assert!(!saved.documents[0].dirty);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "edited"
        );
    }

    #[test]
    fn unsaved_content_survives_switch_and_blocks_workspace_replacement() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("a.md"),
            content: "edited".into(),
        })
        .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("b.md"))).unwrap();
        assert!(matches!(
            app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf())),
            Err(AppError::UnsavedChanges)
        ));
        let reopened = app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
        assert_eq!(reopened.active_document.unwrap().content, "edited");
        assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "a");
    }

    #[test]
    fn invalid_operation_preserves_revision() {
        let mut app = Application::new();
        assert!(matches!(
            app.dispatch(AppCommand::OpenDocument(id("missing.md"))),
            Err(AppError::WorkspaceNotOpen)
        ));
        assert_eq!(app.snapshot().revision, 0);
    }
}
