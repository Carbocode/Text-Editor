//! Immutable Git inspection view; document state and unsaved buffers are independent.
pub use native_workspace_filesystem::{
    GitChange, GitCommit, GitDiff, GitInspection, GitOperation, GitQuery, GitRepository,
};
use serde::Serialize;
pub(crate) mod patch;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitSnapshot {
    pub loaded: bool,
    pub roots: Vec<std::path::PathBuf>,
    pub selected_root: Option<std::path::PathBuf>,
    pub repository: Option<GitRepository>,
    pub diff: Option<GitDiff>,
    pub inspection: Option<GitInspection>,
    pub history: Vec<GraphCommit>,
    pub history_limit: usize,
    pub actions: Vec<GitAction>,
    pub patch_lines: Vec<patch::PatchLine>,
}

impl Default for GitSnapshot {
    fn default() -> Self {
        Self {
            loaded: false,
            roots: Vec::new(),
            selected_root: None,
            repository: None,
            diff: None,
            inspection: None,
            history: Vec::new(),
            history_limit: 200,
            actions: actions(),
            patch_lines: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub start: u8,
    pub end: u8,
    pub color: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GraphCommit {
    pub commit: GitCommit,
    pub lane: usize,
    pub columns: usize,
    pub edges: Vec<GraphEdge>,
}

pub(crate) fn graph(commits: Vec<GitCommit>) -> Vec<GraphCommit> {
    let mut lanes: Vec<String> = Vec::new();
    let mut result = Vec::new();
    for commit in commits {
        let lane = lanes
            .iter()
            .position(|id| *id == commit.id)
            .unwrap_or_else(|| {
                lanes.push(commit.id.clone());
                lanes.len() - 1
            });
        let before = lanes.clone();
        lanes.remove(lane);
        for (offset, parent) in commit.parents.iter().enumerate() {
            if !lanes.contains(parent) {
                lanes.insert((lane + offset).min(lanes.len()), parent.clone());
            }
        }
        let mut edges = Vec::new();
        for (column, id) in before.iter().enumerate() {
            if *id == commit.id {
                edges.push(GraphEdge {
                    from: column,
                    to: lane,
                    start: 0,
                    end: 1,
                    color: lane,
                });
            } else if let Some(to) = lanes.iter().position(|next| next == id) {
                edges.push(GraphEdge {
                    from: column,
                    to,
                    start: 0,
                    end: 2,
                    color: column,
                });
            }
        }
        for parent in &commit.parents {
            if let Some(to) = lanes.iter().position(|next| next == parent) {
                edges.push(GraphEdge {
                    from: lane,
                    to,
                    start: 1,
                    end: 2,
                    color: to,
                });
            }
        }
        result.push(GraphCommit {
            commit,
            lane,
            columns: before.len().max(lanes.len()),
            edges,
        });
    }
    result
}

/// Shared forms used by three native adapters; dispatch remains a typed intention.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitAction {
    pub id: &'static str,
    pub title: &'static str,
    pub first: &'static str,
    pub second: &'static str,
    pub option: &'static str,
    pub destructive: bool,
}
fn actions() -> Vec<GitAction> {
    [
        (
            "init",
            "Initialize Repository",
            "Initial branch",
            "",
            "",
            false,
        ),
        (
            "clone",
            "Clone Repository",
            "Repository URL",
            "New destination directory",
            "",
            false,
        ),
        (
            "identity",
            "Configure Author",
            "Author name",
            "Author email",
            "",
            false,
        ),
        (
            "branch",
            "Create Branch",
            "New branch name",
            "Start from (empty = HEAD)",
            "",
            false,
        ),
        (
            "checkout",
            "Switch Branch / Checkout Commit",
            "Branch or commit",
            "",
            "",
            false,
        ),
        (
            "rename_branch",
            "Rename Branch",
            "Existing branch",
            "New branch name",
            "",
            false,
        ),
        (
            "delete_branch",
            "Delete Branch",
            "Branch name",
            "",
            "Force deletion",
            true,
        ),
        ("fetch", "Fetch", "Remote (empty = all)", "", "", false),
        ("pull", "Pull", "", "", "Rebase instead of merge", false),
        ("push", "Push", "", "", "Force with lease", false),
        ("publish", "Publish Branch", "Remote", "", "", false),
        ("sync", "Sync (Pull then Push)", "", "", "", false),
        ("merge", "Merge Branch", "Branch or commit", "", "", false),
        (
            "rebase",
            "Rebase onto Branch",
            "Branch or commit",
            "",
            "",
            false,
        ),
        ("cherry_pick", "Cherry-pick Commit", "Commit", "", "", false),
        ("revert", "Revert Commit", "Commit", "", "", false),
        (
            "undo_commit",
            "Undo Last Commit (Keep Changes Staged)",
            "",
            "",
            "",
            true,
        ),
        (
            "stash",
            "Stash Changes",
            "Message",
            "",
            "Include untracked files",
            false,
        ),
        (
            "stash_apply",
            "Apply / Pop Stash",
            "Stash number",
            "",
            "Pop after applying",
            false,
        ),
        ("stash_drop", "Delete Stash", "Stash number", "", "", true),
        (
            "remote_add",
            "Add Remote",
            "Remote name",
            "Remote URL",
            "",
            false,
        ),
        (
            "remote_remove",
            "Remove Remote",
            "Remote name",
            "",
            "",
            true,
        ),
        (
            "tag",
            "Create Tag",
            "Tag name",
            "Commit (empty = HEAD)",
            "",
            false,
        ),
        ("tag_delete", "Delete Tag", "Tag name", "", "", true),
        (
            "worktree_add",
            "Create Worktree",
            "New directory",
            "Branch or commit",
            "",
            false,
        ),
        (
            "worktree_remove",
            "Remove Worktree",
            "Worktree directory",
            "",
            "",
            true,
        ),
        (
            "compare",
            "Compare Commits / Branches",
            "From",
            "To",
            "",
            false,
        ),
        (
            "file_history",
            "File History",
            "Repository-relative path",
            "",
            "",
            false,
        ),
        (
            "blame",
            "File Blame",
            "Repository-relative path",
            "",
            "",
            false,
        ),
    ]
    .into_iter()
    .map(
        |(id, title, first, second, option, destructive)| GitAction {
            id,
            title,
            first,
            second,
            option,
            destructive,
        },
    )
    .collect()
}

pub fn git_request(
    action: &str,
    value: String,
    extra: String,
    flag: bool,
) -> Result<crate::AppCommand, crate::AppError> {
    use crate::AppCommand as C;
    use GitOperation as O;
    let invalid = || crate::AppError::Git("Enter valid Git action parameters.".into());
    let op = match action {
        "select_repository" => return Ok(C::SelectGitRepository(value.into())),
        "init" => O::Initialize {
            branch: if value.is_empty() {
                "main".into()
            } else {
                value
            },
        },
        "clone" => O::Clone {
            url: value,
            directory: extra,
        },
        "identity" => O::Identity {
            name: value,
            email: extra,
        },
        "stage" => O::Stage {
            path: (!value.is_empty()).then_some(value),
        },
        "unstage" => O::Unstage {
            path: (!value.is_empty()).then_some(value),
        },
        "discard" => O::Discard { path: value },
        "ignore" => O::Ignore { path: value },
        "commit" => O::Commit {
            message: value,
            amend: flag,
            signoff: extra == "signoff",
        },
        "undo_commit" => O::UndoCommit,
        "branch" => O::CreateBranch {
            name: value,
            start: extra,
        },
        "checkout" => O::Checkout { reference: value },
        "rename_branch" => O::RenameBranch {
            name: value,
            new_name: extra,
        },
        "delete_branch" => O::DeleteBranch {
            name: value,
            force: flag,
        },
        "fetch" => O::Fetch { remote: value },
        "pull" => O::Pull { rebase: flag },
        "push" => O::Push {
            force_with_lease: flag,
        },
        "publish" => O::Publish { remote: value },
        "sync" => O::Sync,
        "merge" => O::Merge { reference: value },
        "rebase" => O::Rebase { reference: value },
        "cherry_pick" => O::CherryPick { commit: value },
        "revert" => O::Revert { commit: value },
        "continue" => O::Continue,
        "abort" => O::Abort,
        "stash" => O::StashSave {
            message: value,
            include_untracked: flag,
        },
        "stash_apply" => O::StashApply {
            index: value.parse().map_err(|_| invalid())?,
            pop: flag,
        },
        "stash_drop" => O::StashDrop {
            index: value.parse().map_err(|_| invalid())?,
        },
        "remote_add" => O::AddRemote {
            name: value,
            url: extra,
        },
        "remote_remove" => O::RemoveRemote { name: value },
        "tag" => O::CreateTag {
            name: value,
            commit: extra,
        },
        "tag_delete" => O::DeleteTag { name: value },
        "worktree_add" => O::WorktreeAdd {
            directory: value,
            reference: extra,
        },
        "worktree_remove" => O::WorktreeRemove { directory: value },
        "show_commit" => return Ok(C::InspectGit(GitQuery::Commit(value))),
        "compare" => {
            return Ok(C::InspectGit(GitQuery::Compare {
                from: value,
                to: extra,
            }));
        }
        "file_history" => return Ok(C::InspectGit(GitQuery::FileHistory(value))),
        "blame" => return Ok(C::InspectGit(GitQuery::Blame(value))),
        "resolve" => return Ok(C::InspectGit(GitQuery::Conflict(value))),
        _ => return Err(invalid()),
    };
    Ok(C::Git(op))
}

pub fn git_action_confirmation() -> crate::Confirmation {
    crate::Confirmation {
        title: "Apply this Git operation?".into(),
        body: "Discarding removes unsaved disk changes. Branch, stash and history operations can remove references or rewrite local history. Review the selected operation before continuing.",
        confirm: "Apply",
        cancel: "Cancel",
        discard: "",
    }
}

impl GitSnapshot {
    /// Shared bridge representation; unchanged snapshots need not cross FFI again.
    pub fn json(&self) -> String {
        serde_json::to_string(self).expect("Git snapshot serializes")
    }
}

pub(crate) fn requires_saved_buffers(operation: &GitOperation) -> bool {
    use GitOperation as O;
    match operation {
        O::Resolve { .. }
        | O::Clone { .. }
        | O::Stage { .. }
        | O::Discard { .. }
        | O::CreateBranch { .. }
        | O::Checkout { .. }
        | O::Pull { .. }
        | O::Sync
        | O::Merge { .. }
        | O::Rebase { .. }
        | O::CherryPick { .. }
        | O::Revert { .. }
        | O::Continue
        | O::Abort
        | O::StashSave { .. }
        | O::StashApply { .. }
        | O::WorktreeRemove { .. }
        | O::Ignore { .. }
        | O::ApplyPatch { .. } => true,
        O::Initialize { .. }
        | O::Unstage { .. }
        | O::Commit { .. }
        | O::UndoCommit
        | O::RenameBranch { .. }
        | O::DeleteBranch { .. }
        | O::Fetch { .. }
        | O::Push { .. }
        | O::Publish { .. }
        | O::StashDrop { .. }
        | O::AddRemote { .. }
        | O::RemoveRemote { .. }
        | O::CreateTag { .. }
        | O::DeleteTag { .. }
        | O::WorktreeAdd { .. }
        | O::Identity { .. } => false,
    }
}

/// Resolve marker blocks while retaining surrounding lines, including diff3 bases.
pub(crate) fn conflict_choices(text: &str) -> (String, String, String) {
    let mut current = String::new();
    let mut incoming = String::new();
    let mut both = String::new();
    let mut state = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("<<<<<<< ") {
            state = 1;
            continue;
        }
        if state == 1 && line.starts_with("||||||| ") {
            state = 2;
            continue;
        }
        if state != 0 && line.trim_end() == "=======" {
            state = 3;
            continue;
        }
        if state != 0 && line.starts_with(">>>>>>> ") {
            state = 0;
            continue;
        }
        match state {
            0 => {
                current.push_str(line);
                incoming.push_str(line);
                both.push_str(line);
            }
            1 => {
                current.push_str(line);
                both.push_str(line);
            }
            3 => {
                incoming.push_str(line);
                both.push_str(line);
            }
            _ => {}
        }
    }
    (current, incoming, both)
}
