//! Typed Git I/O. No shell command supplied by a frontend is ever evaluated.
use super::{GitRepository, checked, command, failure, repository};
use crate::{FilesystemError, RealFilesystem, WorkspaceFilesystem};
use native_workspace_domain::DocumentId;
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitOperation {
    Initialize {
        branch: String,
    },
    Clone {
        url: String,
        directory: String,
    },
    Stage {
        path: Option<String>,
    },
    Unstage {
        path: Option<String>,
    },
    Discard {
        path: String,
    },
    Commit {
        message: String,
        amend: bool,
        signoff: bool,
    },
    Resolve {
        path: String,
        expected: String,
        result: String,
    },
    UndoCommit,
    CreateBranch {
        name: String,
        start: String,
    },
    Checkout {
        reference: String,
    },
    RenameBranch {
        name: String,
        new_name: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    Fetch {
        remote: String,
    },
    Pull {
        rebase: bool,
    },
    Push {
        force_with_lease: bool,
    },
    Publish {
        remote: String,
    },
    Sync,
    Merge {
        reference: String,
    },
    Rebase {
        reference: String,
    },
    CherryPick {
        commit: String,
    },
    Revert {
        commit: String,
    },
    Continue,
    Abort,
    StashSave {
        message: String,
        include_untracked: bool,
    },
    StashApply {
        index: usize,
        pop: bool,
    },
    StashDrop {
        index: usize,
    },
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote {
        name: String,
    },
    CreateTag {
        name: String,
        commit: String,
    },
    DeleteTag {
        name: String,
    },
    WorktreeAdd {
        directory: String,
        reference: String,
    },
    WorktreeRemove {
        directory: String,
    },
    Ignore {
        path: String,
    },
    Identity {
        name: String,
        email: String,
    },
    ApplyPatch {
        path: String,
        expected: String,
        patch: String,
        staged: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitQuery {
    Commit(String),
    Compare { from: String, to: String },
    FileHistory(String),
    Blame(String),
    Conflict(String),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitInspection {
    pub conflict: Option<GitConflict>,
    pub title: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitConflict {
    pub path: String,
    pub base: String,
    pub current: String,
    pub incoming: String,
    pub expected: String,
    pub accept_current: String,
    pub accept_incoming: String,
    pub accept_both: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitCommit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub refs: String,
    pub subject: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitBranch {
    pub name: String,
    pub id: String,
    pub upstream: String,
    pub current: bool,
    pub remote: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitRemote {
    pub name: String,
    pub url: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitStash {
    pub index: usize,
    pub subject: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitWorktree {
    pub path: String,
    pub branch: String,
}

fn text(root: &Path, args: &[&str]) -> Result<String, FilesystemError> {
    Ok(String::from_utf8_lossy(&checked(root, args)?).into_owned())
}
pub(crate) fn enrich(
    root: &Path,
    mut repo: GitRepository,
) -> Result<GitRepository, FilesystemError> {
    repo.head = text(root, &["rev-parse", "--verify", "HEAD"])
        .unwrap_or_default()
        .trim()
        .into();
    let branches = text(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)%00%(upstream:short)%00%(HEAD)%00%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    for line in branches.lines() {
        let fields: Vec<_> = line.split('\0').collect();
        if let [name, id, upstream, head, reference] = fields.as_slice() {
            repo.branches.push(GitBranch {
                name: (*name).into(),
                id: (*id).into(),
                upstream: (*upstream).into(),
                current: *head == "*",
                remote: reference.starts_with("refs/remotes/"),
            });
        }
    }
    if let Ok(counts) = text(
        root,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    ) {
        let mut words = counts.split_whitespace();
        repo.ahead = words.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        repo.behind = words.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    }
    for name in text(root, &["remote"])?.lines() {
        repo.remotes.push(GitRemote {
            name: name.into(),
            url: text(root, &["remote", "get-url", "--", name])?
                .trim()
                .into(),
        });
    }
    for (index, subject) in text(root, &["stash", "list", "--format=%s"])?
        .lines()
        .enumerate()
    {
        repo.stashes.push(GitStash {
            index,
            subject: subject.into(),
        });
    }
    let mut path = String::new();
    for line in text(root, &["worktree", "list", "--porcelain", "-z"])?.split('\0') {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = value.into();
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            repo.worktrees.push(GitWorktree {
                path: path.clone(),
                branch: branch.strip_prefix("refs/heads/").unwrap_or(branch).into(),
            });
            path.clear();
        } else if line == "detached" || line == "bare" {
            repo.worktrees.push(GitWorktree {
                path: path.clone(),
                branch: line.into(),
            });
            path.clear();
        }
    }
    for (file, name) in [
        ("MERGE_HEAD", "merge"),
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
    ] {
        let location = text(root, &["rev-parse", "--git-path", file])?;
        if root.join(location.trim_end_matches('\n')).exists() {
            repo.operation = Some(name.into());
            break;
        }
    }
    Ok(repo)
}

pub(crate) fn history(root: &Path, limit: usize) -> Result<Vec<GitCommit>, FilesystemError> {
    let repo = repository(root)?;
    if repo
        .as_ref()
        .is_none_or(|r| r.head.is_empty() && r.branches.is_empty())
    {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 5000).to_string();
    let out = text(
        root,
        &[
            "log",
            "--all",
            "--topo-order",
            "--date-order",
            "--max-count",
            &limit,
            "--format=%H%x00%P%x00%an%x00%aI%x00%D%x00%s%x00",
        ],
    )?;
    let fields: Vec<_> = out.split('\0').collect();
    Ok(fields
        .as_chunks::<6>()
        .0
        .iter()
        .map(|f| GitCommit {
            id: f[0].trim_start_matches('\n').into(),
            parents: f[1].split_whitespace().map(str::to_owned).collect(),
            author: f[2].into(),
            date: f[3].into(),
            refs: f[4].into(),
            subject: f[5].into(),
        })
        .collect())
}

fn reference(root: &Path, value: &str) -> Result<String, FilesystemError> {
    if value.is_empty() {
        return Err(failure(root, "Choose a branch or commit."));
    }
    Ok(text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{value}^{{commit}}"),
        ],
    )?
    .trim()
    .into())
}
fn safe_path(root: &Path, value: &str) -> Result<DocumentId, FilesystemError> {
    let id = DocumentId::parse(value)
        .map_err(|_| failure(root, "Choose a repository-relative file."))?;
    if value
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return Err(failure(
            root,
            "Git metadata cannot be edited from Source Control.",
        ));
    }
    Ok(id)
}
fn branch(root: &Path, value: &str) -> Result<(), FilesystemError> {
    if value.starts_with('-') || value == "HEAD" {
        return Err(failure(root, "Enter a valid branch name."));
    }
    checked(root, &["check-ref-format", "--branch", value]).map(|_| ())
}

/// Drain both streams concurrently and bound their retained text without stopping a mutation.
fn run(root: &Path, args: &[&str], input: Option<&str>) -> Result<(), FilesystemError> {
    let mut builder = command(root);
    builder
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        builder.process_group(0);
    }
    let mut child = builder
        .spawn()
        .map_err(|error| failure(root, format!("Could not run Git: {error}")))?;
    fn drain(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buffer = [0; 8192];
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let take = count.min(65536_usize.saturating_sub(kept.len()));
                kept.extend_from_slice(&buffer[..take]);
            }
            kept
        })
    }
    let stdout = drain(child.stdout.take().expect("piped stdout"));
    let stderr = drain(child.stderr.take().expect("piped stderr"));
    let write = input.map(|value| {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(value.as_bytes())
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| failure(root, e.to_string()))? {
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            // SAFETY: the unreaped child started its own process group. Kill only
            // that group, including helpers holding the captured output pipes.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(failure(
                root,
                "Git timed out. Check the repository and remote connection, then refresh.",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = stdout.join();
    let errors = stderr.join().unwrap_or_default();
    if !status.success() {
        let detail = String::from_utf8_lossy(&errors);
        return Err(failure(
            root,
            if detail.trim().is_empty() {
                "Git operation failed.".into()
            } else {
                detail.trim().to_owned()
            },
        ));
    }
    if let Some(Err(error)) = write {
        return Err(failure(root, error.to_string()));
    }
    Ok(())
}

pub(crate) fn execute(workspace: &Path, op: &GitOperation) -> Result<(), FilesystemError> {
    if let GitOperation::Initialize { branch: name } = op {
        branch(workspace, name)?;
        return run(
            workspace,
            &["init", "--initial-branch", name, "--", "."],
            None,
        );
    }
    if let GitOperation::Clone { url, directory } = op {
        if url.is_empty() || directory.is_empty() {
            return Err(failure(
                workspace,
                "Enter a repository URL and a new destination folder.",
            ));
        }
        return run(workspace, &["clone", "--", url, directory], None);
    }
    let repo = repository(workspace)?
        .ok_or_else(|| failure(workspace, "Initialize or open a Git repository first."))?;
    let root = &repo.root;
    let all = || {
        repo.changes
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>()
    };
    match op {
        GitOperation::Initialize { .. } | GitOperation::Clone { .. } => {
            unreachable!("handled before repository discovery")
        }
        GitOperation::Stage { path } => {
            let paths = if let Some(path) = path {
                safe_path(root, path)?;
                vec![path.as_str()]
            } else {
                all()
            };
            if paths.is_empty() {
                return Ok(());
            }
            for change in repo
                .changes
                .iter()
                .filter(|c| c.conflicted && paths.contains(&c.path.as_str()))
            {
                let id = safe_path(root, &change.path)?;
                let content = RealFilesystem.read_document(root, &id)?;
                if content.lines().any(|line| {
                    line.starts_with("<<<<<<< ")
                        || line == "======="
                        || line.starts_with(">>>>>>> ")
                }) {
                    return Err(failure(
                        root,
                        "Resolve conflict markers before staging this file.",
                    ));
                }
            }
            let mut args = vec!["add", "--all", "--"];
            args.extend(paths);
            run(root, &args, None)
        }
        GitOperation::Unstage { path } => {
            let paths = if let Some(path) = path {
                safe_path(root, path)?;
                vec![path.as_str()]
            } else {
                all()
            };
            if paths.is_empty() {
                return Ok(());
            }
            let mut args = if repo.head.is_empty() {
                vec!["rm", "--cached", "--ignore-unmatch", "--"]
            } else {
                vec!["restore", "--staged", "--"]
            };
            args.extend(paths);
            run(root, &args, None)
        }
        GitOperation::Discard { path } => {
            let id = safe_path(root, path)?;
            let change = repo
                .changes
                .iter()
                .find(|c| c.path == *path)
                .ok_or_else(|| {
                    failure(
                        root,
                        "This change no longer exists. Refresh Source Control.",
                    )
                })?;
            if change.index == "?" {
                RealFilesystem.delete_entry(root, &id)
            } else {
                run(root, &["restore", "--worktree", "--", path], None)
            }
        }
        GitOperation::Commit {
            message,
            amend,
            signoff,
        } => {
            if root != workspace {
                let full = checked(
                    root,
                    &["status", "--porcelain=v1", "-z", "--untracked-files=no"],
                )?;
                if super::parse_status(root, &full)?
                    .iter()
                    .any(|c| c.staged && !root.join(&c.path).starts_with(workspace))
                {
                    return Err(failure(
                        root,
                        "Open the repository root to commit files staged outside this workspace.",
                    ));
                }
            }
            if message.trim().is_empty() {
                return Err(failure(root, "Enter a commit message."));
            }
            let mut args = vec!["commit", "--message", message];
            if *amend {
                args.push("--amend");
            }
            if *signoff {
                args.push("--signoff");
            }
            run(root, &args, None)
        }
        GitOperation::Resolve {
            path,
            expected,
            result,
        } => {
            let id = safe_path(root, path)?;
            if !repo.changes.iter().any(|c| c.path == *path && c.conflicted)
                || RealFilesystem.read_document(root, &id)? != *expected
            {
                return Err(failure(
                    root,
                    "The conflict changed. Reopen the merge editor.",
                ));
            }
            if result.lines().any(|line| {
                line.starts_with("<<<<<<< ") || line == "=======" || line.starts_with(">>>>>>> ")
            }) {
                return Err(failure(
                    root,
                    "Resolve every conflict marker before saving and staging.",
                ));
            }
            RealFilesystem.write_document(root, &id, result)?;
            run(root, &["add", "--", path], None)
        }
        GitOperation::UndoCommit => run(root, &["reset", "--soft", "HEAD~1"], None),
        GitOperation::CreateBranch { name, start } => {
            branch(root, name)?;
            let oid = reference(root, if start.is_empty() { "HEAD" } else { start })?;
            run(root, &["switch", "--create", name, &oid], None)
        }
        GitOperation::Checkout { reference: value } => {
            if let Some(b) = repo.branches.iter().find(|b| b.name == *value) {
                if b.remote {
                    let name = b
                        .name
                        .split_once('/')
                        .map(|(_, name)| name)
                        .unwrap_or(&b.name);
                    branch(root, name)?;
                    run(
                        root,
                        &["switch", "--track", "--create", name, &b.name],
                        None,
                    )
                } else {
                    run(root, &["switch", "--", &b.name], None)
                }
            } else {
                let oid = reference(root, value)?;
                run(root, &["switch", "--detach", &oid], None)
            }
        }
        GitOperation::RenameBranch { name, new_name } => {
            branch(root, name)?;
            branch(root, new_name)?;
            run(root, &["branch", "--move", name, new_name], None)
        }
        GitOperation::DeleteBranch { name, force } => {
            branch(root, name)?;
            run(
                root,
                &["branch", if *force { "-D" } else { "--delete" }, "--", name],
                None,
            )
        }
        GitOperation::Fetch { remote } => {
            if remote.is_empty() {
                run(root, &["fetch", "--all", "--prune"], None)
            } else {
                remote_exists(root, &repo, remote)?;
                run(root, &["fetch", "--prune", "--", remote], None)
            }
        }
        GitOperation::Pull { rebase } => run(
            root,
            &["pull", if *rebase { "--rebase" } else { "--no-rebase" }],
            None,
        ),
        GitOperation::Push { force_with_lease } => run(
            root,
            if *force_with_lease {
                &["push", "--force-with-lease"]
            } else {
                &["push"]
            },
            None,
        ),
        GitOperation::Publish { remote } => {
            remote_exists(root, &repo, remote)?;
            branch(root, &repo.branch)?;
            run(
                root,
                &["push", "--set-upstream", "--", remote, &repo.branch],
                None,
            )
        }
        GitOperation::Sync => {
            run(root, &["pull", "--no-rebase"], None)?;
            run(root, &["push"], None)
        }
        GitOperation::Merge { reference: value } => {
            let oid = reference(root, value)?;
            let source = if repo.branches.iter().any(|branch| branch.name == *value) {
                value.as_str()
            } else {
                oid.as_str()
            };
            run(root, &["merge", "--no-edit", "--", source], None)
        }
        GitOperation::Rebase { reference: value } => {
            let oid = reference(root, value)?;
            run(root, &["rebase", &oid], None)
        }
        GitOperation::CherryPick { commit } => {
            let oid = reference(root, commit)?;
            run(root, &["cherry-pick", &oid], None)
        }
        GitOperation::Revert { commit } => {
            let oid = reference(root, commit)?;
            run(root, &["revert", "--no-edit", &oid], None)
        }
        GitOperation::Continue | GitOperation::Abort => {
            let kind = repo
                .operation
                .as_deref()
                .ok_or_else(|| failure(root, "No merge, rebase or cherry-pick is in progress."))?;
            run(
                root,
                &[
                    "-c",
                    "core.editor=true",
                    "-c",
                    "sequence.editor=true",
                    kind,
                    if matches!(op, GitOperation::Continue) {
                        "--continue"
                    } else {
                        "--abort"
                    },
                ],
                None,
            )
        }
        GitOperation::StashSave {
            message,
            include_untracked,
        } => {
            let mut args = vec!["stash", "push", "--message", message];
            if *include_untracked {
                args.push("--include-untracked");
            }
            run(root, &args, None)
        }
        GitOperation::StashApply { index, pop } => {
            stash_exists(root, &repo, *index)?;
            run(
                root,
                &[
                    "stash",
                    if *pop { "pop" } else { "apply" },
                    &format!("stash@{{{index}}}"),
                ],
                None,
            )
        }
        GitOperation::StashDrop { index } => {
            stash_exists(root, &repo, *index)?;
            run(
                root,
                &["stash", "drop", &format!("stash@{{{index}}}")],
                None,
            )
        }
        GitOperation::AddRemote { name, url } => {
            if name.is_empty() || name.starts_with('-') || url.is_empty() {
                return Err(failure(root, "Enter a remote name and URL."));
            }
            run(root, &["remote", "add", "--", name, url], None)
        }
        GitOperation::RemoveRemote { name } => {
            remote_exists(root, &repo, name)?;
            run(root, &["remote", "remove", "--", name], None)
        }
        GitOperation::CreateTag { name, commit } => {
            checked(root, &["check-ref-format", &format!("refs/tags/{name}")])?;
            let oid = reference(root, if commit.is_empty() { "HEAD" } else { commit })?;
            run(root, &["tag", "--", name, &oid], None)
        }
        GitOperation::DeleteTag { name } => {
            checked(root, &["check-ref-format", &format!("refs/tags/{name}")])?;
            run(root, &["tag", "--delete", "--", name], None)
        }
        GitOperation::WorktreeAdd {
            directory,
            reference: value,
        } => {
            let oid = reference(root, value)?;
            if repo
                .branches
                .iter()
                .any(|branch| !branch.remote && branch.name == *value)
            {
                run(root, &["worktree", "add", "--", directory, value], None)
            } else {
                run(
                    root,
                    &["worktree", "add", "--detach", "--", directory, &oid],
                    None,
                )
            }
        }
        GitOperation::WorktreeRemove { directory } => {
            let chosen =
                std::fs::canonicalize(directory).map_err(|e| failure(root, e.to_string()))?;
            if !repo
                .worktrees
                .iter()
                .any(|w| Path::new(&w.path) == chosen && chosen != *root)
            {
                return Err(failure(
                    root,
                    "Choose another worktree from the repository.",
                ));
            }
            run(root, &["worktree", "remove", "--", directory], None)
        }
        GitOperation::Ignore { path } => {
            safe_path(root, path)?;
            let id = DocumentId::parse(".gitignore").expect("fixed path");
            let location = root.join(".gitignore");
            let mut content = if location.exists() {
                RealFilesystem.read_document(root, &id)?
            } else {
                String::new()
            };
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('/');
            for c in path.chars() {
                if matches!(c, '\\' | '*' | '?' | '[' | ']' | '#' | '!' | ' ') {
                    content.push('\\');
                }
                if c == '\n' || c == '\r' {
                    return Err(failure(
                        root,
                        "Newlines in ignore patterns are unsupported.",
                    ));
                }
                content.push(c);
            }
            content.push('\n');
            RealFilesystem.write_document(root, &id, &content)
        }
        GitOperation::Identity { name, email } => {
            if name.trim().is_empty() || email.trim().is_empty() {
                return Err(failure(root, "Enter your author name and email."));
            }
            run(root, &["config", "--local", "user.name", name], None)?;
            run(root, &["config", "--local", "user.email", email], None)
        }
        GitOperation::ApplyPatch {
            path,
            expected,
            patch,
            staged,
        } => {
            safe_path(root, path)?;
            let current = super::diff(workspace, path, *staged)?;
            if current.truncated || current.text != *expected {
                return Err(failure(
                    root,
                    "The diff changed. Refresh before staging selected lines.",
                ));
            }
            run(
                root,
                &[
                    "apply",
                    "--cached",
                    "--unidiff-zero",
                    "--whitespace=nowarn",
                    "-",
                ],
                Some(patch),
            )
        }
    }
}

fn remote_exists(root: &Path, repo: &GitRepository, name: &str) -> Result<(), FilesystemError> {
    if repo.remotes.iter().any(|r| r.name == name) {
        Ok(())
    } else {
        Err(failure(root, "Choose a configured remote."))
    }
}
fn stash_exists(root: &Path, repo: &GitRepository, index: usize) -> Result<(), FilesystemError> {
    if repo.stashes.iter().any(|s| s.index == index) {
        Ok(())
    } else {
        Err(failure(
            root,
            "This stash no longer exists. Refresh Source Control.",
        ))
    }
}
pub(crate) fn inspect(
    workspace: &Path,
    query: &GitQuery,
) -> Result<GitInspection, FilesystemError> {
    let repo =
        repository(workspace)?.ok_or_else(|| failure(workspace, "Open a Git repository first."))?;
    let root = &repo.root;
    let options = ["--no-ext-diff", "--no-textconv", "--no-color"];
    let (title, args): (String, Vec<String>) = match query {
        GitQuery::Conflict(path) => {
            let id = safe_path(root, path)?;
            if !repo.changes.iter().any(|c| c.path == *path && c.conflicted) {
                return Err(failure(root, "This file has no unresolved conflict."));
            }
            let base = text(root, &["show", &format!(":1:{path}")]).unwrap_or_default();
            let current = text(root, &["show", &format!(":2:{path}")]).unwrap_or_default();
            let incoming = text(root, &["show", &format!(":3:{path}")]).unwrap_or_default();
            let expected = RealFilesystem.read_document(root, &id)?;
            return Ok(GitInspection {
                title: format!("Resolve: {path}"),
                text: expected.clone(),
                truncated: false,
                conflict: Some(GitConflict {
                    path: path.clone(),
                    base,
                    current,
                    incoming,
                    expected,
                    accept_current: String::new(),
                    accept_incoming: String::new(),
                    accept_both: String::new(),
                }),
            });
        }
        GitQuery::Commit(value) => {
            let oid = reference(root, value)?;
            (
                format!("Commit {oid}"),
                vec![
                    "show".into(),
                    "--format=fuller".into(),
                    "--stat".into(),
                    "--patch".into(),
                    oid,
                ],
            )
        }
        GitQuery::Compare { from, to } => {
            let a = reference(root, from)?;
            let b = reference(root, to)?;
            (format!("{from} → {to}"), vec!["diff".into(), a, b])
        }
        GitQuery::FileHistory(path) => {
            safe_path(root, path)?;
            (
                format!("History: {path}"),
                vec![
                    "log".into(),
                    "--follow".into(),
                    "--max-count=100".into(),
                    "--format=fuller".into(),
                    "--patch".into(),
                    "--".into(),
                    path.into(),
                ],
            )
        }
        GitQuery::Blame(path) => {
            safe_path(root, path)?;
            return Ok(GitInspection {
                conflict: None,
                title: format!("Blame: {path}"),
                text: text(root, &["blame", "--date=short", "--", path])?,
                truncated: false,
            });
        }
    };
    let mut arguments = vec![args[0].as_str()];
    arguments.extend(options);
    arguments.extend(args[1..].iter().map(String::as_str));
    let (out, truncated) = super::output(root, &arguments)?;
    if !truncated && !out.status.success() {
        return Err(failure(
            root,
            "Git history inspection failed. Refresh and try again.",
        ));
    }
    Ok(GitInspection {
        conflict: None,
        title,
        text: String::from_utf8_lossy(&out.stdout).into_owned(),
        truncated,
    })
}
