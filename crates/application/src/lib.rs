//! Shared application core: commands → events → state → snapshots.
//!
//! Layers, in dependency order:
//! - `command`, `event`, `error`, `snapshot`: the public vocabulary;
//! - `state`: authoritative state and its reducer, the only mutation;
//! - `handlers`: one module per area, resolving commands into events;
//! - `Application`: the synchronous pipeline;
//! - [`Session`]: the frontend-facing runtime and its threading policy;
//! - [`sidebar`], `icons`: presentation shared by every frontend.

mod application;
mod command;
mod diagnostics;
mod drop_diagnostics;
mod error;
mod event;
mod git;
mod handlers;
pub use git::{
    GitAction, GitChange, GitCommit, GitCommitDetail, GitCommitFile, GitDiff, GitInspection,
    GitOperation, GitQuery, GitRef, GitRefKind, GitRepository, GitSnapshot, GraphCommit, PatchKind,
    PatchLine, git_action_confirmation, git_request,
};
mod icons;
mod path_filter;
mod presentation;
mod replacement;
mod search;
mod search_options;
mod session;
pub mod sidebar;
mod snapshot;
mod state;
mod terminal;
mod worker;
pub use terminal::{
    TerminalCell, TerminalCommand, TerminalFont, TerminalFrame, TerminalInput, TerminalKey,
    TerminalSnapshot, TerminalTab, terminal_close_confirmation, terminal_font,
};

#[cfg(test)]
mod tests;

pub(crate) use application::Application;
pub use command::AppCommand;
pub use diagnostics::DiagnosticObserver;
pub use drop_diagnostics::SidebarDropTrace;
pub use error::{AppError, AppErrorKind};
pub use event::AppEvent;
pub use icons::{file_icon_name, folder_icon_name};
pub use native_workspace_storage::RecentWorkspace;
pub use path_filter::PathFilterResult;
pub use presentation::{
    Confirmation, close_confirmation, delete_confirmation, quit_confirmation,
    replacement_confirmation,
};
pub use replacement::{ReplacementPreview, ReplacementTarget};
pub use search_options::SearchOptions;
pub use session::Session;
pub use sidebar::{SidebarChild, SidebarRow, SidebarRowKind};
pub use snapshot::{
    Activity, AppSnapshot, DocumentSnapshot, DocumentSummary, Relocation, SearchLocation,
    SearchResult,
};
pub(crate) use state::AppState;
pub use state::DocumentEntry;
