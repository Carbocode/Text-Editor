//! Authoritative state and its reducer.
//!
//! [`AppState::apply`] is the only code that mutates state. Shared lists are
//! copy-on-write (`Arc::make_mut`), and document summaries are cached behind
//! [`DocumentTable`], whose mutators invalidate the cache themselves: no code
//! path can publish a stale list by forgetting an invalidation.

use crate::{
    AppEvent, AppSnapshot, DocumentSnapshot, DocumentSummary, RecentWorkspace, SearchResult,
    search::Matcher,
};
use native_workspace_domain::{DocumentId, Workspace};
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};

/// A workspace file and, while it is open, its buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentEntry {
    id: DocumentId,
    buffer: Option<Buffer>,
    external_change: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Buffer {
    content: Arc<str>,
    /// The disk content the buffer was loaded from or last saved as.
    saved: Arc<str>,
    dirty: bool,
}

impl DocumentEntry {
    pub(crate) fn new(id: DocumentId) -> Self {
        Self {
            id,
            buffer: None,
            external_change: false,
        }
    }

    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    pub(crate) fn content(&self) -> Option<&Arc<str>> {
        self.buffer.as_ref().map(|buffer| &buffer.content)
    }

    pub(crate) fn saved_content(&self) -> Option<&str> {
        self.buffer.as_ref().map(|buffer| &*buffer.saved)
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.buffer.is_some()
    }

    pub(crate) fn dirty(&self) -> bool {
        self.buffer.as_ref().is_some_and(|buffer| buffer.dirty)
    }

    pub(crate) fn load(&mut self, content: Arc<str>) {
        self.buffer = Some(Buffer {
            saved: content.clone(),
            content,
            dirty: false,
        });
        self.external_change = false;
    }

    pub(crate) fn edit(&mut self, content: Arc<str>) {
        let buffer = self.buffer.as_mut().expect("edits target loaded documents");
        buffer.dirty = content != buffer.saved;
        buffer.content = content;
    }

    pub(crate) fn mark_saved(&mut self) {
        let buffer = self.buffer.as_mut().expect("saves target loaded documents");
        buffer.saved = buffer.content.clone();
        buffer.dirty = false;
        self.external_change = false;
    }

    pub(crate) fn unload(&mut self) {
        self.buffer = None;
        self.external_change = false;
    }

    pub(crate) fn set_external_change(&mut self, changed: bool) {
        self.external_change = changed;
    }

    fn summary(&self) -> DocumentSummary {
        DocumentSummary {
            id: self.id.clone(),
            dirty: self.dirty(),
            external_change: self.external_change,
        }
    }
}

/// Documents sorted by path, with a cached list of summaries.
#[derive(Clone, Debug, Default)]
pub(crate) struct DocumentTable {
    entries: Vec<DocumentEntry>,
    summaries: OnceLock<Arc<Vec<DocumentSummary>>>,
}

impl DocumentTable {
    pub fn entries(&self) -> &[DocumentEntry] {
        &self.entries
    }

    pub fn get(&self, id: &DocumentId) -> Option<&DocumentEntry> {
        self.position(id).ok().map(|index| &self.entries[index])
    }

    pub fn contains(&self, id: &DocumentId) -> bool {
        self.position(id).is_ok()
    }

    fn position(&self, id: &DocumentId) -> Result<usize, usize> {
        self.entries.binary_search_by(|entry| entry.id.cmp(id))
    }

    pub fn summaries(&self) -> Arc<Vec<DocumentSummary>> {
        self.summaries
            .get_or_init(|| Arc::new(self.entries.iter().map(DocumentEntry::summary).collect()))
            .clone()
    }

    /// Mutates one validated entry; summaries are invalidated only when the
    /// change is visible in them, so typing does not copy the whole list.
    fn update(&mut self, id: &DocumentId, change: impl FnOnce(&mut DocumentEntry)) {
        let index = self
            .position(id)
            .expect("document validated before its event");
        let entry = &mut self.entries[index];
        let before = (entry.dirty(), entry.external_change);
        change(entry);
        if before != (entry.dirty(), entry.external_change) {
            self.summaries = OnceLock::new();
        }
    }

