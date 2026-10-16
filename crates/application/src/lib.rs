//! Shared application commands, events, state, and owned snapshots.

mod diagnostics;
mod drop_diagnostics;
mod native_application;
mod path_filter;
mod search;
mod sidebar_tree;
pub use drop_diagnostics::SidebarDropTrace;
pub use native_application::NativeApplication;
pub use path_filter::{PathFilter, PathFilterResult};
use search::{Matcher, SearchBatch, SearchJob};
pub use sidebar_tree::SidebarChild;
mod icons;
pub use diagnostics::DiagnosticObserver;
use diagnostics::{Context, Diagnostics};
#[cfg(test)]
mod delete_tests;
#[cfg(test)]
mod optimistic_move_tests;
#[cfg(test)]
mod search_tests;
pub use icons::{file_icon_name, folder_icon_name};

use native_workspace_domain::{DiagnosticState, DocumentId, Workspace};
use native_workspace_filesystem::{
    FilesystemError, RealFilesystem, WorkspaceFilesystem, WorkspaceWatcher,
};
pub use native_workspace_storage::RecentWorkspace;
use native_workspace_storage::{MemoryStorage, MetadataStorage, SqliteStorage, StorageError};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

/// An intention requested by a frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    IncrementDiagnostic,
    SetSearchQuery(String),
    QueueSearchQuery(String),
    PollSearchResults,
    OpenWorkspace(PathBuf),
    CreateEntry {
        id: DocumentId,
        directory: bool,
    },
    RenameEntry {
        id: DocumentId,
        new_name: String,
    },
    MoveEntry {
        id: DocumentId,
        directory: String,
    },
    /// Publish the new tree immediately and confirm the filesystem move off-thread.
    MoveEntryOptimistic {
        id: DocumentId,
        directory: String,
    },
    DeleteEntry(DocumentId),
    RefreshWorkspace,
    PollWorkspaceChanges,
    OpenDocument(DocumentId),
    CloseDocument(DocumentId),
    DiscardAndCloseDocument(DocumentId),
    ChangeDocument {
        id: DocumentId,
        content: String,
    },
    SaveDocument(DocumentId),
    SaveAllDocuments,
    ReloadDocument(DocumentId),
    OverwriteDocument(DocumentId),
}

impl AppCommand {
    fn diagnostic_name(&self) -> &'static str {
        match self {
            Self::IncrementDiagnostic => "command.increment",
            Self::SetSearchQuery(_) => "command.search_sync",
            Self::QueueSearchQuery(_) => "command.search_input",
            Self::PollSearchResults => "command.search_poll",
            Self::OpenWorkspace(_) => "command.workspace_open",
            Self::CreateEntry { .. } => "command.entry_create",
            Self::RenameEntry { .. } => "command.entry_rename",
            Self::MoveEntry { .. } => "command.entry_move",
            Self::MoveEntryOptimistic { .. } => "command.entry_move_optimistic",
            Self::DeleteEntry(_) => "command.entry_delete",
            Self::RefreshWorkspace => "command.workspace_refresh",
            Self::PollWorkspaceChanges => "command.workspace_poll",
            Self::OpenDocument(_) => "command.document_open",
            Self::DiscardAndCloseDocument(_) => "command.document_discard_close",
            Self::CloseDocument(_) => "command.document_close",
            Self::ChangeDocument { .. } => "command.document_change",
            Self::SaveAllDocuments => "command.document_save_all",
            Self::SaveDocument(_) => "command.document_save",
            Self::ReloadDocument(_) => "command.document_reload",
            Self::OverwriteDocument(_) => "command.document_overwrite",
        }
    }
}

