//! Facts produced by successful commands. Events are transient, not a
//! persisted log; `AppState::apply` is their only consumer
//! that mutates state.

use crate::{DocumentEntry, RecentWorkspace, SearchResult};
use native_workspace_domain::{DocumentId, Workspace};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppEvent {
    GitUpdated {
        snapshot: Arc<crate::GitSnapshot>,
    },
    RecentWorkspacesLoaded {
        recent_workspaces: Vec<RecentWorkspace>,
    },
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
    EntryDeleted {
        id: DocumentId,
    },
    /// Rename and move: `id` and everything inside it now live at `target`.
    EntryRelocated {
        id: DocumentId,
        target: DocumentId,
    },
    /// The query changed; results are cleared until the worker publishes.
    SearchQueryQueued {
        query: String,
    },
    SearchConfigured {
        options: crate::SearchOptions,
    },
    SearchCancelled,
    ReplacementPrepared {
        preview: Option<Arc<crate::ReplacementPreview>>,
    },
    /// A closed file was written; its old search results are no longer valid.
    SearchFileReplaced {
        id: DocumentId,
    },
    /// The current query is searched again; current results stay meanwhile.
    SearchStarted,
    /// Matches from the search worker, appended as they stream in or
    /// replacing the current results when a re-search finishes.
    SearchResultsUpdated {
        results: Vec<SearchResult>,
        replace: bool,
        finished: bool,
        truncated: bool,
    },
    DocumentAdded {
        id: DocumentId,
    },
    DocumentLoaded {
        id: DocumentId,
        content: Arc<str>,
    },
    DocumentOpened {
        id: DocumentId,
    },
    DocumentPreviewed {
        id: DocumentId,
    },
    DocumentKeptOpen {
        id: DocumentId,
    },
    DocumentTabMoved {
        id: DocumentId,
        index: usize,
    },
    DocumentClosed {
        id: DocumentId,
    },
    DocumentChanged {
        id: DocumentId,
        content: Arc<str>,
    },
    DocumentSaved {
        id: DocumentId,
    },
}

impl AppEvent {
    /// Diagnostic name and scalar value; never paths or contents.
    pub(crate) fn diagnostic_fact(&self) -> (&'static str, u64) {
        match self {
            Self::GitUpdated { .. } => ("git.updated", 0),
            Self::RecentWorkspacesLoaded { recent_workspaces } => {
                ("workspace.recents_loaded", recent_workspaces.len() as u64)
            }
            Self::WorkspaceOpened { documents, .. } => ("workspace.opened", documents.len() as u64),
            Self::WorkspaceRefreshed { documents, .. } => {
                ("workspace.refreshed", documents.len() as u64)
            }
            Self::EntryDeleted { .. } => ("filesystem.entry_deleted", 0),
            Self::EntryRelocated { .. } => ("filesystem.entry_relocated", 0),
            Self::SearchQueryQueued { .. } => ("search.input_changed", 0),
            Self::SearchConfigured { .. } => ("search.configured", 0),
            Self::SearchCancelled => ("search.cancelled", 0),
            Self::ReplacementPrepared { .. } => ("replace.preview", 0),
            Self::SearchFileReplaced { .. } => ("replace.file_written", 0),
            Self::SearchStarted => ("search.started", 0),
            Self::SearchResultsUpdated { results, .. } => {
                ("search.results_applied", results.len() as u64)
            }
            Self::DocumentLoaded { content, .. } => ("document.loaded_bytes", content.len() as u64),
            Self::DocumentAdded { .. } => ("document.added", 0),
            Self::DocumentOpened { .. } => ("document.opened", 0),
            Self::DocumentPreviewed { .. } => ("document.previewed", 0),
            Self::DocumentKeptOpen { .. } => ("document.kept_open", 0),
            Self::DocumentTabMoved { .. } => ("document.tab_moved", 0),
            Self::DocumentClosed { .. } => ("document.closed", 0),
            Self::DocumentChanged { content, .. } => {
                ("document.buffer_bytes", content.len() as u64)
            }
            Self::DocumentSaved { .. } => ("document.saved", 0),
        }
    }

    /// The buffer size an event leaves behind, for the devtools gauge.
    pub(crate) fn buffer_bytes(&self) -> Option<u64> {
        match self {
            Self::DocumentLoaded { content, .. } => Some(content.len() as u64),
            Self::DocumentClosed { .. } => Some(0),
            Self::GitUpdated { .. }
            | Self::RecentWorkspacesLoaded { .. }
            | Self::WorkspaceOpened { .. }
            | Self::WorkspaceRefreshed { .. }
            | Self::EntryDeleted { .. }
            | Self::EntryRelocated { .. }
            | Self::SearchQueryQueued { .. }
            | Self::SearchConfigured { .. }
            | Self::SearchCancelled
            | Self::ReplacementPrepared { .. }
            | Self::SearchFileReplaced { .. }
            | Self::SearchStarted
            | Self::SearchResultsUpdated { .. }
            | Self::DocumentAdded { .. }
            | Self::DocumentOpened { .. }
            | Self::DocumentPreviewed { .. }
            | Self::DocumentKeptOpen { .. }
            | Self::DocumentTabMoved { .. }
            | Self::DocumentChanged { .. }
            | Self::DocumentSaved { .. } => None,
        }
    }

    pub(crate) fn is_search_delivery(&self) -> bool {
        matches!(self, Self::SearchResultsUpdated { .. })
    }
}
