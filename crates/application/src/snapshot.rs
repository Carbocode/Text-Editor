//! Owned, immutable views of the application state for frontends.
//!
//! Large lists and document text are shared through `Arc`: an unchanged list
//! keeps its pointer, so adapters compare with `Arc::ptr_eq` and skip
//! reconverting it.

use crate::{PathFilterResult, RecentWorkspace};
use native_workspace_domain::{DocumentId, Workspace};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSummary {
    pub id: DocumentId,
    pub dirty: bool,
    pub external_change: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub content: Arc<str>,
    pub dirty: bool,
    pub external_change: bool,
}

/// One occurrence of the search query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub id: DocumentId,
    /// The matching line, trimmed around the match.
    pub snippet: String,
    pub location: SearchLocation,
}

/// A match position in every unit the native text views use.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SearchLocation {
    pub line: u64,
    pub column: u64,
    /// Unicode scalar offsets (GTK).
    pub start_char: u64,
    pub end_char: u64,
    /// UTF-16 offsets (AppKit and Qt).
    pub start_utf16: u64,
    pub end_utf16: u64,
}

/// A rename or move whose filesystem confirmation is pending.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relocation {
    pub source: DocumentId,
    pub target: DocumentId,
}

/// Background work a [`crate::Session`] is running.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Activity {
    /// The session worker runs; further commands are deferred or rejected.
    pub busy: bool,
    /// Already applied to every path in the snapshot.
    pub relocation: Option<Relocation>,
    /// The relocation has taken long enough to show a progress indicator.
    pub relocation_slow: bool,
    /// A deletion or OS file transfer runs; editing pauses until it finishes.
    pub editing_paused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    /// Opaque diagnostics correlation; never used to resolve commands.
    pub diagnostic_operation: u64,
    /// Increments once per committed command or projection.
    pub revision: u64,
    pub workspace: Option<Workspace>,
    pub recent_workspaces: Vec<RecentWorkspace>,
    /// Workspace files and open external documents, sorted by identity.
    pub documents: Arc<Vec<DocumentSummary>>,
    pub directories: Arc<Vec<String>>,
    pub search_query: String,
    pub search_options: crate::SearchOptions,
    pub search_error: Option<String>,
    pub replacement_preview: Option<Arc<crate::ReplacementPreview>>,
    /// Content matches sorted by path, then by position.
    pub search_results: Arc<Vec<SearchResult>>,
    pub search_in_progress: bool,
    /// The search stopped at its result limit; more matches may exist.
    pub search_truncated: bool,
    /// The Files name filter; its query may lag behind the typed text.
    pub file_filter: Arc<PathFilterResult>,
    pub file_filter_in_progress: bool,
    /// Open tabs in visual order, independent of the workspace listing.
    pub open_documents: Vec<DocumentId>,
    /// At most one clean tab may be replaced by the next preview opening.
    pub preview_document: Option<DocumentId>,
    pub active_document: Option<DocumentSnapshot>,
    /// Changes whenever the editor shows a different document, but not when
    /// the shown document is renamed or moved. Editors reset undo history,
    /// selection and scrolling only when it changes.
    pub editor_generation: u64,
    pub activity: Activity,
    /// The last failure, shown until the next successful user command.
    pub error: Option<String>,
}

impl AppSnapshot {
    pub fn has_unsaved_documents(&self) -> bool {
        self.documents.iter().any(|document| document.dirty)
    }

    /// Resolve a native selection before sending a replacement intention.
    pub fn replacement_target(
        &self,
        path: Option<&str>,
        result_index: Option<u64>,
    ) -> Result<crate::ReplacementTarget, crate::AppError> {
        if let Some(index) = result_index {
            let result = usize::try_from(index)
                .ok()
                .and_then(|index| self.search_results.get(index))
                .ok_or(crate::AppError::NoReplacements)?;
            Ok(crate::ReplacementTarget::Match {
                id: result.id.clone(),
                location: result.location.clone(),
            })
        } else if let Some(path) = path {
            Ok(crate::ReplacementTarget::Document(
                DocumentId::from_document_path(path)
                    .map_err(|_| crate::AppError::InvalidEntryName)?,
            ))
        } else {
            Ok(crate::ReplacementTarget::All)
        }
    }
    pub fn workspace_root(&self) -> Option<&std::path::Path> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.root.as_path())
    }

    pub fn workspace_name(&self) -> Option<&str> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.name.as_str())
    }

    /// The listed document with this path, found without scanning the list.
    pub fn document_summary(&self, path: &str) -> Option<&DocumentSummary> {
        self.documents
            .binary_search_by(|document| document.id.as_str().cmp(path))
            .ok()
            .map(|index| &self.documents[index])
    }

    pub fn is_dirty(&self, id: &DocumentId) -> bool {
        self.document_summary(id.as_str())
            .is_some_and(|document| document.dirty)
    }

    pub fn is_open(&self, id: &DocumentId) -> bool {
        self.open_documents.contains(id)
    }

    pub fn active_id(&self) -> Option<&DocumentId> {
        self.active_document.as_ref().map(|document| &document.id)
    }
}