/// A fact produced by a successful command. Events are transient, not a persisted log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppEvent {
    EntryDeleted {
        id: DocumentId,
    },
    EntryRenamed {
        id: DocumentId,
        target: DocumentId,
    },
    EntryMoved {
        id: DocumentId,
        target: DocumentId,
    },
    DiagnosticIncremented,
    SearchQueryChanged {
        query: SearchQuery,
        results: Vec<SearchResult>,
        truncated: bool,
    },
    /// The query changed and the worker will publish its results, so typing
    /// never scans the workspace on the UI thread.
    SearchQueryQueued(SearchQuery),
    /// Matches from the search worker, appended as they stream in or
    /// replacing the current results when a re-search finishes.
    SearchResultsUpdated {
        results: Vec<SearchResult>,
        replace: bool,
        finished: bool,
        truncated: bool,
    },
    SearchStarted,
    WorkspaceOpened {
        workspace: Workspace,
        documents: Vec<DocumentId>,
        directories: Vec<String>,
        recent_workspaces: Vec<RecentWorkspace>,
    },
    WorkspaceRefreshed {
        documents: Vec<DocumentEntry>,
        directories: Vec<String>,
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
    pub external_change: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub title: String,
    pub content: String,
    pub dirty: bool,
    pub external_change: bool,
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
    pub location: Option<SearchLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchLocation {
    pub line: u64,
    pub column: u64,
    pub start_char: u64,
    pub end_char: u64,
    pub start_utf16: u64,
    pub end_utf16: u64,
}

/// Every match of `needle` in `content`. An empty query lists the document itself.
fn search_results_for(
    entry: &DocumentEntry,
    needle: &str,
    content: Option<&str>,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    if needle.is_empty() {
        results.push(SearchResult {
            id: entry.id.clone(),
            title: entry.id.as_str().to_owned(),
            dirty: entry.dirty(),
            snippet: None,
            location: None,
        });
    } else if let Some(content) = content {
        Matcher::new(needle).content_results(entry, content, &mut results);
    }
    results
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    /// Opaque correlation only; never used to resolve application commands.
    pub diagnostic_operation: u64,
    pub revision: u64,
    pub diagnostic_counter: u64,
    pub workspace_name: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub search_query: SearchQuery,
    /// Shared, so publishing or keeping a snapshot never copies every row.
    pub search_results: Arc<Vec<SearchResult>>,
    pub search_in_progress: bool,
    /// The search stopped at its result limit; more matches may exist.
    pub search_truncated: bool,
    pub directories: Arc<Vec<String>>,
    pub documents: Arc<Vec<DocumentSummary>>,
    /// Open tabs in insertion order, independent of the workspace file listing.
    pub open_documents: Vec<DocumentId>,
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
    #[error("enter a single file or folder name")]
    InvalidEntryName,
    #[error("a folder cannot be moved inside itself")]
    InvalidMove,
    #[error("document not found: {0}")]
    DocumentNotFound(String),
    #[error("document is not open: {0}")]
    DocumentNotOpen(String),
    #[error("save modified documents before changing workspaces or closing them")]
    UnsavedChanges,
    #[error("save all modified documents in this file or folder before deleting it")]
    DeleteUnsavedChanges,
    #[error("a file operation is in progress; wait for it to finish")]
    DeleteInProgress,
    #[error("document changed on disk; reload it or explicitly overwrite the disk version: {0}")]
    ExternalChanges(String),
    #[error("diagnostic counter overflow")]
    CounterOverflow,
    #[error("application revision overflow")]
    RevisionOverflow,
    #[error(transparent)]
    Filesystem(#[from] FilesystemError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentEntry {
    id: DocumentId,
    content: Option<String>,
    saved_content: Option<String>,
    external_change: bool,
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
    search_in_progress: bool,
    search_truncated: bool,
    /// Documents whose results were recomputed from their buffer while a
    /// search ran; that search's possibly older matches for them are dropped.
    search_touched: HashSet<DocumentId>,
    directories: Vec<String>,
    documents: Vec<DocumentEntry>,
    open_documents: Vec<DocumentId>,
    active_document: Option<DocumentId>,
    /// Lists published by the last snapshot. Unchanged lists keep the same
    /// `Arc`, so frontends skip reconverting tens of thousands of paths.
    published_documents: OnceLock<Arc<Vec<DocumentSummary>>>,
    published_directories: OnceLock<Arc<Vec<String>>>,
    published_search_results: OnceLock<Arc<Vec<SearchResult>>>,
}

fn renamed_path(path: &str, id: &DocumentId, target: &DocumentId) -> String {
    if path == id.as_str() {
        target.as_str().to_owned()
    } else if let Some(suffix) = path
        .strip_prefix(id.as_str())
        .and_then(|suffix| suffix.strip_prefix('/'))
    {
        format!("{}/{suffix}", target.as_str())
    } else {
        path.to_owned()
    }
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
                        external_change: entry.external_change,
                    })
                })
        });
        AppSnapshot {
            diagnostic_operation: 0,
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
            search_results: self
                .published_search_results
                .get_or_init(|| Arc::new(self.search_results.clone()))
                .clone(),
            search_in_progress: self.search_in_progress,
            search_truncated: self.search_truncated,
            directories: self
                .published_directories
                .get_or_init(|| Arc::new(self.directories.clone()))
                .clone(),
            documents: self
                .published_documents
                .get_or_init(|| {
                    Arc::new(
                        self.documents
                            .iter()
                            .map(|entry| DocumentSummary {
                                id: entry.id.clone(),
                                title: entry.id.as_str().to_owned(),
                                dirty: entry.dirty(),
                                external_change: entry.external_change,
                            })
                            .collect(),
                    )
                })
                .clone(),
            open_documents: self.open_documents.clone(),
            active_document,
        }
    }

    fn apply(&mut self, event: &AppEvent) {
        match event {
            // These invalidate only the lists they actually change.
            AppEvent::DiagnosticIncremented
            | AppEvent::SearchStarted
            | AppEvent::SearchQueryQueued(_)
            | AppEvent::DocumentOpened { .. }
            | AppEvent::DocumentClosed { .. }
            | AppEvent::DocumentChanged { .. } => {}
            AppEvent::SearchQueryChanged { .. } | AppEvent::SearchResultsUpdated { .. } => {
                self.published_search_results = OnceLock::new();
            }
            _ => {
                self.published_documents = OnceLock::new();
                self.published_directories = OnceLock::new();
                self.published_search_results = OnceLock::new();
            }
        }
        match event {
            AppEvent::EntryDeleted { id } => {
                self.documents
                    .retain(|entry| !entry_is_within(entry.id.as_str(), id));
                self.directories.retain(|path| !entry_is_within(path, id));
                self.search_results
                    .retain(|result| !entry_is_within(result.id.as_str(), id));
                if self
                    .active_document
                    .as_ref()
                    .is_some_and(|active| entry_is_within(active.as_str(), id))
                {
                    self.active_document = None;
                }
            }
            AppEvent::EntryRenamed { id, target } | AppEvent::EntryMoved { id, target } => {
                let affected = |path: &str| {
                    path == id.as_str()
                        || path
                            .strip_prefix(id.as_str())
                            .is_some_and(|suffix| suffix.starts_with('/'))
                };
                for open in &mut self.open_documents {
                    *open = DocumentId::parse(renamed_path(open.as_str(), id, target)).unwrap();
                }
                for entry in &mut self.documents {
                    if affected(entry.id.as_str()) {
                        entry.id =
                            DocumentId::parse(renamed_path(entry.id.as_str(), id, target)).unwrap();
                    }
                }
                self.documents.sort_by(|a, b| a.id.cmp(&b.id));
                for path in &mut self.directories {
                    if affected(path) {
                        *path = renamed_path(path, id, target);
                    }
                }
                self.directories.sort();
                if let Some(active) = &mut self.active_document {
                    *active = DocumentId::parse(renamed_path(active.as_str(), id, target)).unwrap();
                }
                if self.search_query.0.trim().is_empty() {
                    self.list_documents_as_results();
                } else {
                    // Matches move with their files; nothing is searched again.
                    for result in &mut self.search_results {
                        if affected(result.id.as_str()) {
                            result.id =
                                DocumentId::parse(renamed_path(result.id.as_str(), id, target))
                                    .unwrap();
                            result.title = result.id.as_str().to_owned();
                        }
                    }
                }
            }
            AppEvent::DiagnosticIncremented => {
                self.diagnostic
                    .increment()
                    .expect("diagnostic overflow checked before producing event");
            }
            AppEvent::SearchQueryChanged {
                query,
                results,
                truncated,
            } => {
                self.search_query = query.clone();
                self.search_results = results.clone();
                self.search_in_progress = false;
                self.search_truncated = *truncated;
            }
            AppEvent::SearchQueryQueued(query) => {
                let needle = query.0.trim();
                if needle.is_empty() {
                    self.published_search_results = OnceLock::new();
                    self.list_documents_as_results();
                    self.search_in_progress = false;
                } else if !self.search_results.is_empty() {
                    // Results of the previous query no longer apply.
                    self.published_search_results = OnceLock::new();
                    self.search_results.clear();
                }
                self.search_query = query.clone();
                self.search_truncated = false;
            }
            AppEvent::SearchResultsUpdated {
                results,
                replace,
                finished,
                truncated,
            } => {
                let touched = std::mem::take(&mut self.search_touched);
                if *replace {
                    self.search_results = results.clone();
                    for id in &touched {
                        if self.documents.iter().any(|entry| &entry.id == id) {
                            self.refresh_search_result(id);
                        }
                    }
                } else {
                    self.search_results.extend(
                        results
                            .iter()
                            .filter(|result| !touched.contains(&result.id))
                            .cloned(),
                    );
                }
                if *finished {
                    self.search_in_progress = false;
                    self.search_truncated = *truncated;
                } else {
                    self.search_touched = touched;
                }
            }
            AppEvent::SearchStarted => {
                self.search_in_progress = true;
                self.search_touched.clear();
            }
            AppEvent::WorkspaceOpened {
                workspace,
                documents,
                directories,
                recent_workspaces,
            } => {
                self.open_documents.clear();
                self.workspace = Some(workspace.clone());
                self.search_query = SearchQuery::default();
                self.search_in_progress = false;
                self.recent_workspaces = recent_workspaces.clone();
                self.directories = directories.clone();
                self.documents = documents
                    .iter()
                    .cloned()
                    .map(|id| DocumentEntry {
                        id,
                        content: None,
                        saved_content: None,
                        external_change: false,
                    })
                    .collect();
                self.list_documents_as_results();
                self.active_document = None;
            }
            AppEvent::WorkspaceRefreshed {
                documents,
                directories,
            } => {
                self.documents = documents.clone();
                self.directories = directories.clone();
                if self.search_query.0.trim().is_empty() {
                    self.list_documents_as_results();
                } else {
                    // Like VS Code, disk changes do not rerun a search: removed
                    // files drop out and open buffers are matched again.
                    let listed: HashSet<&DocumentId> =
                        self.documents.iter().map(|entry| &entry.id).collect();
                    self.search_results
                        .retain(|result| listed.contains(&result.id));
                    let loaded: Vec<DocumentId> = self
                        .documents
                        .iter()
                        .filter(|entry| entry.content.is_some())
                        .map(|entry| entry.id.clone())
                        .collect();
                    for id in &loaded {
                        self.refresh_search_result(id);
                    }
                }
                if self
                    .active_document
                    .as_ref()
                    .is_some_and(|id| !self.documents.iter().any(|entry| &entry.id == id))
                {
                    self.active_document = None;
                }
            }
            AppEvent::DocumentLoaded { id, content } => {
                let entry = self.document_mut(id);
                entry.content = Some(content.clone());
                entry.saved_content = Some(content.clone());
                entry.external_change = false;
                self.refresh_search_result(id);
            }
            AppEvent::DocumentOpened { id } => {
                if !self.open_documents.contains(id) {
                    self.open_documents.push(id.clone());
                }
                self.active_document = Some(id.clone());
            }
            AppEvent::DocumentClosed { id } => {
                let index = self.open_documents.iter().position(|open| open == id);
                self.open_documents.retain(|open| open != id);
                if self.active_document.as_ref() == Some(id) {
                    self.active_document = index
                        .and_then(|i| {
                            self.open_documents
                                .get(i.min(self.open_documents.len().saturating_sub(1)))
                        })
                        .cloned();
                }
                let entry = self.document_mut(id);
                entry.content = None;
                entry.saved_content = None;
                entry.external_change = false;
                self.published_documents = OnceLock::new();
                self.published_search_results = OnceLock::new();
                self.refresh_search_result(id);
            }
            AppEvent::DocumentChanged { id, content } => {
                let entry = self.document_mut(id);
                let was_dirty = entry.dirty();
                entry.content = Some(content.clone());
                // Typing changes the summaries only when the dirty marker flips.
                if entry.dirty() != was_dirty {
                    self.published_documents = OnceLock::new();
                    self.published_search_results = OnceLock::new();
                } else if !self.search_query.0.trim().is_empty() {
                    self.published_search_results = OnceLock::new();
                }
                self.refresh_search_result(id);
            }
            AppEvent::DocumentSaved { id } => {
                let entry = self.document_mut(id);
                entry.saved_content = entry.content.clone();
                entry.external_change = false;
                self.refresh_search_result(id);
            }
        }
        if matches!(
            event,
            AppEvent::EntryDeleted { .. } | AppEvent::WorkspaceRefreshed { .. }
        ) {
            self.open_documents
                .retain(|id| self.documents.iter().any(|entry| &entry.id == id));
            if self.active_document.is_none() {
                self.active_document = self.open_documents.last().cloned();
            }
        }
    }

    fn document_mut(&mut self, id: &DocumentId) -> &mut DocumentEntry {
        self.documents
            .iter_mut()
            .find(|entry| &entry.id == id)
            .expect("document validated before producing event")
    }

    /// An empty query lists every document.
    fn list_documents_as_results(&mut self) {
        self.search_results = self
            .documents
            .iter()
            .flat_map(|entry| search_results_for(entry, "", None))
            .collect();
    }

    fn refresh_search_result(&mut self, id: &DocumentId) {
        if self.search_in_progress {
            self.search_touched.insert(id.clone());
        }
        let needle = self.search_query.0.trim().to_lowercase();
        let index = self
            .documents
            .iter()
            .position(|entry| &entry.id == id)
            .expect("document validated before producing event");
        let entry = &self.documents[index];
        if needle.is_empty()
            && let Some(result) = self.search_results.get_mut(index)
            && result.id == *id
        {
            result.dirty = entry.dirty();
            return;
        }
        let results = search_results_for(entry, &needle, entry.content.as_deref());
        self.search_results.retain(|result| &result.id != id);
        if !results.is_empty() {
            let order: HashMap<_, _> = self
                .documents
                .iter()
                .enumerate()
                .map(|(position, document)| (&document.id, position))
                .collect();
            let position = self.search_results.partition_point(|existing| {
                order
                    .get(&existing.id)
                    .is_some_and(|existing_index| *existing_index < index)
            });
            self.search_results.splice(position..position, results);
        }
    }
}

