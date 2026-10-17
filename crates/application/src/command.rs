//! Frontend intentions and their execution policy.
//!
//! Every `match` on [`AppCommand`] in this module is exhaustive on purpose:
//! adding a command fails to compile until its diagnostics name, path
//! rebasing, scheduling and busy policy are decided here, in one place.

use crate::{AppError, Relocation, ReplacementTarget, SearchOptions};
use native_workspace_domain::{DocumentId, path};
use std::path::PathBuf;

/// An intention requested by a frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    OpenWorkspace(PathBuf),
    /// Ordered OS open requests: folders select a workspace; files open permanent tabs.
    /// Without a workspace, the first file selects its parent folder.
    OpenPaths(Vec<PathBuf>),
    /// Open OS files permanently. Successful earlier files survive a later failure.
    OpenFiles(Vec<PathBuf>),
    OpenFile(PathBuf),
    /// Move OS files into a workspace folder, never replacing a destination.
    MoveFiles {
        paths: Vec<PathBuf>,
        directory: String,
    },
    MoveFile {
        path: PathBuf,
        directory: String,
    },
    /// Rescan the tree and reconcile open buffers with the disk.
    RefreshWorkspace,
    /// Collect watcher events and finished background scans.
    PollWorkspaceChanges,
    /// Record the query; the search worker debounces and searches.
    QueueSearchQuery(String),
    ConfigureSearch(SearchOptions),
    RefreshSearch,
    CancelSearch,
    /// Prepare a reviewable edit without changing files or buffers.
    PrepareReplacement {
        replacement: String,
        preserve_case: bool,
        target: ReplacementTarget,
    },
    ApplyReplacement,
    DiscardReplacement,
    /// Collect streamed search results.
    PollSearchResults,
    CreateEntry {
        id: DocumentId,
        directory: bool,
    },
    RenameEntry {
        id: DocumentId,
        new_name: String,
    },
    /// Move into `directory`, a workspace-relative folder ("" for the root).
    MoveEntry {
        id: DocumentId,
        directory: String,
    },
    DeleteEntry(DocumentId),
    /// Open permanently, or promote an already open preview.
    OpenDocument(DocumentId),
    /// Activate an existing tab, or open in the single replaceable preview slot.
    PreviewDocument(DocumentId),
    /// Promote an existing preview without changing the active document.
    KeepDocumentOpen(DocumentId),
    MoveDocumentTab {
        id: DocumentId,
        index: usize,
    },
    /// Close a clean tab; modified tabs need an explicit choice.
    CloseDocument(DocumentId),
    DiscardAndCloseDocument(DocumentId),
    /// Save, then close only if the save succeeded.
    SaveAndCloseDocument(DocumentId),
    ChangeDocument {
        id: DocumentId,
        content: String,
    },
    SaveDocument(DocumentId),
    /// Save modified tabs in tab order, stopping at the first failure.
    SaveAllDocuments,
    ReloadDocument(DocumentId),
    OverwriteDocument(DocumentId),
}

/// Where a [`crate::Session`] executes a command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Schedule {
    /// On the caller's thread; cheap or needed before the call returns.
    Inline,
    /// On the session worker; the published state moves immediately.
    Relocation,
    /// On the session worker; editing pauses until it finishes.
    Exclusive,
    /// On the session worker; nothing is projected.
    Background,
}

/// What a [`crate::Session`] does with a command while its worker runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusyPolicy {
    /// Nothing to do until the worker returns.
    Skip,
    /// Replay after the worker returns, remapped if a relocation fails.
    Defer,
    /// Show the edit at once and replay it after the worker returns.
    ProjectEdit,
    /// Refuse with [`crate::AppError::OperationInProgress`].
    Reject,
}