    fn replace(&mut self, mut entries: Vec<DocumentEntry>) {
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        self.entries = entries;
        self.summaries = OnceLock::new();
    }

    fn retain(&mut self, keep: impl FnMut(&DocumentEntry) -> bool) {
        self.entries.retain(keep);
        self.summaries = OnceLock::new();
    }

    fn rebase(&mut self, from: &DocumentId, to: &DocumentId) {
        let entries = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|mut entry| {
                entry.id = entry.id.rebased(from, to);
                entry
            })
            .collect();
        self.replace(entries);
    }
}

/// Authoritative current state. Sessions clone it to project pending work.
#[derive(Clone, Debug, Default)]
pub(crate) struct AppState {
    pub(crate) revision: u64,
    pub(crate) workspace: Option<Workspace>,
    pub(crate) recent_workspaces: Vec<RecentWorkspace>,
    pub(crate) documents: DocumentTable,
    pub(crate) directories: Arc<Vec<String>>,
    pub(crate) search_query: String,
    pub(crate) search_results: Arc<Vec<SearchResult>>,
    pub(crate) search_in_progress: bool,
    pub(crate) search_truncated: bool,
    /// Documents whose matches were recomputed from their buffer while a
    /// search ran; that search's possibly older matches for them are dropped.
    search_touched: HashSet<DocumentId>,
    pub(crate) open_documents: Vec<DocumentId>,
    pub(crate) preview_document: Option<DocumentId>,
    pub(crate) active_document: Option<DocumentId>,
    pub(crate) editor_generation: u64,
}

impl AppState {
    pub fn snapshot(&self) -> AppSnapshot {
        let active_document = self
            .active_document
            .as_ref()
            .and_then(|id| self.documents.get(id))
            .and_then(|entry| {
                Some(DocumentSnapshot {
                    id: entry.id.clone(),
                    content: entry.content()?.clone(),
                    dirty: entry.dirty(),
                    external_change: entry.external_change,
                })
            });
        AppSnapshot {
            diagnostic_operation: 0,
            revision: self.revision,
            workspace: self.workspace.clone(),
            recent_workspaces: self.recent_workspaces.clone(),
            documents: self.documents.summaries(),
            directories: self.directories.clone(),
            search_query: self.search_query.clone(),
            search_results: self.search_results.clone(),
            search_in_progress: self.search_in_progress,
            search_truncated: self.search_truncated,
            file_filter: Default::default(),
            file_filter_in_progress: false,
            open_documents: self.open_documents.clone(),
            preview_document: self.preview_document.clone(),
            active_document,
            editor_generation: self.editor_generation,
            activity: Default::default(),
            error: None,
        }
    }

    pub(crate) fn has_query(&self) -> bool {
        !self.search_query.trim().is_empty()
    }

    /// Open tabs with unsaved changes, in tab order.
    pub(crate) fn dirty_tabs(&self) -> Vec<DocumentId> {
        self.open_documents
            .iter()
            .filter(|id| self.documents.get(id).is_some_and(DocumentEntry::dirty))
            .cloned()
            .collect()
    }

    /// Whether `path` names a listed file or folder.
    pub(crate) fn entry_exists(&self, path: &str) -> bool {
        self.directories
            .binary_search_by(|dir| dir.as_str().cmp(path))
            .is_ok()
            || DocumentId::parse(path).is_ok_and(|id| self.documents.contains(&id))
    }