pub struct Application {
    diagnostics: Diagnostics,
    search_context: Context,
    command_context: Context,
    snapshot_operation: u64,
    state: AppState,
    filesystem: Arc<dyn WorkspaceFilesystem>,
    storage: Box<dyn MetadataStorage>,
    watcher: Option<WorkspaceWatcher>,
    last_reconcile: Instant,
    search_jobs: Sender<SearchJob>,
    search_results_rx: Receiver<SearchBatch>,
    search_generation: Arc<AtomicU64>,
    pending_move: Option<PendingMove>,
    pending_reconcile: Option<PendingReconcile>,
    sidebar_drop_trace: Option<SidebarDropTrace>,
    deferred_commands: Vec<AppCommand>,
    background_error: Option<String>,
}

struct PendingReconcile {
    revision: u64,
    result: Receiver<Result<Vec<AppEvent>, AppError>>,
}

struct PendingMove {
    id: DocumentId,
    target: DocumentId,
    result: Receiver<Result<(), FilesystemError>>,
    trace: Option<SidebarDropTrace>,
}

fn reconcile_workspace(
    filesystem: &dyn WorkspaceFilesystem,
    root: &Path,
    previous_documents: &[DocumentEntry],
    previous_directories: &[String],
) -> Result<Vec<AppEvent>, AppError> {
    let (_, ids, directories) = filesystem.open_workspace(root)?;
    let listed: HashSet<_> = ids.iter().collect();
    let previous: HashMap<_, _> = previous_documents
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect();
    let mut documents = Vec::with_capacity(ids.len());
    for id in &ids {
        let mut entry = previous
            .get(&id)
            .map(|entry| (*entry).clone())
            .unwrap_or(DocumentEntry {
                id: id.clone(),
                content: None,
                saved_content: None,
                external_change: false,
            });
        if entry.content.is_some() {
            match filesystem.read_document(root, &entry.id) {
                Ok(disk_content) if entry.saved_content.as_ref() == Some(&disk_content) => {
                    entry.external_change = false;
                }
                Ok(disk_content) if !entry.dirty() => {
                    entry.content = Some(disk_content.clone());
                    entry.saved_content = Some(disk_content);
                    entry.external_change = false;
                }
                _ => entry.external_change = true,
            }
        }
        documents.push(entry);
    }
    for entry in previous_documents {
        if !listed.contains(&entry.id) && entry.content.is_some() {
            let mut missing = entry.clone();
            missing.external_change = true;
            documents.push(missing);
        }
    }
    documents.sort_by(|left, right| left.id.cmp(&right.id));
    if documents == previous_documents && directories == previous_directories {
        Ok(Vec::new())
    } else {
        Ok(vec![AppEvent::WorkspaceRefreshed {
            documents,
            directories,
        }])
    }
}

fn move_target(id: &DocumentId, directory: &str) -> Result<DocumentId, AppError> {
    let name = id.as_str().rsplit('/').next().unwrap();
    if directory.is_empty() {
        return Ok(DocumentId::parse(name).unwrap());
    }
    let parent = DocumentId::parse(directory).map_err(|_| AppError::InvalidEntryName)?;
    Ok(DocumentId::parse(format!("{}/{name}", parent.as_str())).unwrap())
}

