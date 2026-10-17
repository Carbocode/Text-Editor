//! Application errors. Their `Display` text is the only user-facing wording:
//! adapters forward it and never rephrase or re-map messages.

use native_workspace_domain::DocumentId;
use native_workspace_filesystem::FilesystemError;
use native_workspace_storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("No workspace is open.")]
    WorkspaceNotOpen,
    #[error("Enter a single file or folder name.")]
    InvalidEntryName,
    #[error("A folder cannot be moved inside itself.")]
    InvalidMove,
    #[error("“{0}” already exists.")]
    DestinationExists(DocumentId),
    #[error("“{0}” was not found.")]
    DocumentNotFound(DocumentId),
    #[error("“{0}” is not open.")]
    DocumentNotOpen(DocumentId),
    #[error("Save modified documents first.")]
    UnsavedChanges,
    #[error("Save all modified documents in this file or folder before deleting it.")]
    DeleteUnsavedChanges,
    #[error("A file operation is in progress; wait for it to finish.")]
    OperationInProgress,
    #[error("“{0}” changed on disk. Reload it or explicitly overwrite the disk version.")]
    ExternalChanges(DocumentId),
    #[error("A background operation stopped unexpectedly; reopen the workspace.")]
    BackgroundTaskFailed,
    #[error("The application revision overflowed.")]
    RevisionOverflow,
    #[error(transparent)]
    Filesystem(#[from] FilesystemError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// A payload-free classification for adapters and diagnostics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AppErrorKind {
    WorkspaceNotOpen,
    InvalidEntryName,
    InvalidMove,
    DestinationExists,
    DocumentNotFound,
    DocumentNotOpen,
    UnsavedChanges,
    DeleteUnsavedChanges,
    OperationInProgress,
    ExternalChanges,
    BackgroundTaskFailed,
    RevisionOverflow,
    Filesystem,
    Storage,
}

impl AppError {
    pub fn kind(&self) -> AppErrorKind {
        match self {
            Self::WorkspaceNotOpen => AppErrorKind::WorkspaceNotOpen,
            Self::InvalidEntryName => AppErrorKind::InvalidEntryName,
            Self::InvalidMove => AppErrorKind::InvalidMove,
            Self::DestinationExists(_) => AppErrorKind::DestinationExists,
            Self::DocumentNotFound(_) => AppErrorKind::DocumentNotFound,
            Self::DocumentNotOpen(_) => AppErrorKind::DocumentNotOpen,
            Self::UnsavedChanges => AppErrorKind::UnsavedChanges,
            Self::DeleteUnsavedChanges => AppErrorKind::DeleteUnsavedChanges,
            Self::OperationInProgress => AppErrorKind::OperationInProgress,
            Self::ExternalChanges(_) => AppErrorKind::ExternalChanges,
            Self::BackgroundTaskFailed => AppErrorKind::BackgroundTaskFailed,
            Self::RevisionOverflow => AppErrorKind::RevisionOverflow,
            Self::Filesystem(_) => AppErrorKind::Filesystem,
            Self::Storage(_) => AppErrorKind::Storage,
        }
    }
}

impl AppErrorKind {
    pub(crate) fn diagnostic_name(self) -> &'static str {
        match self {
            Self::ExternalChanges => "error.save_conflict",
            Self::UnsavedChanges | Self::DeleteUnsavedChanges => "error.unsaved_changes",
            Self::Filesystem => "error.filesystem",
            Self::Storage => "error.storage",
            Self::WorkspaceNotOpen
            | Self::InvalidEntryName
            | Self::InvalidMove
            | Self::DestinationExists
            | Self::DocumentNotFound
            | Self::DocumentNotOpen
            | Self::OperationInProgress
            | Self::RevisionOverflow => "error.validation",
            Self::BackgroundTaskFailed => "error.filesystem",
        }
    }
}
