//! Shared application commands, events, state, and owned snapshots.

use native_workspace_domain::{DiagnosticState, DocumentId, Workspace};
use native_workspace_filesystem::{FilesystemError, RealFilesystem, WorkspaceFilesystem};
use std::path::PathBuf;

/// An intention requested by a frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    IncrementDiagnostic,
    OpenWorkspace(PathBuf),
    OpenDocument(DocumentId),
    CloseDocument(DocumentId),
    ChangeDocument { id: DocumentId, content: String },
    SaveDocument(DocumentId),
}

/// A fact produced by a successful command. Events are transient, not a persisted log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppEvent {
    DiagnosticIncremented,
    WorkspaceOpened {
        workspace: Workspace,
        documents: Vec<DocumentId>,
    },
    DocumentLoaded {
        id: DocumentId,
        content: String,
    },
    DocumentOpened {
        id: DocumentId,
    },
    DocumentClosed {
        id: DocumentId,
    },
    DocumentChanged {
        id: DocumentId,
        content: String,
    },
    DocumentSaved {
        id: DocumentId,
    },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOutcome {
    pub events: Vec<AppEvent>,
    pub snapshot: AppSnapshot,
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

/// Authoritative current state. Its fields change only through validated events.
#[derive(Debug, Default)]
pub struct AppState {
    revision: u64,
    diagnostic: DiagnosticState,
    workspace: Option<Workspace>,
    documents: Vec<DocumentEntry>,
    active_document: Option<DocumentId>,
}

impl AppState {
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

    fn apply(&mut self, event: &AppEvent) {
        match event {
            AppEvent::DiagnosticIncremented => {
                self.diagnostic
                    .increment()
                    .expect("diagnostic overflow checked before producing event");
            }
            AppEvent::WorkspaceOpened {
                workspace,
                documents,
            } => {
                self.workspace = Some(workspace.clone());
                self.documents = documents
                    .iter()
                    .cloned()
                    .map(|id| DocumentEntry {
                        id,
                        content: None,
                        saved_content: None,
                    })
                    .collect();
                self.active_document = None;
            }
            AppEvent::DocumentLoaded { id, content } => {
                let entry = self.document_mut(id);
                entry.content = Some(content.clone());
                entry.saved_content = Some(content.clone());
            }
            AppEvent::DocumentOpened { id } => self.active_document = Some(id.clone()),
            AppEvent::DocumentClosed { id } => {
                debug_assert_eq!(self.active_document.as_ref(), Some(id));
                self.active_document = None;
            }
            AppEvent::DocumentChanged { id, content } => {
                self.document_mut(id).content = Some(content.clone());
            }
            AppEvent::DocumentSaved { id } => {
                let entry = self.document_mut(id);
                entry.saved_content = entry.content.clone();
            }
        }
    }

    fn document_mut(&mut self, id: &DocumentId) -> &mut DocumentEntry {
        self.documents
            .iter_mut()
            .find(|entry| &entry.id == id)
            .expect("document validated before producing event")
    }
}

pub struct Application {
    state: AppState,
    filesystem: Box<dyn WorkspaceFilesystem>,
}

impl Application {
    pub fn new() -> Self {
        Self::with_filesystem(Box::new(RealFilesystem))
    }

    pub fn with_filesystem(filesystem: Box<dyn WorkspaceFilesystem>) -> Self {
        Self {
            state: AppState::default(),
            filesystem,
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.state.snapshot()
    }

    /// Compatibility path for frontends that only render the resulting snapshot.
    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        self.dispatch_with_events(command)
            .map(|outcome| outcome.snapshot)
    }

    /// Performs external I/O first, then applies the resulting events to state.
    /// Failed and no-op commands produce no events and do not advance revision.
    pub fn dispatch_with_events(&mut self, command: AppCommand) -> Result<AppOutcome, AppError> {
        let next_revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        let events = self.resolve(command)?;
        if !events.is_empty() {
            for event in &events {
                self.state.apply(event);
            }
            self.state.revision = next_revision;
        }
        Ok(AppOutcome {
            events,
            snapshot: self.snapshot(),
        })
    }

    fn resolve(&self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let events = match command {
            AppCommand::IncrementDiagnostic => {
                self.state
                    .diagnostic
                    .counter
                    .checked_add(1)
                    .ok_or(AppError::CounterOverflow)?;
                vec![AppEvent::DiagnosticIncremented]
            }
            AppCommand::OpenWorkspace(path) => {
                if self.state.documents.iter().any(DocumentEntry::dirty) {
                    return Err(AppError::UnsavedChanges);
                }
                let (workspace, documents) = self.filesystem.open_workspace(&path)?;
                vec![AppEvent::WorkspaceOpened {
                    workspace,
                    documents,
                }]
            }
            AppCommand::OpenDocument(id) => {
                let root = &self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                let entry = self
                    .state
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let mut events = Vec::new();
                if entry.content.is_none() {
                    let content = self.filesystem.read_document(root, &id)?;
                    events.push(AppEvent::DocumentLoaded {
                        id: id.clone(),
                        content,
                    });
                }
                if self.state.active_document.as_ref() != Some(&id) {
                    events.push(AppEvent::DocumentOpened { id });
                }
                events
            }
            AppCommand::CloseDocument(id) => {
                let entry = self
                    .state
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                if entry.dirty() {
                    return Err(AppError::UnsavedChanges);
                }
                if self.state.active_document.as_ref() == Some(&id) {
                    vec![AppEvent::DocumentClosed { id }]
                } else {
                    Vec::new()
                }
            }
            AppCommand::ChangeDocument { id, content } => {
                self.state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?;
                let entry = self
                    .state
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let current = entry
                    .content
                    .as_ref()
                    .ok_or_else(|| AppError::DocumentNotOpen(id.as_str().to_owned()))?;
                if current == &content {
                    Vec::new()
                } else {
                    vec![AppEvent::DocumentChanged { id, content }]
                }
            }
            AppCommand::SaveDocument(id) => {
                let root = &self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                let entry = self
                    .state
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let content = entry
                    .content
                    .as_ref()
                    .ok_or_else(|| AppError::DocumentNotOpen(id.as_str().to_owned()))?;
                if entry.dirty() {
                    self.filesystem.write_document(root, &id, content)?;
                    vec![AppEvent::DocumentSaved { id }]
                } else {
                    Vec::new()
                }
            }
        };
        Ok(events)
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
        let outcome = app
            .dispatch_with_events(AppCommand::IncrementDiagnostic)
            .unwrap();
        assert_eq!(outcome.events, [AppEvent::DiagnosticIncremented]);
        let after = outcome.snapshot;
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

    #[test]
    fn events_follow_successful_changes_and_keep_one_revision_per_command() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "original").unwrap();
        let mut app = Application::new();

        let listed = app
            .dispatch_with_events(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        assert!(matches!(
            listed.events.as_slice(),
            [AppEvent::WorkspaceOpened { .. }]
        ));
        assert_eq!(listed.snapshot.revision, 1);

        let opened = app
            .dispatch_with_events(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        assert_eq!(
            opened.events,
            [
                AppEvent::DocumentLoaded {
                    id: id("note.md"),
                    content: "original".into()
                },
                AppEvent::DocumentOpened { id: id("note.md") }
            ]
        );
        assert_eq!(opened.snapshot.revision, 2);
        assert_eq!(app.state().snapshot(), opened.snapshot);

        let no_op = app
            .dispatch_with_events(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        assert!(no_op.events.is_empty());
        assert_eq!(no_op.snapshot.revision, 2);

        let changed = app
            .dispatch_with_events(AppCommand::ChangeDocument {
                id: id("note.md"),
                content: "edited".into(),
            })
            .unwrap();
        assert_eq!(
            changed.events,
            [AppEvent::DocumentChanged {
                id: id("note.md"),
                content: "edited".into()
            }]
        );
        assert!(changed.snapshot.documents[0].dirty);

        let saved = app
            .dispatch_with_events(AppCommand::SaveDocument(id("note.md")))
            .unwrap();
        assert_eq!(
            saved.events,
            [AppEvent::DocumentSaved { id: id("note.md") }]
        );
        assert!(!saved.snapshot.documents[0].dirty);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "edited"
        );

        let already_saved = app
            .dispatch_with_events(AppCommand::SaveDocument(id("note.md")))
            .unwrap();
        assert!(already_saved.events.is_empty());
        assert_eq!(already_saved.snapshot.revision, saved.snapshot.revision);

        let closed = app
            .dispatch_with_events(AppCommand::CloseDocument(id("note.md")))
            .unwrap();
        assert_eq!(
            closed.events,
            [AppEvent::DocumentClosed { id: id("note.md") }]
        );
        assert!(closed.snapshot.active_document.is_none());
    }

    struct FailingWriteFilesystem;

    impl WorkspaceFilesystem for FailingWriteFilesystem {
        fn open_workspace(
            &self,
            path: &std::path::Path,
        ) -> Result<(Workspace, Vec<DocumentId>), FilesystemError> {
            RealFilesystem.open_workspace(path)
        }

        fn read_document(
            &self,
            root: &std::path::Path,
            id: &DocumentId,
        ) -> Result<String, FilesystemError> {
            RealFilesystem.read_document(root, id)
        }

        fn write_document(
            &self,
            root: &std::path::Path,
            id: &DocumentId,
            _content: &str,
        ) -> Result<(), FilesystemError> {
            Err(FilesystemError::Io {
                path: root.join(id.as_str()),
                source: std::io::Error::other("write failed"),
            })
        }
    }

    #[test]
    fn failed_save_keeps_dirty_state_and_does_not_advance_revision() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "original").unwrap();
        let mut app = Application::with_filesystem(Box::new(FailingWriteFilesystem));
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "edited".into(),
        })
        .unwrap();
        let before = app.snapshot();

        assert!(matches!(
            app.dispatch_with_events(AppCommand::SaveDocument(id("note.md"))),
            Err(AppError::Filesystem(_))
        ));
        assert_eq!(app.snapshot(), before);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "original"
        );
    }
}