fn entry_is_within(path: &str, id: &DocumentId) -> bool {
    path == id.as_str()
        || path
            .strip_prefix(id.as_str())
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn remap_command(command: AppCommand, source: &DocumentId, target: &DocumentId) -> AppCommand {
    let map =
        |id: DocumentId| DocumentId::parse(renamed_path(id.as_str(), source, target)).unwrap();
    match command {
        AppCommand::DeleteEntry(id) => AppCommand::DeleteEntry(map(id)),
        AppCommand::OpenDocument(id) => AppCommand::OpenDocument(map(id)),
        AppCommand::CloseDocument(id) => AppCommand::CloseDocument(map(id)),
        AppCommand::DiscardAndCloseDocument(id) => AppCommand::DiscardAndCloseDocument(map(id)),
        AppCommand::SaveDocument(id) => AppCommand::SaveDocument(map(id)),
        AppCommand::ReloadDocument(id) => AppCommand::ReloadDocument(map(id)),
        AppCommand::OverwriteDocument(id) => AppCommand::OverwriteDocument(map(id)),
        AppCommand::ChangeDocument { id, content } => AppCommand::ChangeDocument {
            id: map(id),
            content,
        },
        AppCommand::CreateEntry { id, directory } => AppCommand::CreateEntry {
            id: map(id),
            directory,
        },
        AppCommand::RenameEntry { id, new_name } => AppCommand::RenameEntry {
            id: map(id),
            new_name,
        },
        AppCommand::MoveEntry { id, directory } => AppCommand::MoveEntry {
            id: map(id),
            directory: renamed_path(&directory, source, target),
        },
        AppCommand::MoveEntryOptimistic { id, directory } => AppCommand::MoveEntryOptimistic {
            id: map(id),
            directory: renamed_path(&directory, source, target),
        },
        other => other,
    }
}

impl Application {
    pub fn new() -> Self {
        Self::with_filesystem(Box::new(RealFilesystem))
    }

    pub fn with_filesystem(filesystem: Box<dyn WorkspaceFilesystem>) -> Self {
        let filesystem: Arc<dyn WorkspaceFilesystem> = filesystem.into();
        let (search_jobs, jobs_rx) = mpsc::channel();
        let (results_tx, search_results_rx) = mpsc::channel();
        let search_generation = Arc::new(AtomicU64::new(0));
        let worker_filesystem = Arc::clone(&filesystem);
        let worker_generation = Arc::clone(&search_generation);
        thread::spawn(move || {
            search::search_worker(jobs_rx, results_tx, worker_filesystem, worker_generation)
        });
        Self {
            diagnostics: Diagnostics::default(),
            search_context: Context::default(),
            command_context: Context::default(),
            snapshot_operation: 0,
            state: AppState::default(),
            filesystem,
            storage: Box::new(MemoryStorage::default()),
            watcher: None,
            last_reconcile: Instant::now(),
            search_jobs,
            search_results_rx,
            search_generation,
            pending_move: None,
            pending_reconcile: None,
            sidebar_drop_trace: None,
            deferred_commands: Vec::new(),
            background_error: None,
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
        let mut snapshot = self.state.snapshot();
        snapshot.diagnostic_operation = self.snapshot_operation;
        snapshot
    }

    /// Opt-in, best effort and compile-time removable. Never returns editor errors.
    pub fn enable_devtools(&mut self, frontend: &'static str) {
        self.diagnostics.enable(frontend);
    }

    pub fn start_devtools(&mut self, frontend: &'static str) {
        self.diagnostics.start(frontend);
        let context = self.diagnostics.context();
        self.emit_diagnostic_state(&context);
    }
    fn emit_diagnostic_state(&self, context: &Context) {
        if !self.diagnostics.enabled() {
            return;
        }
        for (name, value) in [
            ("state.documents", self.state.documents.len() as u64),
            ("state.directories", self.state.directories.len() as u64),
            (
                "state.search_results",
                self.state.search_results.len() as u64,
            ),
            (
                "state.search_pending",
                u64::from(self.state.search_in_progress),
            ),
            ("state.revision", self.state.revision),
            (
                "state.search_generation",
                self.search_generation.load(Ordering::Relaxed),
            ),
            ("state.watcher_active", u64::from(self.watcher.is_some())),
        ] {
            context.event(name, "main", self.state.revision, value);
        }
    }
    pub fn devtools_supported(&self) -> bool {
        cfg!(feature = "devtools")
    }
    pub fn open_devtools(&self) {
        self.diagnostics.observer().open_console();
    }

    pub fn diagnostic_observer(&self) -> DiagnosticObserver {
        self.diagnostics.observer()
    }

    pub fn devtools_enabled(&self) -> bool {
        self.diagnostics.enabled()
    }

    /// Time spent applying a snapshot, not proof that a frame was presented.
    pub fn diagnostic_snapshot_applied(&self, operation: u64, revision: u64, duration_ns: u64) {
        self.diagnostics
            .snapshot_applied(operation, revision, duration_ns);
    }

    pub fn take_background_error(&mut self) -> Option<String> {
        self.background_error.take()
    }

    pub fn pending_move_target(&self) -> Option<&DocumentId> {
        self.pending_move.as_ref().map(|pending| &pending.target)
    }

    pub fn has_pending_move(&self) -> bool {
        self.pending_move.is_some()
    }

    /// Searches on the calling thread; `SetSearchQuery` exists for tests.
    fn search_now(&self, query: &str) -> (Vec<SearchResult>, bool) {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            let results = self
                .state
                .documents
                .iter()
                .flat_map(|entry| search_results_for(entry, "", None))
                .collect();
            return (results, false);
        }
        let Some(workspace) = &self.state.workspace else {
            return (Vec::new(), false);
        };
        let mut results = Vec::new();
        let truncated = search::run_search(
            self.filesystem.as_ref(),
            &workspace.root,
            &self.state.documents,
            query,
            &|| false,
            |batch| results.extend(batch),
        );
        search::sort_by_document(&mut results);
        (results, truncated)
    }

    /// `stream` publishes matches as they are found; use it when the current
    /// results were cleared. Otherwise they stay until the search finishes.
    fn queue_current_search(&mut self, stream: bool) {
        if self.pending_move.is_some() {
            return;
        }
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(workspace) = &self.state.workspace
            && !self.state.search_query.0.trim().is_empty()
        {
            self.search_context = self.command_context.clone();
            self.search_context.generation = generation;
            self.search_context
                .event("search.queued", "main", self.state.revision, generation);
            let _ = self.search_jobs.send(SearchJob {
                diagnostic: self.search_context.clone(),
                generation,
                root: workspace.root.clone(),
                documents: self.state.documents.clone(),
                query: self.state.search_query.0.clone(),
                stream,
            });
        }
    }

    /// Compatibility path for frontends that only render the resulting snapshot.
    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        self.dispatch_with_events(command)
            .map(|outcome| outcome.snapshot)
    }

    pub fn dispatch_sidebar_move(
        &mut self,
        id: DocumentId,
        directory: String,
        optimistic: bool,
        trace: SidebarDropTrace,
    ) -> Result<AppSnapshot, AppError> {
        self.sidebar_drop_trace = Some(trace);
        let result = self.dispatch(if optimistic {
            AppCommand::MoveEntryOptimistic { id, directory }
        } else {
            AppCommand::MoveEntry { id, directory }
        });
        self.sidebar_drop_trace = None;
        result
    }

    /// Synchronous commands perform I/O before publishing events. Optimistic moves
    /// publish first; subsequent dispatches confirm or roll back their paths.
    pub fn dispatch_with_events(&mut self, command: AppCommand) -> Result<AppOutcome, AppError> {
        let events = self.execute(command)?;
        Ok(AppOutcome {
            events,
            snapshot: self.snapshot(),
        })
    }

    /// Avoids cloning a large workspace snapshot for a no-op timer poll.
    pub fn dispatch_if_changed(
        &mut self,
        command: AppCommand,
    ) -> Result<Option<AppSnapshot>, AppError> {
        let revision = self.state.revision;
        self.execute(command)?;
        Ok((self.state.revision != revision).then(|| self.snapshot()))
    }

    fn finish_pending_move(&mut self) -> Option<(DocumentId, DocumentId)> {
        let pending = self.pending_move.as_ref()?;
        let result = match pending.result.try_recv() {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Move worker stopped before confirming the operation".into())
            }
        };
        let pending = self.pending_move.take().unwrap();
        if let Some(trace) = &pending.trace {
            trace.confirmed(result.is_ok());
        }
        let rollback = if let Err(error) = result {
            self.state.apply(&AppEvent::EntryMoved {
                id: pending.target.clone(),
                target: pending.id.clone(),
            });
            self.state.revision += 1;
            self.background_error = Some(error);
            Some((pending.target, pending.id))
        } else {
            None
        };
        for command in std::mem::take(&mut self.deferred_commands) {
            let command = if let Some((source, target)) = &rollback {
                remap_command(command, source, target)
            } else {
                command
            };
            if let Err(error) = self.execute(command) {
                self.background_error = Some(error.to_string());
            }
        }
        rollback
    }

    fn execute(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let context = self
            .sidebar_drop_trace
            .as_ref()
            .and_then(SidebarDropTrace::context)
            .unwrap_or_else(|| self.diagnostics.context());
        self.command_context = context.clone();
        let name = command.diagnostic_name();
        let span = context.begin(name, "dispatch");
        let before_revision = self.state.revision;
        let result = self.execute_inner(command);
        if self.state.revision != before_revision {
            self.snapshot_operation = if result.as_ref().is_ok_and(|events| {
                events
                    .iter()
                    .any(|event| matches!(event, AppEvent::SearchResultsUpdated { .. }))
            }) {
                self.search_context.operation
            } else {
                context.operation
            };
        }
        if let Ok(events) = &result {
            let facts = if events
                .iter()
                .any(|event| matches!(event, AppEvent::SearchResultsUpdated { .. }))
            {
                &self.search_context
            } else {
                &context
            };
            for event in events {
                let (name, value) = match event {
                    AppEvent::DocumentChanged { content, .. } => {
                        ("document.buffer_bytes", content.len() as u64)
                    }
                    AppEvent::DocumentLoaded { content, .. } => {
                        ("document.loaded_bytes", content.len() as u64)
                    }
                    AppEvent::DocumentSaved { .. } => ("document.saved", 0),
                    AppEvent::DocumentOpened { .. } => ("document.opened", 0),
                    AppEvent::DocumentClosed { .. } => ("document.closed", 0),
                    AppEvent::WorkspaceOpened { documents, .. } => {
                        ("workspace.opened", documents.len() as u64)
                    }
                    AppEvent::WorkspaceRefreshed { documents, .. } => {
                        ("workspace.refreshed", documents.len() as u64)
                    }
                    AppEvent::SearchResultsUpdated { results, .. } => {
                        ("search.results_applied", results.len() as u64)
                    }
                    AppEvent::SearchQueryChanged { results, .. } => {
                        ("search.input_changed", results.len() as u64)
                    }
                    AppEvent::SearchQueryQueued(_) => ("search.input_changed", 0),
                    AppEvent::SearchStarted => ("search.started", 0),
                    AppEvent::EntryRenamed { .. } => ("filesystem.entry_renamed", 0),
                    AppEvent::EntryMoved { .. } => ("filesystem.entry_moved", 0),
                    AppEvent::EntryDeleted { .. } => ("filesystem.entry_deleted", 0),
                    AppEvent::DiagnosticIncremented => ("diagnostic.incremented", 0),
                };
                facts.event(name, "main", self.state.revision, value);
                if matches!(event, AppEvent::DocumentLoaded { .. }) {
                    facts.event("document.buffer_bytes", "main", self.state.revision, value);
                }
                if matches!(event, AppEvent::DocumentClosed { .. }) {
                    facts.event("document.buffer_bytes", "main", self.state.revision, 0);
                }
            }
            if self.state.revision != before_revision {
                self.emit_diagnostic_state(facts);
            }
        } else if let Err(error) = &result {
            let name = match error {
                AppError::ExternalChanges(_) => "error.save_conflict",
                AppError::UnsavedChanges => "error.unsaved_changes",
                AppError::Filesystem(_) => "error.filesystem",
                AppError::Storage(_) => "error.storage",
                _ => "error.validation",
            };
            context.error(name, "main", self.state.revision);
        }
        let outcome = match &result {
            Ok(events) if events.is_empty() => "noop",
            Ok(_) => "ok",
            Err(_) => "error",
        };
        context.end(
            name,
            "dispatch",
            span,
            outcome,
            self.state.revision,
            result.as_ref().map_or(0, |events| events.len() as u64),
        );
        result
    }

    fn execute_inner(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let command = if let Some((source, target)) = self.finish_pending_move() {
            remap_command(command, &source, &target)
        } else {
            command
        };
        if self.pending_move.is_some() {
            match &command {
                AppCommand::PollWorkspaceChanges | AppCommand::PollSearchResults => {
                    return Ok(Vec::new());
                }
                AppCommand::ChangeDocument { .. }
                | AppCommand::DiscardAndCloseDocument(_)
                | AppCommand::CloseDocument(_)
                | AppCommand::IncrementDiagnostic => {}
                _ => {
                    self.deferred_commands.push(command);
                    return Ok(Vec::new());
                }
            }
        }
        // Commit each save separately so an error never hides earlier successful writes.
        if matches!(command, AppCommand::SaveAllDocuments) {
            let ids: Vec<_> = self.state.open_documents.clone();
            let mut events = Vec::new();
            for id in ids {
                events.extend(self.execute_inner(AppCommand::SaveDocument(id))?);
            }
            return Ok(events);
        }
        let queued_query = matches!(command, AppCommand::QueueSearchQuery(_));
        let synchronous_query = matches!(command, AppCommand::SetSearchQuery(_));
        let explicit_refresh = matches!(
            command,
            AppCommand::RefreshWorkspace
                | AppCommand::DeleteEntry(_)
                | AppCommand::CloseDocument(_)
                | AppCommand::DiscardAndCloseDocument(_)
        );
        let next_revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        let mut events = self.resolve(command)?;
        // Only a new query or an explicit refresh searches the workspace again;
        // other changes update the existing matches (see `AppState::apply`).
        // A refresh that changed nothing searches again without a new revision.
        let research = explicit_refresh && !self.state.search_query.0.trim().is_empty();
        if research && !events.is_empty() {
            events.push(AppEvent::SearchStarted);
        }
        if !events.is_empty() {
            for event in &events {
                self.state.apply(event);
            }
            self.state.revision = next_revision;
            if events
                .iter()
                .any(|event| matches!(event, AppEvent::WorkspaceOpened { .. }))
                || synchronous_query
            {
                self.search_generation.fetch_add(1, Ordering::Relaxed);
            } else if queued_query {
                self.queue_current_search(true);
            }
        }
        if research {
            self.queue_current_search(false);
        }
        Ok(events)
    }

    /// Rename and move share the same filesystem operation and buffer protection.
    fn relocate_entry(&self, id: &DocumentId, target: &DocumentId) -> Result<bool, AppError> {
        if !self.validate_relocation(id, target)? {
            return Ok(false);
        }
        let root = &self.state.workspace.as_ref().unwrap().root;
        if let Some(trace) = &self.sidebar_drop_trace {
            trace.filesystem_started();
        }
        let result = self.filesystem.rename_entry(root, id, target);
        if let Some(trace) = &self.sidebar_drop_trace {
            trace.filesystem_finished(result.is_ok());
        }
        result?;
        Ok(true)
    }

    fn validate_relocation(&self, id: &DocumentId, target: &DocumentId) -> Result<bool, AppError> {
        let root = &self
            .state
            .workspace
            .as_ref()
            .ok_or(AppError::WorkspaceNotOpen)?
            .root;
        if !self.state.documents.iter().any(|entry| &entry.id == id)
            && !self
                .state
                .directories
                .iter()
                .any(|path| path == id.as_str())
        {
            return Err(AppError::DocumentNotFound(id.as_str().to_owned()));
        }
        if target == id {
            return Ok(false);
        }
        if target.as_str().starts_with(&format!("{}/", id.as_str())) {
            return Err(AppError::InvalidMove);
        }
        // Protect buffers retained after external deletion as well as disk entries.
        let target_prefix = format!("{}/", target.as_str());
        if self
            .state
            .documents
            .iter()
            .any(|entry| &entry.id == target || entry.id.as_str().starts_with(&target_prefix))
            || self
                .state
                .directories
                .iter()
                .any(|path| path == target.as_str() || path.starts_with(&target_prefix))
        {
            return Err(AppError::Filesystem(FilesystemError::Io {
                path: root.join(target.as_str()),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination already exists",
                ),
            }));
        }
        Ok(true)
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
                    let (results, truncated) = self.search_now(&query);
                    vec![AppEvent::SearchQueryChanged {
                        query: SearchQuery(query),
                        results,
                        truncated,
                    }]
                }
            }
            AppCommand::QueueSearchQuery(query) => {
                if self.state.search_query.0 == query {
                    Vec::new()
                } else {
                    let searching = !query.trim().is_empty();
                    let mut events = vec![AppEvent::SearchQueryQueued(SearchQuery(query))];
                    if searching {
                        events.push(AppEvent::SearchStarted);
                    }
                    events
                }
            }
            AppCommand::PollSearchResults => {
                // Streamed batches are merged into one event per poll.
                let current = self.search_generation.load(Ordering::Relaxed);
                let mut merged: Option<(Vec<SearchResult>, bool, bool, bool)> = None;
                while let Ok(batch) = self.search_results_rx.try_recv() {
                    if batch.generation != current {
                        let span = batch.diagnostic.begin("search.delivery", "main");
                        batch.diagnostic.end(
                            "search.delivery",
                            "main",
                            span,
                            "discarded",
                            self.state.revision,
                            0,
                        );
                        batch.diagnostic.event(
                            "search.stale_generation",
                            "main",
                            self.state.revision,
                            batch.generation,
                        );
                        continue;
                    }
                    batch.diagnostic.event(
                        "search.delivered",
                        "main",
                        self.state.revision,
                        batch.results.len() as u64,
                    );
                    self.search_context = batch.diagnostic;
                    let (results, replace, finished, truncated) =
                        merged.get_or_insert_with(|| (Vec::new(), false, false, false));
                    if batch.replace {
                        *results = batch.results;
                        *replace = true;
                    } else {
                        results.extend(batch.results);
                    }
                    *finished |= batch.finished;
                    *truncated |= batch.truncated;
                }
                merged
                    .map(|(results, replace, finished, truncated)| {
                        vec![AppEvent::SearchResultsUpdated {
                            results,
                            replace,
                            finished,
                            truncated,
                        }]
                    })
                    .unwrap_or_default()
            }
            AppCommand::OpenWorkspace(path) => {
                self.pending_reconcile = None;
                if self.state.documents.iter().any(DocumentEntry::dirty) {
                    return Err(AppError::UnsavedChanges);
                }
                let (workspace, documents, directories) = self.filesystem.open_workspace(&path)?;
                let recent_workspaces = self.storage.record_workspace(&workspace)?;
                self.watcher = WorkspaceWatcher::new(&workspace.root).ok();
                self.last_reconcile = Instant::now();
                vec![AppEvent::WorkspaceOpened {
                    workspace,
                    documents,
                    directories,
                    recent_workspaces,
                }]
            }
            AppCommand::PollWorkspaceChanges => {
                if self.state.workspace.is_none() {
                    return Ok(Vec::new());
                }
                if let Some(pending) = &self.pending_reconcile {
                    match pending.result.try_recv() {
                        Ok(result) => {
                            let pending = self.pending_reconcile.take().unwrap();
                            // A scan cannot replace edits or paths changed since it started.
                            if pending.revision != self.state.revision {
                                self.last_reconcile = Instant::now() - Duration::from_secs(30);
                                return Ok(Vec::new());
                            }
                            return result;
                        }
                        Err(mpsc::TryRecvError::Empty) => return Ok(Vec::new()),
                        Err(mpsc::TryRecvError::Disconnected) => {
                            self.pending_reconcile = None;
                            return Err(AppError::Filesystem(FilesystemError::Io {
                                path: self.state.workspace.as_ref().unwrap().root.clone(),
                                source: std::io::Error::other("Workspace scan worker stopped"),
                            }));
                        }
                    }
                }
                let interval = if self.watcher.is_some() {
                    Duration::from_secs(30)
                } else {
                    Duration::from_secs(2)
                };
                let (changed, count, errors) = self
                    .watcher
                    .as_ref()
                    .map(WorkspaceWatcher::drain_changes)
                    .unwrap_or_default();
                if count > 0 {
                    self.command_context
                        .event("watcher.batch", "main", self.state.revision, count);
                }
                if errors > 0 {
                    self.command_context.event(
                        "watcher.errors",
                        "main",
                        self.state.revision,
                        errors,
                    );
                }
                if changed || self.last_reconcile.elapsed() >= interval {
                    self.last_reconcile = Instant::now();
                    let root = self.state.workspace.as_ref().unwrap().root.clone();
                    let documents = self.state.documents.clone();
                    let directories = self.state.directories.clone();
                    let filesystem = Arc::clone(&self.filesystem);
                    let (sender, result) = mpsc::channel();
                    self.pending_reconcile = Some(PendingReconcile {
                        revision: self.state.revision,
                        result,
                    });
                    let context = self.command_context.clone();
                    let revision = self.state.revision;
                    thread::spawn(move || {
                        let span = context.begin("reconcile.background", "filesystem-worker");
                        let outcome = reconcile_workspace(
                            filesystem.as_ref(),
                            &root,
                            &documents,
                            &directories,
                        );
                        context.end(
                            "reconcile.background",
                            "filesystem-worker",
                            span,
                            if outcome.is_ok() { "ok" } else { "error" },
                            revision,
                            0,
                        );
                        let _ = sender.send(outcome);
                    });
                }
                Vec::new()
            }
            AppCommand::RefreshWorkspace => {
                self.pending_reconcile = None;
                let root = &self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                reconcile_workspace(
                    self.filesystem.as_ref(),
                    root,
                    &self.state.documents,
                    &self.state.directories,
                )?
            }

            AppCommand::RenameEntry { id, new_name } => {
                let name =
                    DocumentId::parse(new_name.clone()).map_err(|_| AppError::InvalidEntryName)?;
                if name.as_str().contains('/') {
                    return Err(AppError::InvalidEntryName);
                }
                let target = DocumentId::parse(match id.as_str().rsplit_once('/') {
                    Some((parent, _)) => format!("{parent}/{new_name}"),
                    None => new_name,
                })
                .unwrap();
                if self.relocate_entry(&id, &target)? {
                    vec![AppEvent::EntryRenamed { id, target }]
                } else {
                    Vec::new()
                }
            }
            AppCommand::MoveEntryOptimistic { id, directory } => {
                let target = move_target(&id, &directory)?;
                if !self.validate_relocation(&id, &target)? {
                    return Ok(Vec::new());
                }
                let root = self.state.workspace.as_ref().unwrap().root.clone();
                let filesystem = Arc::clone(&self.filesystem);
                let source = id.clone();
                let destination = target.clone();
                let (sender, result) = mpsc::channel();
                self.search_generation.fetch_add(1, Ordering::Relaxed);
                let trace = self.sidebar_drop_trace.clone();
                self.pending_move = Some(PendingMove {
                    id: id.clone(),
                    target: target.clone(),
                    result,
                    trace: trace.clone(),
                });
                thread::spawn(move || {
                    if let Some(trace) = &trace {
                        trace.filesystem_started();
                    }
                    let result = filesystem.rename_entry(&root, &source, &destination);
                    if let Some(trace) = &trace {
                        trace.filesystem_finished(result.is_ok());
                    }
                    let _ = sender.send(result);
                });
                vec![AppEvent::EntryMoved { id, target }]
            }
            AppCommand::MoveEntry { id, directory } => {
                let name = id.as_str().rsplit('/').next().unwrap();
                let target = if directory.is_empty() {
                    DocumentId::parse(name).unwrap()
                } else {
                    let parent =
                        DocumentId::parse(directory).map_err(|_| AppError::InvalidEntryName)?;
                    DocumentId::parse(format!("{}/{name}", parent.as_str())).unwrap()
                };
                if self.relocate_entry(&id, &target)? {
                    vec![AppEvent::EntryMoved { id, target }]
                } else {
                    Vec::new()
                }
            }
            AppCommand::DeleteEntry(id) => {
                let workspace = self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?;
                if !self.state.documents.iter().any(|entry| entry.id == id)
                    && !self
                        .state
                        .directories
                        .iter()
                        .any(|path| path == id.as_str())
                {
                    return Err(AppError::DocumentNotFound(id.as_str().to_owned()));
                }
                if self
                    .state
                    .documents
                    .iter()
                    .any(|entry| entry_is_within(entry.id.as_str(), &id) && entry.dirty())
                {
                    return Err(AppError::DeleteUnsavedChanges);
                }
                self.filesystem.delete_entry(&workspace.root, &id)?;
                self.pending_reconcile = None;
                vec![AppEvent::EntryDeleted { id }]
            }
            AppCommand::CreateEntry { id, directory } => {
                let workspace = self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?;
                self.filesystem
                    .create_entry(&workspace.root, &id, directory)?;
                self.resolve(AppCommand::RefreshWorkspace)?
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
            AppCommand::DiscardAndCloseDocument(id) => {
                if self.state.open_documents.contains(&id) {
                    vec![AppEvent::DocumentClosed { id }]
                } else {
                    Vec::new()
                }
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
                if self.state.open_documents.contains(&id) {
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
            AppCommand::SaveAllDocuments => unreachable!("handled before resolution"),
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
                    let disk_content = self.filesystem.read_document(root, &id).ok();
                    if disk_content != entry.saved_content {
                        return Err(AppError::ExternalChanges(id.as_str().to_owned()));
                    }
                    self.filesystem.write_document(root, &id, content)?;
                    vec![AppEvent::DocumentSaved { id }]
                } else {
                    Vec::new()
                }
            }
            AppCommand::ReloadDocument(id) => {
                let root = &self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?
                    .root;
                self.state
                    .documents
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| AppError::DocumentNotFound(id.as_str().to_owned()))?;
                let content = self.filesystem.read_document(root, &id)?;
                vec![AppEvent::DocumentLoaded { id, content }]
            }
            AppCommand::OverwriteDocument(id) => {
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
                self.filesystem.write_document(root, &id, content)?;
                vec![AppEvent::DocumentSaved { id }]
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
    fn no_op_poll_does_not_clone_a_snapshot() {
        let mut app = Application::new();
        assert!(
            app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap()
                .is_none()
        );
        assert!(
            app.dispatch_if_changed(AppCommand::PollSearchResults)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn creates_files_and_folders_without_overwriting_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let snapshot = app
            .dispatch(AppCommand::CreateEntry {
                id: id("notes"),
                directory: true,
            })
            .unwrap();
        assert_eq!(snapshot.directories[..], ["notes"]);
        let snapshot = app
            .dispatch(AppCommand::CreateEntry {
                id: id("notes/todo.md"),
                directory: false,
            })
            .unwrap();
        assert!(
            snapshot
                .documents
                .iter()
                .any(|entry| entry.id == id("notes/todo.md"))
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/todo.md")).unwrap(),
            ""
        );
        fs::write(dir.path().join("notes/todo.md"), "keep me").unwrap();
        assert!(
            app.dispatch(AppCommand::CreateEntry {
                id: id("notes/todo.md"),
                directory: false,
            })
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/todo.md")).unwrap(),
            "keep me"
        );
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
        assert!(changed.search_results[0].dirty);
        let saved = app
            .dispatch(AppCommand::SaveDocument(id("note.md")))
            .unwrap();
        assert!(!saved.documents[0].dirty);
        assert!(!saved.search_results[0].dirty);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "edited"
        );
    }

    #[test]
    fn refresh_updates_clean_documents_and_workspace_search() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "old text").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        app.dispatch(AppCommand::SetSearchQuery("new".into()))
            .unwrap();
        fs::write(dir.path().join("note.md"), "new text").unwrap();
        fs::write(dir.path().join("added.txt"), "new file").unwrap();
        let refreshed = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        assert_eq!(refreshed.active_document.unwrap().content, "new text");
        assert_eq!(refreshed.documents.len(), 2);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
            if snapshot.search_results.len() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "background search did not finish"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        let revision = app.snapshot().revision;
        assert_eq!(
            app.dispatch(AppCommand::RefreshWorkspace).unwrap().revision,
            revision
        );
    }

    #[test]
    fn watcher_refreshes_workspace_after_external_change() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        let path = dir.path().join("nested/note.md");
        fs::write(&path, "before").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        assert!(app.watcher.is_some(), "native watcher should start");
        app.dispatch(AppCommand::OpenDocument(id("nested/note.md")))
            .unwrap();
        let initial_revision = app.snapshot().revision;
        assert_eq!(
            app.dispatch(AppCommand::PollWorkspaceChanges)
                .unwrap()
                .revision,
            initial_revision
        );
        fs::write(&path, "after").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
            if snapshot.revision != initial_revision {
                assert_eq!(snapshot.active_document.unwrap().content, "after");
                break;
            }
            assert!(Instant::now() < deadline, "watcher did not report the edit");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn queued_search_discards_stale_queries_in_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();
        fs::write(dir.path().join("nested/note.rs"), "alpha beta beta").unwrap();
        let mut app = Application::new();
        let opened = app
            .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        assert_eq!(opened.directories[..], ["empty", "nested"]);
        assert_eq!(opened.documents[0].id, id("nested/note.rs"));
        app.dispatch(AppCommand::QueueSearchQuery("alpha".into()))
            .unwrap();
        let queued = app
            .dispatch(AppCommand::QueueSearchQuery("beta".into()))
            .unwrap();
        assert!(queued.search_in_progress);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
            if !snapshot.search_in_progress {
                assert_eq!(snapshot.search_query.0, "beta");
                assert_eq!(snapshot.search_results.len(), 2);
                assert!(
                    snapshot
                        .search_results
                        .iter()
                        .all(|result| result.id == id("nested/note.rs"))
                );
                break;
            }
            assert!(Instant::now() < deadline, "queued search did not finish");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn typing_a_query_leaves_searching_and_unchanged_lists_to_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("alpha.md"), "beta").unwrap();
        fs::write(dir.path().join("other.md"), "alpha beta").unwrap();
        let mut app = Application::new();
        let opened = app
            .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let queued = app
            .dispatch(AppCommand::QueueSearchQuery("alpha".into()))
            .unwrap();
        assert!(queued.search_in_progress);
        assert!(
            queued.search_results.is_empty(),
            "no scan before the worker"
        );
        assert!(Arc::ptr_eq(&opened.documents, &queued.documents));
        assert!(Arc::ptr_eq(&opened.directories, &queued.directories));
        let deadline = Instant::now() + Duration::from_secs(5);
        let found = loop {
            let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
            if !snapshot.search_in_progress {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "queued search did not finish");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(found.search_results.len(), 1, "content matches only");
        assert_eq!(found.search_results[0].id, id("other.md"));
        let refined = app
            .dispatch(AppCommand::QueueSearchQuery("alpha b".into()))
            .unwrap();
        assert!(refined.search_in_progress);
        assert!(
            refined.search_results.is_empty(),
            "stale results are cleared"
        );
        let cleared = app
            .dispatch(AppCommand::QueueSearchQuery(String::new()))
            .unwrap();
        assert!(!cleared.search_in_progress);
        assert_eq!(cleared.search_results.len(), 2, "every document is listed");
    }

    #[test]
    fn external_edits_block_save_until_reload_or_explicit_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "base").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("note.md")))
            .unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "local".into(),
        })
        .unwrap();
        fs::write(&path, "external").unwrap();
        assert!(matches!(
            app.dispatch(AppCommand::SaveDocument(id("note.md"))),
            Err(AppError::ExternalChanges(_))
        ));
        let conflicted = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        let active = conflicted.active_document.unwrap();
        assert_eq!(active.content, "local");
        assert!(active.dirty && active.external_change);
        app.dispatch(AppCommand::ReloadDocument(id("note.md")))
            .unwrap();
        assert_eq!(app.snapshot().active_document.unwrap().content, "external");
        app.dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "local again".into(),
        })
        .unwrap();
        fs::remove_file(&path).unwrap();
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        assert!(app.snapshot().active_document.unwrap().external_change);
        app.dispatch(AppCommand::OverwriteDocument(id("note.md")))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "local again");
        assert!(!app.snapshot().active_document.unwrap().external_change);
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
    fn text_search_lists_content_matches_only_and_keeps_open_document() {
        let dir = tempfile::tempdir().unwrap();
        // A file name match alone is not a text match.
        fs::write(dir.path().join("Alpha.md"), "one alpha").unwrap();
        fs::write(dir.path().join("alpine.md"), "two").unwrap();
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
            [AppEvent::SearchQueryChanged { query, results, .. }]
                if query.0 == "  ALP  " && results.len() == 1
        ));
        assert_eq!(outcome.snapshot.search_results.len(), 1);
        assert_eq!(outcome.snapshot.search_results[0].id, id("Alpha.md"));
        assert!(outcome.snapshot.search_results[0].location.is_some());
        assert_eq!(outcome.snapshot.active_document.unwrap().id, id("beta.txt"));
        let revision = outcome.snapshot.revision;
        let unchanged = app
            .dispatch(AppCommand::SetSearchQuery("  ALP  ".into()))
            .unwrap();
        assert_eq!(unchanged.revision, revision);
        let cleared = app
            .dispatch(AppCommand::SetSearchQuery(String::new()))
            .unwrap();
        assert_eq!(cleared.search_results.len(), 3);
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
    fn search_returns_each_occurrence_with_editor_offsets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "A 💡 café café\ncafé").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("CAFÉ".into()))
            .unwrap();
        assert_eq!(found.search_results.len(), 3);
        let locations: Vec<_> = found
            .search_results
            .iter()
            .map(|result| result.location.clone().unwrap())
            .collect();
        assert_eq!(
            (
                locations[0].line,
                locations[0].column,
                locations[0].start_char,
                locations[0].start_utf16
            ),
            (1, 5, 4, 5)
        );
        assert_eq!(
            (
                locations[1].line,
                locations[1].column,
                locations[1].start_char,
                locations[1].start_utf16
            ),
            (1, 10, 9, 10)
        );
        assert_eq!(
            (
                locations[2].line,
                locations[2].column,
                locations[2].start_char,
                locations[2].start_utf16
            ),
            (2, 1, 14, 15)
        );
        assert_eq!(locations[2].end_utf16, 19);
    }

    #[test]
    fn content_occurrences_remain_individually_selectable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "note note").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("note".into()))
            .unwrap();
        assert_eq!(found.search_results.len(), 2);
        assert_eq!(
            found.search_results[0]
                .location
                .as_ref()
                .unwrap()
                .start_char,
            0
        );
        assert_eq!(
            found.search_results[1]
                .location
                .as_ref()
                .unwrap()
                .start_char,
            5
        );
    }

    #[test]
    fn overlapping_text_occurrences_are_both_listed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("text.md"), "banana").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("ana".into()))
            .unwrap();
        assert_eq!(found.search_results.len(), 2);
        assert_eq!(
            found.search_results[0]
                .location
                .as_ref()
                .unwrap()
                .start_char,
            1
        );
        assert_eq!(
            found.search_results[1]
                .location
                .as_ref()
                .unwrap()
                .start_char,
            3
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

    #[test]
    fn move_folder_preserves_open_buffers_and_can_move_back_to_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        fs::create_dir(dir.path().join("destination")).unwrap();
        fs::write(dir.path().join("src/nested/a.rs"), "original").unwrap();
        fs::write(dir.path().join("src/b.rs"), "second").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        for path in ["src/b.rs", "src/nested/a.rs"] {
            app.dispatch(AppCommand::OpenDocument(id(path))).unwrap();
            app.dispatch(AppCommand::ChangeDocument {
                id: id(path),
                content: "local needle".into(),
            })
            .unwrap();
        }
        app.dispatch(AppCommand::SetSearchQuery("needle".into()))
            .unwrap();
        let moved = app
            .dispatch_with_events(AppCommand::MoveEntry {
                id: id("src"),
                directory: "destination".into(),
            })
            .unwrap();
        assert!(
            matches!(&moved.events[0], AppEvent::EntryMoved { id: source, target } if source.as_str() == "src" && target.as_str() == "destination/src")
        );
        let active = moved.snapshot.active_document.unwrap();
        assert_eq!(active.id, id("destination/src/nested/a.rs"));
        assert_eq!(active.content, "local needle");
        assert!(active.dirty);
        assert_eq!(
            moved.snapshot.directories[..],
            ["destination", "destination/src", "destination/src/nested"]
        );
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        // Also exercise a file move, preserving its base name, into the root.
        app.dispatch(AppCommand::MoveEntry {
            id: id("destination/src/b.rs"),
            directory: String::new(),
        })
        .unwrap();
        app.dispatch(AppCommand::SaveDocument(id("b.rs"))).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("b.rs")).unwrap(),
            "local needle"
        );
        app.dispatch(AppCommand::MoveEntry {
            id: id("destination/src"),
            directory: String::new(),
        })
        .unwrap();
        app.dispatch(AppCommand::SaveDocument(id("src/nested/a.rs")))
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/nested/a.rs")).unwrap(),
            "local needle"
        );
        assert!(!dir.path().join("destination/src").exists());
        assert!(
            app.snapshot()
                .search_results
                .iter()
                .all(|result| !result.id.as_str().starts_with("destination/"))
        );
    }

    #[test]
    fn move_rejects_descendants_collisions_and_invalid_destinations_atomically() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        fs::create_dir_all(dir.path().join("occupied/src")).unwrap();
        fs::write(dir.path().join("src/a.md"), "original").unwrap();
        fs::write(dir.path().join("file"), "not a directory").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("src/a.md")))
            .unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("src/a.md"),
            content: "local".into(),
        })
        .unwrap();
        let before = app.snapshot();
        for directory in [
            "src",
            "src/nested",
            "occupied",
            "missing",
            "file",
            "..",
            "../outside",
            "/tmp",
            "a//b",
        ] {
            assert!(
                app.dispatch(AppCommand::MoveEntry {
                    id: id("src"),
                    directory: directory.into()
                })
                .is_err(),
                "{directory}"
            );
            assert_eq!(app.snapshot(), before);
        }
        let unchanged = app
            .dispatch_with_events(AppCommand::MoveEntry {
                id: id("src"),
                directory: String::new(),
            })
            .unwrap();
        assert!(unchanged.events.is_empty());
        assert_eq!(unchanged.snapshot, before);
        assert_eq!(
            fs::read_to_string(dir.path().join("src/a.md")).unwrap(),
            "original"
        );
        assert!(
            app.dispatch(AppCommand::MoveEntry {
                id: id("nonexistent"),
                directory: String::new()
            })
            .is_err()
        );
        assert_eq!(app.snapshot(), before);
    }

    #[test]
    fn move_protects_conflicts_and_missing_dirty_destination_buffers() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dest")).unwrap();
        fs::write(dir.path().join("a.md"), "original").unwrap();
        fs::write(dir.path().join("dest/a.md"), "destination").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("dest/a.md")))
            .unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("dest/a.md"),
            content: "local destination".into(),
        })
        .unwrap();
        fs::remove_file(dir.path().join("dest/a.md")).unwrap();
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        let before = app.snapshot();
        assert!(
            app.dispatch(AppCommand::MoveEntry {
                id: id("a.md"),
                directory: "dest".into()
            })
            .is_err()
        );
        assert_eq!(app.snapshot(), before);
        app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("a.md"),
            content: "local source".into(),
        })
        .unwrap();
        fs::write(dir.path().join("a.md"), "external source").unwrap();
        fs::create_dir(dir.path().join("other")).unwrap();
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        let moved = app
            .dispatch(AppCommand::MoveEntry {
                id: id("a.md"),
                directory: "other".into(),
            })
            .unwrap();
        let active = moved.active_document.unwrap();
        assert_eq!(active.id, id("other/a.md"));
        assert_eq!(active.content, "local source");
        assert!(active.external_change);
        assert!(matches!(
            app.dispatch(AppCommand::SaveDocument(id("other/a.md"))),
            Err(AppError::ExternalChanges(_))
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("other/a.md")).unwrap(),
            "external source"
        );
    }

    #[test]
    fn renaming_folder_preserves_all_open_buffers_and_search_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        fs::create_dir(dir.path().join("src-other")).unwrap();
        fs::write(dir.path().join("src/nested/a.rs"), "original").unwrap();
        fs::write(dir.path().join("src/b.rs"), "second").unwrap();
        fs::write(dir.path().join("src-other/c.rs"), "untouched").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        for path in ["src/b.rs", "src/nested/a.rs"] {
            app.dispatch(AppCommand::OpenDocument(id(path))).unwrap();
            app.dispatch(AppCommand::ChangeDocument {
                id: id(path),
                content: "local needle".into(),
            })
            .unwrap();
        }
        app.dispatch(AppCommand::SetSearchQuery("needle".into()))
            .unwrap();
        let snapshot = app
            .dispatch(AppCommand::RenameEntry {
                id: id("src"),
                new_name: "lib".into(),
            })
            .unwrap();
        let active = snapshot.active_document.unwrap();
        assert_eq!(active.id, id("lib/nested/a.rs"));
        assert_eq!(active.content, "local needle");
        assert!(active.dirty);
        assert_eq!(snapshot.directories[..], ["lib", "lib/nested", "src-other"]);
        assert!(
            snapshot
                .search_results
                .iter()
                .all(|result| result.id.as_str().starts_with("lib/"))
        );
        assert!(!dir.path().join("src").exists());
        // Watcher reconciliation must not lose either unsaved buffer.
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        for path in ["lib/b.rs", "lib/nested/a.rs"] {
            assert_eq!(
                app.dispatch(AppCommand::OpenDocument(id(path)))
                    .unwrap()
                    .active_document
                    .unwrap()
                    .content,
                "local needle"
            );
            app.dispatch(AppCommand::SaveDocument(id(path))).unwrap();
            assert_eq!(
                fs::read_to_string(dir.path().join(path)).unwrap(),
                "local needle"
            );
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("src-other/c.rs")).unwrap(),
            "untouched"
        );
    }

    #[test]
    fn rename_rejects_collisions_and_invalid_names_without_changing_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        fs::create_dir(dir.path().join("folder")).unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
        let before = app.snapshot();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "nested/new",
            "a\\b",
            "b.md",
            "folder",
        ] {
            assert!(
                app.dispatch(AppCommand::RenameEntry {
                    id: id("a.md"),
                    new_name: name.into()
                })
                .is_err(),
                "{name}"
            );
            assert_eq!(app.snapshot(), before);
        }
        let unchanged = app
            .dispatch_with_events(AppCommand::RenameEntry {
                id: id("a.md"),
                new_name: "a.md".into(),
            })
            .unwrap();
        assert!(unchanged.events.is_empty());
        assert_eq!(unchanged.snapshot, before);
        assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "a");
        assert_eq!(fs::read_to_string(dir.path().join("b.md")).unwrap(), "b");
        // Collision created after the last snapshot is still rejected by the filesystem.
        fs::write(dir.path().join("new.md"), "external").unwrap();
        assert!(
            app.dispatch(AppCommand::RenameEntry {
                id: id("a.md"),
                new_name: "new.md".into()
            })
            .is_err()
        );
        assert_eq!(app.snapshot(), before);
        fs::remove_file(dir.path().join("a.md")).unwrap();
        assert!(
            app.dispatch(AppCommand::RenameEntry {
                id: id("a.md"),
                new_name: "renamed.md".into()
            })
            .is_err()
        );
        assert_eq!(app.snapshot(), before);
    }

    #[test]
    fn rename_keeps_external_conflict_and_protects_missing_dirty_destination() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "original").unwrap();
        fs::write(dir.path().join("b.md"), "second").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("a.md"),
            content: "local".into(),
        })
        .unwrap();
        fs::write(dir.path().join("a.md"), "external").unwrap();
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        let renamed = app
            .dispatch(AppCommand::RenameEntry {
                id: id("a.md"),
                new_name: "renamed.md".into(),
            })
            .unwrap();
        assert!(renamed.active_document.unwrap().external_change);
        assert!(matches!(
            app.dispatch(AppCommand::SaveDocument(id("renamed.md"))),
            Err(AppError::ExternalChanges(_))
        ));
        fs::remove_file(dir.path().join("renamed.md")).unwrap();
        app.dispatch(AppCommand::RefreshWorkspace).unwrap();
        let before = app.snapshot();
        assert!(
            app.dispatch(AppCommand::RenameEntry {
                id: id("b.md"),
                new_name: "renamed.md".into()
            })
            .is_err()
        );
        assert_eq!(app.snapshot(), before);
    }

    struct FailingWriteFilesystem;

    impl WorkspaceFilesystem for FailingWriteFilesystem {
        fn open_workspace(
            &self,
            path: &std::path::Path,
        ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
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

#[cfg(test)]
mod document_tabs_tests;