    pub(crate) fn apply(&mut self, event: &AppEvent) {
        let shown = self.active_document.clone();
        match event {
            AppEvent::RecentWorkspacesLoaded { recent_workspaces } => {
                self.recent_workspaces = recent_workspaces.clone()
            }
            AppEvent::WorkspaceOpened {
                workspace,
                documents,
                directories,
                recent_workspaces,
            } => {
                self.workspace = Some(workspace.clone());
                self.recent_workspaces = recent_workspaces.clone();
                self.documents
                    .replace(documents.iter().cloned().map(DocumentEntry::new).collect());
                self.directories = Arc::new(directories.clone());
                self.search_query.clear();
                self.search_results = Arc::default();
                self.search_in_progress = false;
                self.search_truncated = false;
                self.open_documents.clear();
                self.preview_document = None;
                self.active_document = None;
            }
            AppEvent::WorkspaceRefreshed {
                documents,
                directories,
            } => {
                self.documents.replace(documents.clone());
                self.directories = Arc::new(directories.clone());
                // Like VS Code, disk changes do not rerun a search: removed
                // files drop out and open buffers are matched again.
                if self
                    .search_results
                    .iter()
                    .any(|result| !self.documents.contains(&result.id))
                {
                    let documents = &self.documents;
                    Arc::make_mut(&mut self.search_results)
                        .retain(|result| documents.contains(&result.id));
                }
                let loaded: Vec<_> = self
                    .documents
                    .entries()
                    .iter()
                    .filter(|entry| entry.is_loaded())
                    .map(|entry| entry.id.clone())
                    .collect();
                for id in &loaded {
                    self.refresh_matches(id);
                }
                let removed: HashSet<DocumentId> = self
                    .open_documents
                    .iter()
                    .filter(|id| !self.documents.contains(id))
                    .cloned()
                    .collect();
                self.close_tabs(|id| removed.contains(id));
            }
            AppEvent::EntryDeleted { id } => {
                self.documents.retain(|entry| !entry.id.is_within(id));
                Arc::make_mut(&mut self.directories)
                    .retain(|path| !native_workspace_domain::path::is_within(path, id.as_str()));
                if self
                    .search_results
                    .iter()
                    .any(|result| result.id.is_within(id))
                {
                    Arc::make_mut(&mut self.search_results)
                        .retain(|result| !result.id.is_within(id));
                }
                self.close_tabs(|open| open.is_within(id));
            }
            AppEvent::EntryRelocated { id, target } => {
                self.search_touched = std::mem::take(&mut self.search_touched)
                    .into_iter()
                    .map(|touched| touched.rebased(id, target))
                    .collect();
                self.documents.rebase(id, target);
                let directories = Arc::make_mut(&mut self.directories);
                for path in directories.iter_mut() {
                    *path =
                        native_workspace_domain::path::rebase(path, id.as_str(), target.as_str())
                            .into_owned();
                }
                directories.sort();
                if self
                    .search_results
                    .iter()
                    .any(|result| result.id.is_within(id))
                {
                    // Matches move with their files; nothing is searched again.
                    let results = Arc::make_mut(&mut self.search_results);
                    for result in results.iter_mut() {
                        result.id = result.id.rebased(id, target);
                    }
                    results.sort_by(|left, right| left.id.cmp(&right.id));
                }
                for open in &mut self.open_documents {
                    *open = open.rebased(id, target);
                }
                for tab in [&mut self.preview_document, &mut self.active_document]
                    .into_iter()
                    .flatten()
                {
                    *tab = tab.rebased(id, target);
                }
            }
            AppEvent::SearchQueryQueued { query } => {
                self.search_query = query.clone();
                self.search_results = Arc::default();
                self.search_in_progress = self.has_query();
                self.search_truncated = false;
                self.search_touched.clear();
            }
            AppEvent::SearchStarted => {
                self.search_in_progress = true;
                self.search_touched.clear();
            }
            AppEvent::SearchResultsUpdated {
                results,
                replace,
                finished,
                truncated,
            } => {
                let touched = std::mem::take(&mut self.search_touched);
                let current = Arc::make_mut(&mut self.search_results);
                if *replace {
                    current.clone_from(results);
                } else {
                    current.extend(
                        results
                            .iter()
                            .filter(|result| !touched.contains(&result.id))
                            .cloned(),
                    );
                }
                // A stable sort keeps each file's matches in text order.
                current.sort_by(|left, right| left.id.cmp(&right.id));
                if *replace {
                    for id in &touched {
                        if self.documents.contains(id) {
                            self.refresh_matches(id);
                        }
                    }
                }
                if *finished {
                    self.search_in_progress = false;
                    self.search_truncated = *truncated;
                } else {
                    self.search_touched = touched;
                }
            }
            AppEvent::DocumentLoaded { id, content } => {
                self.documents
                    .update(id, |entry| entry.load(content.clone()));
                self.refresh_matches(id);
            }
            AppEvent::DocumentOpened { id } => {
                if !self.open_documents.contains(id) {
                    self.open_documents.push(id.clone());
                }
                self.active_document = Some(id.clone());
            }
            AppEvent::DocumentPreviewed { id } => {
                self.preview_document = Some(id.clone());
            }
            AppEvent::DocumentKeptOpen { id } => self.promote(id),
            AppEvent::DocumentTabMoved { id, index } => {
                self.promote(id);
                let old = self
                    .open_documents
                    .iter()
                    .position(|open| open == id)
                    .expect("tab validated before its event");
                let tab = self.open_documents.remove(old);
                self.open_documents.insert(*index, tab);
            }
            AppEvent::DocumentClosed { id } => {
                self.close_tabs(|open| open == id);
                self.documents.update(id, DocumentEntry::unload);
                self.refresh_matches(id);
            }
            AppEvent::DocumentChanged { id, content } => {
                // A real edit permanently promotes a preview, even after undo.
                self.promote(id);
                self.documents
                    .update(id, |entry| entry.edit(content.clone()));
                self.refresh_matches(id);
            }
            AppEvent::DocumentSaved { id } => {
                self.promote(id);
                self.documents.update(id, DocumentEntry::mark_saved);
            }
        }
        let relocated = matches!(event, AppEvent::EntryRelocated { .. });
        if self.active_document != shown && !relocated {
            self.editor_generation = self.editor_generation.wrapping_add(1);
        }
    }

