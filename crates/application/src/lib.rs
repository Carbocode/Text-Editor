//! Shared application commands, events, state, and owned snapshots.

use native_workspace_domain::{DiagnosticState, DocumentId, Workspace};
use native_workspace_filesystem::{FilesystemError, RealFilesystem, WorkspaceFilesystem};
pub use native_workspace_storage::RecentWorkspace;
use native_workspace_storage::{MemoryStorage, MetadataStorage, SqliteStorage, StorageError};
use std::path::PathBuf;

/// An intention requested by a frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    IncrementDiagnostic,
    SetSearchQuery(String),
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
    SearchQueryChanged {
        query: SearchQuery,
        results: Vec<SearchResult>,
    },
    WorkspaceOpened {
        workspace: Workspace,
        documents: Vec<DocumentId>,
        recent_workspaces: Vec<RecentWorkspace>,
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

/// A search query, kept as entered so frontends can render the same text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchQuery(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub id: DocumentId,
    pub title: String,
    pub dirty: bool,
    pub snippet: Option<String>,
}

fn search_result(
    entry: &DocumentEntry,
    needle: &str,
    content: Option<&str>,
) -> Option<SearchResult> {
    let matching_line = if needle.is_empty() {
        None
    } else {
        content.and_then(|content| {
            content
                .lines()
                .find(|line| line.to_lowercase().contains(needle))
        })
    };
    if !needle.is_empty()
        && !entry.id.as_str().to_lowercase().contains(needle)
        && matching_line.is_none()
    {
        return None;
    }
    let snippet = matching_line.map(|line| {
        let line = line.trim();
        let lowercase = line.to_lowercase();
        let match_start = lowercase
            .find(needle)
            .map(|byte_index| lowercase[..byte_index].chars().count())
            .unwrap_or(0);
        let start = match_start.saturating_sub(40);
        let mut chars = line.chars().skip(start);
        let preview: String = chars.by_ref().take(160).collect();
        format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            preview,
            if chars.next().is_some() { "…" } else { "" }
        )
    });
    Some(SearchResult {
        id: entry.id.clone(),
        title: entry.id.as_str().to_owned(),
        dirty: entry.dirty(),
        snippet,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub diagnostic_counter: u64,
    pub workspace_name: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub search_query: SearchQuery,
    pub search_results: Vec<SearchResult>,
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
    #[error(transparent)]
    Storage(#[from] StorageError),
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
    recent_workspaces: Vec<RecentWorkspace>,
    search_query: SearchQuery,
    search_results: Vec<SearchResult>,
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
            recent_workspaces: self.recent_workspaces.clone(),
            search_query: self.search_query.clone(),
            search_results: self.search_results.clone(),
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
            AppEvent::SearchQueryChanged { query, results } => {
                self.search_query = query.clone();
                self.search_results = results.clone();
            }
            AppEvent::WorkspaceOpened {
                workspace,
                documents,
                recent_workspaces,
            } => {
                self.workspace = Some(workspace.clone());
                self.search_query = SearchQuery::default();
                self.recent_workspaces = recent_workspaces.clone();
                self.documents = documents
                    .iter()
                    .cloned()
                    .map(|id| DocumentEntry {
                        id,
                        content: None,
                        saved_content: None,
                    })
                    .collect();
                self.search_results = self
                    .documents
                    .iter()
                    .filter_map(|entry| search_result(entry, "", None))
                    .collect();
                self.active_document = None;
            }
            AppEvent::DocumentLoaded { id, content } => {
                let entry = self.document_mut(id);
                entry.content = Some(content.clone());
                entry.saved_content = Some(content.clone());
                self.refresh_search_result(id);
            }
            AppEvent::DocumentOpened { id } => self.active_document = Some(id.clone()),
            AppEvent::DocumentClosed { id } => {
                debug_assert_eq!(self.active_document.as_ref(), Some(id));
                self.active_document = None;
            }
            AppEvent::DocumentChanged { id, content } => {
                self.document_mut(id).content = Some(content.clone());
                self.refresh_search_result(id);
            }
            AppEvent::DocumentSaved { id } => {
                let entry = self.document_mut(id);
                entry.saved_content = entry.content.clone();
                self.refresh_search_result(id);
            }
        }
    }

    fn document_mut(&mut self, id: &DocumentId) -> &mut DocumentEntry {
        self.documents
            .iter_mut()
            .find(|entry| &entry.id == id)
            .expect("document validated before producing event")
    }

    fn refresh_search_result(&mut self, id: &DocumentId) {
        let needle = self.search_query.0.trim().to_lowercase();
        let index = self
            .documents
            .iter()
            .position(|entry| &entry.id == id)
            .expect("document validated before producing event");
        let entry = &self.documents[index];
        let result = search_result(entry, &needle, entry.content.as_deref());
        self.search_results.retain(|result| &result.id != id);
        if let Some(result) = result {
            let position = self.search_results.partition_point(|existing| {
                self.documents
                    .iter()
                    .position(|entry| entry.id == existing.id)
                    .is_some_and(|existing_index| existing_index < index)
            });
            self.search_results.insert(position, result);
        }
    }
}

