//! Shared application commands, events, state, and owned snapshots.

mod icons;
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
        Arc,
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
    CreateEntry { id: DocumentId, directory: bool },
    RefreshWorkspace,
    PollWorkspaceChanges,
    OpenDocument(DocumentId),
    CloseDocument(DocumentId),
    ChangeDocument { id: DocumentId, content: String },
    SaveDocument(DocumentId),
    ReloadDocument(DocumentId),
    OverwriteDocument(DocumentId),
}

/// A fact produced by a successful command. Events are transient, not a persisted log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppEvent {
    DiagnosticIncremented,
    SearchQueryChanged {
        query: SearchQuery,
        results: Vec<SearchResult>,
    },
    SearchResultsUpdated(Vec<SearchResult>),
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
        search_results: Vec<SearchResult>,
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

fn preview(line: &str, match_start: usize) -> String {
    let leading = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let line = line.trim();
    let match_start = match_start.saturating_sub(leading);
    let start = match_start.saturating_sub(40);
    let mut chars = line.chars().skip(start);
    let text: String = chars.by_ref().take(160).collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        text,
        if chars.next().is_some() { "…" } else { "" }
    )
}

fn search_results_for(
    entry: &DocumentEntry,
    needle: &str,
    content: Option<&str>,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    if needle.is_empty() || entry.id.as_str().to_lowercase().contains(needle) {
        results.push(SearchResult {
            id: entry.id.clone(),
            title: entry.id.as_str().to_owned(),
            dirty: entry.dirty(),
            snippet: None,
            location: None,
        });
    }
    if needle.is_empty() {
        return results;
    }
    let Some(content) = content else {
        return results;
    };
    let mut line_start_char = 0usize;
    let mut line_start_utf16 = 0usize;
    for (line_index, raw_line) in content.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let mut folded = String::new();
        let mut positions = Vec::new();
        let mut char_offset = line_start_char;
        let mut utf16_offset = line_start_utf16;
        for ch in line.chars() {
            let next_char = char_offset + 1;
            let next_utf16 = utf16_offset + ch.len_utf16();
            for lower in ch.to_lowercase() {
                positions.push((
                    folded.len(),
                    char_offset,
                    next_char,
                    utf16_offset,
                    next_utf16,
                ));
                folded.push(lower);
            }
            char_offset = next_char;
            utf16_offset = next_utf16;
        }
        let mut search_from = 0;
        while search_from < folded.len() {
            let Some(relative) = folded[search_from..].find(needle) else {
                break;
            };
            let start_byte = search_from + relative;
            let start_index = positions
                .binary_search_by_key(&start_byte, |position| position.0)
                .expect("match starts at a character boundary");
            let end_byte = start_byte + needle.len();
            let end_index = positions
                .binary_search_by_key(&end_byte, |position| position.0)
                .unwrap_or_else(|index| index);
            let (_, start_char, _, start_utf16, _) = positions[start_index];
            let (_, _, end_char, _, end_utf16) = positions[end_index - 1];
            let column = start_char - line_start_char + 1;
            results.push(SearchResult {
                id: entry.id.clone(),
                title: entry.id.as_str().to_owned(),
                dirty: entry.dirty(),
                snippet: Some(preview(line, column - 1)),
                location: Some(SearchLocation {
                    line: (line_index + 1) as u64,
                    column: column as u64,
                    start_char: start_char as u64,
                    end_char: end_char as u64,
                    start_utf16: start_utf16 as u64,
                    end_utf16: end_utf16 as u64,
                }),
            });
            search_from = start_byte + folded[start_byte..].chars().next().unwrap().len_utf8();
        }
        line_start_char += raw_line.chars().count();
        line_start_utf16 += raw_line.encode_utf16().count();
    }
    results
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
    pub search_in_progress: bool,
    pub directories: Vec<String>,
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
    directories: Vec<String>,
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
                        external_change: entry.external_change,
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
            search_in_progress: self.search_in_progress,
            directories: self.directories.clone(),
            documents: self
                .documents
                .iter()
                .map(|entry| DocumentSummary {
                    id: entry.id.clone(),
                    title: entry.id.as_str().to_owned(),
                    dirty: entry.dirty(),
                    external_change: entry.external_change,
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
                self.search_in_progress = false;
            }
            AppEvent::SearchResultsUpdated(results) => {
                self.search_results = results.clone();
                self.search_in_progress = false;
            }
            AppEvent::SearchStarted => self.search_in_progress = true,
            AppEvent::WorkspaceOpened {
                workspace,
                documents,
                directories,
                recent_workspaces,
            } => {
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
                self.search_results = self
                    .documents
                    .iter()
                    .flat_map(|entry| search_results_for(entry, "", None))
                    .collect();
                self.active_document = None;
            }
            AppEvent::WorkspaceRefreshed {
                documents,
                directories,
                search_results,
            } => {
                self.documents = documents.clone();
                self.directories = directories.clone();
                self.search_results = search_results.clone();
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
                entry.external_change = false;
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
    state: AppState,
    filesystem: Arc<dyn WorkspaceFilesystem>,
    storage: Box<dyn MetadataStorage>,
    watcher: Option<WorkspaceWatcher>,
    last_reconcile: Instant,
    search_jobs: Sender<SearchJob>,
    search_results_rx: Receiver<SearchFinished>,
    search_generation: Arc<AtomicU64>,
}

struct SearchJob {
    generation: u64,
    root: PathBuf,
    documents: Vec<DocumentEntry>,
    query: String,
}

struct SearchFinished {
    generation: u64,
    results: Vec<SearchResult>,
}

fn collect_search_results(
    filesystem: &dyn WorkspaceFilesystem,
    root: Option<&Path>,
    documents: &[DocumentEntry],
    query: &str,
    cancelled: impl Fn() -> bool,
) -> Vec<SearchResult> {
    let needle = query.trim().to_lowercase();
    documents
        .iter()
        .take_while(|_| !cancelled())
        .flat_map(|entry| {
            let disk_content = if needle.is_empty() || entry.content.is_some() {
                None
            } else {
                root.and_then(|root| filesystem.read_document_if_matching(root, &entry.id, &needle))
            };
            search_results_for(
                entry,
                &needle,
                entry.content.as_deref().or(disk_content.as_deref()),
            )
        })
        .collect()
}

fn search_worker(
    jobs: Receiver<SearchJob>,
    results: Sender<SearchFinished>,
    filesystem: Arc<dyn WorkspaceFilesystem>,
    generation: Arc<AtomicU64>,
) {
    while let Ok(mut job) = jobs.recv() {
        loop {
            match jobs.recv_timeout(Duration::from_millis(150)) {
                Ok(newer) => job = newer,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        if generation.load(Ordering::Relaxed) != job.generation {
            continue;
        }
        let found = collect_search_results(
            filesystem.as_ref(),
            Some(&job.root),
            &job.documents,
            &job.query,
            || generation.load(Ordering::Relaxed) != job.generation,
        );
        if generation.load(Ordering::Relaxed) == job.generation
            && results
                .send(SearchFinished {
                    generation: job.generation,
                    results: found,
                })
                .is_err()
        {
            return;
        }
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
            search_worker(jobs_rx, results_tx, worker_filesystem, worker_generation)
        });
        Self {
            state: AppState::default(),
            filesystem,
            storage: Box::new(MemoryStorage::default()),
            watcher: None,
            last_reconcile: Instant::now(),
            search_jobs,
            search_results_rx,
            search_generation,
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

    fn search_results(&self, documents: &[DocumentEntry], query: &str) -> Vec<SearchResult> {
        collect_search_results(
            self.filesystem.as_ref(),
            self.state
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.as_path()),
            documents,
            query,
            || false,
        )
    }

    fn queue_current_search(&mut self) {
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(workspace) = &self.state.workspace
            && !self.state.search_query.0.trim().is_empty()
        {
            let _ = self.search_jobs.send(SearchJob {
                generation,
                root: workspace.root.clone(),
                documents: self.state.documents.clone(),
                query: self.state.search_query.0.clone(),
            });
        }
    }

    /// Compatibility path for frontends that only render the resulting snapshot.
    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        self.dispatch_with_events(command)
            .map(|outcome| outcome.snapshot)
    }

    /// Performs external I/O first, then applies the resulting events to state.
    /// Failed and no-op commands produce no events and do not advance revision.
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
        let events = self.execute(command)?;
        Ok((!events.is_empty()).then(|| self.snapshot()))
    }

    fn execute(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        let queued_query = matches!(command, AppCommand::QueueSearchQuery(_));
        let synchronous_query = matches!(command, AppCommand::SetSearchQuery(_));
        let explicit_refresh = matches!(command, AppCommand::RefreshWorkspace);
        let next_revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        let mut events = self.resolve(command)?;
        if !queued_query
            && !synchronous_query
            && !self.state.search_query.0.trim().is_empty()
            && events.iter().any(|event| {
                matches!(
                    event,
                    AppEvent::WorkspaceRefreshed { .. }
                        | AppEvent::DocumentLoaded { .. }
                        | AppEvent::DocumentChanged { .. }
                        | AppEvent::DocumentSaved { .. }
                )
            })
        {
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
            {
                self.search_generation.fetch_add(1, Ordering::Relaxed);
            } else if queued_query
                || (!synchronous_query
                    && events.iter().any(|event| {
                        matches!(
                            event,
                            AppEvent::WorkspaceRefreshed { .. }
                                | AppEvent::DocumentLoaded { .. }
                                | AppEvent::DocumentChanged { .. }
                                | AppEvent::DocumentSaved { .. }
                        )
                    }))
            {
                self.queue_current_search();
            } else if synchronous_query {
                self.search_generation.fetch_add(1, Ordering::Relaxed);
            }
        } else if explicit_refresh {
            self.queue_current_search();
        }
        Ok(events)
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
                    let results = self.search_results(&self.state.documents, &query);
                    vec![AppEvent::SearchQueryChanged {
                        query: SearchQuery(query),
                        results,
                    }]
                }
            }
            AppCommand::QueueSearchQuery(query) => {
                if self.state.search_query.0 == query {
                    Vec::new()
                } else {
                    let needle = query.trim().to_lowercase();
                    let results = self
                        .state
                        .documents
                        .iter()
                        .flat_map(|entry| {
                            search_results_for(entry, &needle, entry.content.as_deref())
                        })
                        .collect();
                    let mut events = vec![AppEvent::SearchQueryChanged {
                        query: SearchQuery(query),
                        results,
                    }];
                    if !needle.is_empty() {
                        events.push(AppEvent::SearchStarted);
                    }
                    events
                }
            }
            AppCommand::PollSearchResults => {
                let mut latest = None;
                while let Ok(finished) = self.search_results_rx.try_recv() {
                    if finished.generation == self.search_generation.load(Ordering::Relaxed) {
                        latest = Some(finished.results);
                    }
                }
                latest
                    .map(|results| vec![AppEvent::SearchResultsUpdated(results)])
                    .unwrap_or_default()
            }
            AppCommand::OpenWorkspace(path) => {
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
                let interval = if self.watcher.is_some() {
                    Duration::from_secs(30)
                } else {
                    Duration::from_secs(2)
                };
                let changed = self
                    .watcher
                    .as_ref()
                    .is_some_and(WorkspaceWatcher::has_changes);
                if changed || self.last_reconcile.elapsed() >= interval {
                    self.last_reconcile = Instant::now();
                    let events = self.resolve(AppCommand::RefreshWorkspace)?;
                    if events.is_empty() {
                        self.queue_current_search();
                    }
                    return Ok(events);
                }
                Vec::new()
            }
            AppCommand::RefreshWorkspace => {
                let workspace = self
                    .state
                    .workspace
                    .as_ref()
                    .ok_or(AppError::WorkspaceNotOpen)?;
                let (_, ids, directories) = self.filesystem.open_workspace(&workspace.root)?;
                let listed: HashSet<_> = ids.iter().collect();
                let previous: HashMap<_, _> = self
                    .state
                    .documents
                    .iter()
                    .map(|entry| (&entry.id, entry))
                    .collect();
                let mut documents = Vec::with_capacity(ids.len());
                for id in &ids {
                    let mut entry =
                        previous
                            .get(&id)
                            .map(|entry| (*entry).clone())
                            .unwrap_or(DocumentEntry {
                                id: id.clone(),
                                content: None,
                                saved_content: None,
                                external_change: false,
                            });
                    if entry.content.is_some() {
                        match self.filesystem.read_document(&workspace.root, &entry.id) {
                            Ok(disk_content)
                                if entry.saved_content.as_ref() == Some(&disk_content) =>
                            {
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
                for entry in &self.state.documents {
                    if !listed.contains(&entry.id) && entry.content.is_some() {
                        let mut missing = entry.clone();
                        missing.external_change = true;
                        documents.push(missing);
                    }
                }
                documents.sort_by(|left, right| left.id.cmp(&right.id));
                if documents == self.state.documents && directories == self.state.directories {
                    Vec::new()
                } else {
                    let needle = self.state.search_query.0.trim().to_lowercase();
                    let search_results = documents
                        .iter()
                        .flat_map(|entry| {
                            search_results_for(entry, &needle, entry.content.as_deref())
                        })
                        .collect();
                    vec![AppEvent::WorkspaceRefreshed {
                        documents,
                        directories,
                        search_results,
                    }]
                }
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
        assert_eq!(snapshot.directories, ["notes"]);
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
        assert_eq!(opened.directories, ["empty", "nested"]);
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
    fn filename_hit_and_content_occurrences_remain_individually_selectable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "note note").unwrap();
        let mut app = Application::new();
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
            .unwrap();
        let found = app
            .dispatch(AppCommand::SetSearchQuery("note".into()))
            .unwrap();
        assert_eq!(found.search_results.len(), 3);
        assert!(found.search_results[0].location.is_none());
        assert_eq!(
            found.search_results[1]
                .location
                .as_ref()
                .unwrap()
                .start_char,
            0
        );
        assert_eq!(
            found.search_results[2]
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