impl AppCommand {
    pub(crate) fn diagnostic_name(&self) -> &'static str {
        match self {
            Self::OpenWorkspace(_) => "command.workspace_open",
            Self::OpenPaths(_) => "command.paths_open",
            Self::OpenFiles(_) => "command.files_open",
            Self::OpenFile(_) => "command.file_open",
            Self::MoveFiles { .. } => "command.files_move",
            Self::MoveFile { .. } => "command.file_move",
            Self::RefreshWorkspace => "command.workspace_refresh",
            Self::PollWorkspaceChanges => "command.workspace_poll",
            Self::QueueSearchQuery(_) => "command.search_input",
            Self::ConfigureSearch(_) => "command.search_configure",
            Self::RefreshSearch => "command.search_refresh",
            Self::CancelSearch => "command.search_cancel",
            Self::PrepareReplacement { .. } => "command.replace_preview",
            Self::ApplyReplacement => "command.replace_apply",
            Self::DiscardReplacement => "command.replace_discard",
            Self::PollSearchResults => "command.search_poll",
            Self::CreateEntry { .. } => "command.entry_create",
            Self::RenameEntry { .. } => "command.entry_rename",
            Self::MoveEntry { .. } => "command.entry_move",
            Self::DeleteEntry(_) => "command.entry_delete",
            Self::OpenDocument(_) => "command.document_open",
            Self::PreviewDocument(_) => "command.document_preview",
            Self::KeepDocumentOpen(_) => "command.document_keep_open",
            Self::MoveDocumentTab { .. } => "command.document_tab_move",
            Self::CloseDocument(_) => "command.document_close",
            Self::DiscardAndCloseDocument(_) => "command.document_discard_close",
            Self::SaveAndCloseDocument(_) => "command.document_save_close",
            Self::ChangeDocument { .. } => "command.document_change",
            Self::SaveDocument(_) => "command.document_save",
            Self::SaveAllDocuments => "command.document_save_all",
            Self::ReloadDocument(_) => "command.document_reload",
            Self::OverwriteDocument(_) => "command.document_overwrite",
        }
    }

    /// The command addressed to the same entries after `from` moved to `to`.
    pub(crate) fn rebased(self, from: &DocumentId, to: &DocumentId) -> Self {
        let id = |id: DocumentId| id.rebased(from, to);
        let folder =
            |directory: String| path::rebase(&directory, from.as_str(), to.as_str()).into_owned();
        match self {
            Self::PrepareReplacement {
                replacement,
                preserve_case,
                target,
            } => Self::PrepareReplacement {
                replacement,
                preserve_case,
                target: target.rebased(from, to),
            },
            Self::MoveFiles { paths, directory } => Self::MoveFiles {
                paths,
                directory: folder(directory),
            },
            Self::MoveFile { path, directory } => Self::MoveFile {
                path,
                directory: folder(directory),
            },
            Self::OpenPaths(_)
            | Self::OpenFiles(_)
            | Self::OpenFile(_)
            | Self::OpenWorkspace(_)
            | Self::RefreshWorkspace
            | Self::PollWorkspaceChanges
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::ApplyReplacement
            | Self::DiscardReplacement
            | Self::PollSearchResults
            | Self::SaveAllDocuments => self,
            Self::CreateEntry {
                id: entry,
                directory,
            } => Self::CreateEntry {
                id: id(entry),
                directory,
            },
            Self::RenameEntry {
                id: entry,
                new_name,
            } => Self::RenameEntry {
                id: id(entry),
                new_name,
            },
            Self::MoveEntry {
                id: entry,
                directory,
            } => Self::MoveEntry {
                id: id(entry),
                directory: folder(directory),
            },
            Self::DeleteEntry(entry) => Self::DeleteEntry(id(entry)),
            Self::OpenDocument(entry) => Self::OpenDocument(id(entry)),
            Self::PreviewDocument(entry) => Self::PreviewDocument(id(entry)),
            Self::KeepDocumentOpen(entry) => Self::KeepDocumentOpen(id(entry)),
            Self::MoveDocumentTab { id: entry, index } => Self::MoveDocumentTab {
                id: id(entry),
                index,
            },
            Self::CloseDocument(entry) => Self::CloseDocument(id(entry)),
            Self::DiscardAndCloseDocument(entry) => Self::DiscardAndCloseDocument(id(entry)),
            Self::SaveAndCloseDocument(entry) => Self::SaveAndCloseDocument(id(entry)),
            Self::ChangeDocument { id: entry, content } => Self::ChangeDocument {
                id: id(entry),
                content,
            },
            Self::SaveDocument(entry) => Self::SaveDocument(id(entry)),
            Self::ReloadDocument(entry) => Self::ReloadDocument(id(entry)),
            Self::OverwriteDocument(entry) => Self::OverwriteDocument(id(entry)),
        }
    }

    /// The relocation a rename or move requests, before validation against
    /// the workspace; `None` for every other command.
    pub(crate) fn relocation(&self) -> Option<Result<Relocation, AppError>> {
        let target = |target: Result<DocumentId, _>| target.map_err(|_| AppError::InvalidEntryName);
        match self {
            Self::RenameEntry { id, new_name } => {
                Some(target(id.renamed(new_name)).map(|target| Relocation {
                    source: id.clone(),
                    target,
                }))
            }
            Self::MoveEntry { id, directory } => {
                Some(target(id.moved_into(directory)).map(|target| Relocation {
                    source: id.clone(),
                    target,
                }))
            }
            Self::OpenPaths(_)
            | Self::OpenFiles(_)
            | Self::OpenFile(_)
            | Self::MoveFiles { .. }
            | Self::MoveFile { .. }
            | Self::OpenWorkspace(_)
            | Self::RefreshWorkspace
            | Self::PollWorkspaceChanges
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::ApplyReplacement
            | Self::DiscardReplacement
            | Self::PrepareReplacement { .. }
            | Self::PollSearchResults
            | Self::CreateEntry { .. }
            | Self::DeleteEntry(_)
            | Self::OpenDocument(_)
            | Self::PreviewDocument(_)
            | Self::KeepDocumentOpen(_)
            | Self::MoveDocumentTab { .. }
            | Self::CloseDocument(_)
            | Self::DiscardAndCloseDocument(_)
            | Self::SaveAndCloseDocument(_)
            | Self::ChangeDocument { .. }
            | Self::SaveDocument(_)
            | Self::SaveAllDocuments
            | Self::ReloadDocument(_)
            | Self::OverwriteDocument(_) => None,
        }
    }

    /// Composite commands run as a sequence of commands, each committed on
    /// its own so a failure never hides earlier successful steps.
    pub(crate) fn steps(&self, dirty_tabs: impl FnOnce() -> Vec<DocumentId>) -> Option<Vec<Self>> {
        match self {
            Self::OpenFiles(paths) => Some(paths.iter().cloned().map(Self::OpenFile).collect()),
            Self::MoveFiles { paths, directory } => Some(
                paths
                    .iter()
                    .cloned()
                    .map(|path| Self::MoveFile {
                        path,
                        directory: directory.clone(),
                    })
                    .collect(),
            ),
            Self::SaveAllDocuments => {
                Some(dirty_tabs().into_iter().map(Self::SaveDocument).collect())
            }
            Self::SaveAndCloseDocument(id) => Some(vec![
                Self::SaveDocument(id.clone()),
                Self::CloseDocument(id.clone()),
            ]),
            Self::OpenPaths(_)
            | Self::OpenFile(_)
            | Self::MoveFile { .. }
            | Self::OpenWorkspace(_)
            | Self::RefreshWorkspace
            | Self::PollWorkspaceChanges
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::ApplyReplacement
            | Self::DiscardReplacement
            | Self::PrepareReplacement { .. }
            | Self::PollSearchResults
            | Self::CreateEntry { .. }
            | Self::RenameEntry { .. }
            | Self::MoveEntry { .. }
            | Self::DeleteEntry(_)
            | Self::OpenDocument(_)
            | Self::PreviewDocument(_)
            | Self::KeepDocumentOpen(_)
            | Self::MoveDocumentTab { .. }
            | Self::CloseDocument(_)
            | Self::DiscardAndCloseDocument(_)
            | Self::ChangeDocument { .. }
            | Self::SaveDocument(_)
            | Self::ReloadDocument(_)
            | Self::OverwriteDocument(_) => None,
        }
    }

    /// Whether a successful command searches the workspace again: closing a
    /// buffer or changing the tree makes disk content authoritative again.
    pub(crate) fn researches(&self) -> bool {
        match self {
            Self::MoveFiles { .. }
            | Self::MoveFile { .. }
            | Self::RefreshWorkspace
            | Self::DeleteEntry(_)
            | Self::CloseDocument(_)
            | Self::DiscardAndCloseDocument(_) => true,
            Self::OpenPaths(_)
            | Self::OpenFiles(_)
            | Self::OpenFile(_)
            | Self::OpenWorkspace(_)
            | Self::PollWorkspaceChanges
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::ApplyReplacement
            | Self::DiscardReplacement
            | Self::PrepareReplacement { .. }
            | Self::PollSearchResults
            | Self::CreateEntry { .. }
            | Self::RenameEntry { .. }
            | Self::MoveEntry { .. }
            | Self::OpenDocument(_)
            | Self::PreviewDocument(_)
            | Self::KeepDocumentOpen(_)
            | Self::MoveDocumentTab { .. }
            | Self::SaveAndCloseDocument(_)
            | Self::ChangeDocument { .. }
            | Self::SaveDocument(_)
            | Self::SaveAllDocuments
            | Self::ReloadDocument(_)
            | Self::OverwriteDocument(_) => false,
        }
    }

    pub(crate) fn schedule(&self) -> Schedule {
        match self {
            Self::RenameEntry { .. } | Self::MoveEntry { .. } => Schedule::Relocation,
            Self::OpenPaths(_)
            | Self::MoveFiles { .. }
            | Self::MoveFile { .. }
            | Self::DeleteEntry(_)
            | Self::ApplyReplacement => Schedule::Exclusive,
            Self::OpenFiles(_)
            | Self::OpenFile(_)
            | Self::RefreshWorkspace
            | Self::PrepareReplacement { .. } => Schedule::Background,
            Self::OpenWorkspace(_)
            | Self::PollWorkspaceChanges
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::DiscardReplacement
            | Self::PollSearchResults
            | Self::CreateEntry { .. }
            | Self::OpenDocument(_)
            | Self::PreviewDocument(_)
            | Self::KeepDocumentOpen(_)
            | Self::MoveDocumentTab { .. }
            | Self::CloseDocument(_)
            | Self::DiscardAndCloseDocument(_)
            | Self::SaveAndCloseDocument(_)
            | Self::ChangeDocument { .. }
            | Self::SaveDocument(_)
            | Self::SaveAllDocuments
            | Self::ReloadDocument(_)
            | Self::OverwriteDocument(_) => Schedule::Inline,
        }
    }

    /// Policy while the worker runs a command scheduled as `running`.
    pub(crate) fn while_busy(&self, running: Schedule) -> BusyPolicy {
        let exclusive = running == Schedule::Exclusive;
        match self {
            Self::PollWorkspaceChanges | Self::PollSearchResults => BusyPolicy::Skip,
            Self::OpenPaths(_) => BusyPolicy::Defer,
            // Exclusive operations may remove or relocate the edited document.
            Self::ChangeDocument { .. } if exclusive => BusyPolicy::Reject,
            Self::ChangeDocument { .. } => BusyPolicy::ProjectEdit,
            // Destructive or relocating commands never queue behind another.
            Self::MoveFiles { .. }
            | Self::MoveFile { .. }
            | Self::DeleteEntry(_)
            | Self::RenameEntry { .. }
            | Self::MoveEntry { .. }
            | Self::PrepareReplacement { .. }
            | Self::ApplyReplacement => BusyPolicy::Reject,
            _ if exclusive => BusyPolicy::Reject,
            Self::OpenFiles(_)
            | Self::OpenFile(_)
            | Self::OpenWorkspace(_)
            | Self::RefreshWorkspace
            | Self::QueueSearchQuery(_)
            | Self::ConfigureSearch(_)
            | Self::RefreshSearch
            | Self::CancelSearch
            | Self::DiscardReplacement
            | Self::CreateEntry { .. }
            | Self::OpenDocument(_)
            | Self::PreviewDocument(_)
            | Self::KeepDocumentOpen(_)
            | Self::MoveDocumentTab { .. }
            | Self::CloseDocument(_)
            | Self::DiscardAndCloseDocument(_)
            | Self::SaveAndCloseDocument(_)
            | Self::SaveDocument(_)
            | Self::SaveAllDocuments
            | Self::ReloadDocument(_)
            | Self::OverwriteDocument(_) => BusyPolicy::Defer,
        }
    }
}