    fn promote(&mut self, id: &DocumentId) {
        if self.preview_document.as_ref() == Some(id) {
            self.preview_document = None;
        }
    }

    /// Removes matching tabs. A removed active tab is replaced by its visual
    /// neighbor: the next tab, or the previous one when it was last.
    fn close_tabs(&mut self, remove: impl Fn(&DocumentId) -> bool) {
        let active_index = self
            .active_document
            .as_ref()
            .and_then(|active| self.open_documents.iter().position(|open| open == active));
        self.open_documents.retain(|open| !remove(open));
        if self.preview_document.as_ref().is_some_and(&remove) {
            self.preview_document = None;
        }
        if self.active_document.as_ref().is_some_and(&remove) {
            self.active_document = active_index.and_then(|index| {
                let last = self.open_documents.len().checked_sub(1)?;
                self.open_documents.get(index.min(last)).cloned()
            });
        }
    }

    /// Recomputes the matches of one document from its buffer, or drops them
    /// when it is closed: the next search reads its disk content again.
    fn refresh_matches(&mut self, id: &DocumentId) {
        if !self.has_query() {
            return;
        }
        if self.search_in_progress {
            self.search_touched.insert(id.clone());
        }
        let mut found = Vec::new();
        if let Some(content) = self.documents.get(id).and_then(DocumentEntry::content) {
            Matcher::new(&self.search_query).content_results(id, content, &mut found);
        }
        let start = self
            .search_results
            .partition_point(|result| result.id < *id);
        let end = self
            .search_results
            .partition_point(|result| result.id <= *id);
        if start == end && found.is_empty() {
            return;
        }
        Arc::make_mut(&mut self.search_results).splice(start..end, found);
    }
}