pub struct Application {
    state: AppState,
    filesystem: Box<dyn WorkspaceFilesystem>,
    storage: Box<dyn MetadataStorage>,
}

impl Application {
    pub fn new() -> Self {
        Self::with_filesystem(Box::new(RealFilesystem))
    }

    pub fn with_filesystem(filesystem: Box<dyn WorkspaceFilesystem>) -> Self {
        Self {
            state: AppState::default(),
            filesystem,
            storage: Box::new(MemoryStorage::default()),
        }
    }

    /// Attach the user's SQLite metadata database before processing commands.
    pub fn enable_persistence_default(&mut self) -> Result<AppSnapshot, AppError> {
        self.enable_persistence(SqliteStorage::open_default()?)
    }

    pub fn enable_persistence(&mut self, storage: SqliteStorage) -> Result<AppSnapshot, AppError> {
        let recent = storage.recent_workspaces()?;
        self.storage = Box::new(storage);
        self.state.recent_workspaces = recent;
        Ok(self.snapshot())
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

    fn resolve(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let events = match command {
            AppCommand::IncrementDiagnostic => {
                self.state
                    .diagnostic
                    .counter
                    .checked_add(1)
                    .ok_or(AppError::CounterOverflow)?;
                vec![AppEvent::DiagnosticIncremented]
            }
            AppCommand::SetSearchQuery(query) => {
                if self.state.search_query.0 == query {
                    Vec::new()
                } else {
                    let needle = query.trim().to_lowercase();
                    let root = self
                        .state
                        .workspace
                        .as_ref()
                        .map(|workspace| &workspace.root);
                    let results = self
                        .state
                        .documents
                        .iter()
                        .filter_map(|entry| {
                            let disk_content = if needle.is_empty() || entry.content.is_some() {
                                None
                            } else {
                                root.and_then(|root| {
                                    self.filesystem.read_document_for_search(root, &entry.id)
                                })
                            };
                            search_result(
                                entry,
                                &needle,
                                entry.content.as_deref().or(disk_content.as_deref()),
                            )
                        })
                        .collect();
                    vec![AppEvent::SearchQueryChanged {
                        query: SearchQuery(query),
                        results,
                    }]
                }
            }
            AppCommand::OpenWorkspace(path) => {
                if self.state.documents.iter().any(DocumentEntry::dirty) {
                    return Err(AppError::UnsavedChanges);
                }
                let (workspace, documents) = self.filesystem.open_workspace(&path)?;
                let recent_workspaces = self.storage.record_workspace(&workspace)?;
                vec![AppEvent::WorkspaceOpened {
                    workspace,
                    documents,
                    recent_workspaces,
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
    fn filename_search_filters_in_core_and_keeps_open_document() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Alpha.md"), "one").unwrap();
        fs::write(dir.path().join("beta.txt"), "two").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("beta.txt")))
            .unwrap();
        let outcome = app
            .dispatch_with_events(AppCommand::SetSearchQuery("  ALP  ".into()))
            .unwrap();
        assert!(matches!(
            outcome.events.as_slice(),
            [AppEvent::SearchQueryChanged { query, results }]
                if query.0 == "  ALP  " && results.len() == 1
        ));
        assert_eq!(outcome.snapshot.search_results.len(), 1);
        assert_eq!(outcome.snapshot.search_results[0].id, id("Alpha.md"));
        assert_eq!(outcome.snapshot.active_document.unwrap().id, id("beta.txt"));
        let revision = outcome.snapshot.revision;
        let unchanged = app
            .dispatch(AppCommand::SetSearchQuery("  ALP  ".into()))
            .unwrap();
        assert_eq!(unchanged.revision, revision);
        let cleared = app
            .dispatch(AppCommand::SetSearchQuery(String::new()))
            .unwrap();
        assert_eq!(cleared.search_results.len(), 2);
    }

    #[test]
    fn opening_workspace_resets_search_query() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join("first.md"), "one").unwrap();
        fs::write(second.path().join("second.md"), "two").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(first.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::SetSearchQuery("first".into()))
            .unwrap();
        let snapshot = app
            .dispatch(AppCommand::OpenWorkspace(second.path().to_path_buf()))
            .unwrap();
        assert_eq!(snapshot.search_query, SearchQuery::default());
        assert_eq!(snapshot.search_results[0].id, id("second.md"));
    }

    #[test]
    fn content_search_finds_unopened_files_and_returns_matching_line() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("alpha.md"),
            "heading\n  A Hidden Phrase here\nend",
        )
        .unwrap();
        fs::write(dir.path().join("beta.txt"), "unrelated").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("  hidden phrase  ".into()))
            .unwrap();
        assert_eq!(found.search_results.len(), 1);
        assert_eq!(found.search_results[0].id, id("alpha.md"));
        assert_eq!(
            found.search_results[0].snippet.as_deref(),
            Some("A Hidden Phrase here")
        );
        assert!(found.active_document.is_none());
    }

    #[test]
    fn content_preview_keeps_match_visible_in_long_line() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("note.md"),
            format!("{}needle", "x".repeat(300)),
        )
        .unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("needle".into()))
            .unwrap();
        assert!(
            found.search_results[0]
                .snippet
                .as_deref()
                .unwrap()
                .contains("needle")
        );
    }

    #[test]
    fn content_search_tracks_unsaved_edits_and_saved_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "original").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        app.dispatch(AppCommand::SetSearchQuery("draft".into()))
            .unwrap();
        let changed = app
            .dispatch(AppCommand::ChangeDocument {
                id: id("note.md"),
                content: "a draft line".into(),
            })
            .unwrap();
        assert_eq!(
            changed.search_results[0].snippet.as_deref(),
            Some("a draft line")
        );
        assert!(changed.search_results[0].dirty);
        let saved = app
            .dispatch(AppCommand::SaveDocument(id("note.md")))
            .unwrap();
        assert!(!saved.search_results[0].dirty);
        let changed_again = app
            .dispatch(AppCommand::ChangeDocument {
                id: id("note.md"),
                content: "without that word".into(),
            })
            .unwrap();
        assert!(changed_again.search_results.is_empty());
    }

    #[test]
    fn recent_workspaces_survive_application_restart() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("metadata.sqlite3");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let mut app = Application::new();
        let initial = app
            .enable_persistence(SqliteStorage::open(&db).unwrap())
            .unwrap();
        assert_eq!(initial.revision, 0);
        app.dispatch(AppCommand::OpenWorkspace(first.clone()))
            .unwrap();
        app.dispatch(AppCommand::OpenWorkspace(second.clone()))
            .unwrap();
        let snapshot = app
            .dispatch(AppCommand::OpenWorkspace(first.clone()))
            .unwrap();
        assert_eq!(snapshot.recent_workspaces.len(), 2);
        assert_eq!(
            snapshot.recent_workspaces[0].root,
            fs::canonicalize(first).unwrap()
        );
        drop(app);

        let mut reopened = Application::new();
        let restored = reopened
            .enable_persistence(SqliteStorage::open(&db).unwrap())
            .unwrap();
        assert_eq!(restored.revision, 0);
        assert!(restored.workspace_name.is_none());
        assert_eq!(restored.recent_workspaces, snapshot.recent_workspaces);
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
