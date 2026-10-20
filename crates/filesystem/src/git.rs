//! Git CLI I/O adapter. Commands run only on application workers.
use crate::{FilesystemError, RealFilesystem, at};
use native_workspace_domain::DocumentId;
use serde::Serialize;
pub(super) mod operations;
pub use operations::{
    GitBranch, GitCommit, GitCommitDetail, GitCommitFile, GitConflict, GitInspection, GitOperation,
    GitQuery, GitRef, GitRefKind, GitRemote, GitStash, GitWorktree,
};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const MAX_OUTPUT: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitChange {
    /// Repository-relative, even when the workspace is a repository subfolder.
    pub path: String,
    pub original_path: Option<String>,
    pub index: String,
    pub worktree: String,
    pub staged: bool,
    pub unstaged: bool,
    pub conflicted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitRepository {
    pub root: PathBuf,
    pub branch: String,
    pub changes: Vec<GitChange>,
    pub head: String,
    pub branches: Vec<GitBranch>,
    pub remotes: Vec<GitRemote>,
    pub stashes: Vec<GitStash>,
    pub worktrees: Vec<GitWorktree>,
    pub ahead: u64,
    pub behind: u64,
    pub operation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitDiff {
    pub path: String,
    pub staged: bool,
    pub text: String,
    pub truncated: bool,
    pub before: Option<String>,
    pub after: Option<String>,
}

fn failure(root: &Path, message: impl Into<String>) -> FilesystemError {
    FilesystemError::Io {
        path: root.into(),
        source: io::Error::other(message.into()),
    }
}

fn command(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(["--no-pager", "--literal-pathspecs", "--no-optional-locks"])
        .arg("-C")
        .arg(root)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    // Inherited Git routing must not redirect inspection to another repository.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        cmd.env_remove(key);
    }
    cmd
}

fn output(root: &Path, args: &[&str]) -> Result<(Output, bool), FilesystemError> {
    let mut child = command(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            failure(
                root,
                format!("Could not run Git: {error}. Install Git and refresh."),
            )
        })?;
    let mut stdout = Vec::new();
    let result = child
        .stdout
        .take()
        .expect("piped stdout")
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut stdout);
    let truncated = stdout.len() as u64 > MAX_OUTPUT;
    if truncated || result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(at(root))?;
    result.map_err(at(root))?;
    stdout.truncate(MAX_OUTPUT as usize);
    Ok((
        Output {
            status,
            stdout,
            stderr: Vec::new(),
        },
        truncated,
    ))
}

fn checked(root: &Path, args: &[&str]) -> Result<Vec<u8>, FilesystemError> {
    let (out, truncated) = output(root, args)?;
    if truncated {
        return Err(failure(
            root,
            "Git output exceeds the inspection limit; narrow the workspace.",
        ));
    }
    if !out.status.success() {
        return Err(failure(
            root,
            "Git inspection failed. Check repository access and refresh.",
        ));
    }
    Ok(out.stdout)
}

pub(super) fn repository(root: &Path) -> Result<Option<GitRepository>, FilesystemError> {
    let (top, _) = output(root, &["rev-parse", "--show-toplevel"])?;
    if !top.status.success() {
        // Distinguish an ordinary folder from an inaccessible/broken repository.
        if root
            .ancestors()
            .any(|ancestor| ancestor.join(".git").exists())
        {
            return Err(failure(
                root,
                "Git repository is unavailable. Check access and refresh.",
            ));
        }
        return Ok(None);
    }
    let root_text = String::from_utf8(top.stdout)
        .map_err(|_| failure(root, "Git repository path is not UTF-8."))?;
    let repo_root = PathBuf::from(root_text.strip_suffix('\n').unwrap_or(&root_text));
    let bytes = checked(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
            "--",
            ".",
        ],
    )?;
    let changes = parse_status(root, &bytes)?;
    let (symbolic, _) = output(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let branch = if symbolic.status.success() {
        String::from_utf8_lossy(&symbolic.stdout)
            .trim_end_matches('\n')
            .into()
    } else {
        format!(
            "Detached HEAD ({})",
            String::from_utf8_lossy(&checked(root, &["rev-parse", "--short", "HEAD"])?).trim()
        )
    };
    operations::enrich(
        root,
        GitRepository {
            root: repo_root,
            branch,
            changes,
            head: String::new(),
            branches: Vec::new(),
            remotes: Vec::new(),
            stashes: Vec::new(),
            worktrees: Vec::new(),
            ahead: 0,
            behind: 0,
            operation: None,
        },
    )
    .map(Some)
}

fn parse_status(root: &Path, bytes: &[u8]) -> Result<Vec<GitChange>, FilesystemError> {
    let mut records = bytes.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            return Err(failure(root, "Invalid Git status response."));
        }
        let renamed = matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C');
        let original = if renamed {
            Some(
                records
                    .next()
                    .ok_or_else(|| failure(root, "Incomplete Git rename response."))?,
            )
        } else {
            None
        };
        // Like the workspace listing, unsupported non-UTF-8 names are skipped.
        let Ok(path) = std::str::from_utf8(&record[3..]) else {
            continue;
        };
        let original_path = original
            .and_then(|v| std::str::from_utf8(v).ok())
            .map(str::to_owned);
        if DocumentId::parse(path).is_err()
            || original_path
                .as_ref()
                .is_some_and(|p| DocumentId::parse(p).is_err())
        {
            continue;
        }
        changes.push(GitChange {
            path: path.into(),
            original_path,
            index: (record[0] as char).to_string(),
            worktree: (record[1] as char).to_string(),
            staged: !matches!(record[0], b' ' | b'?'),
            unstaged: record[1] != b' ',
            conflicted: matches!(
                &record[..2],
                b"DD" | b"AU" | b"UD" | b"UA" | b"DU" | b"AA" | b"UU"
            ),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

pub(super) fn diff(root: &Path, path: &str, staged: bool) -> Result<GitDiff, FilesystemError> {
    // Re-read status: callers cannot supply an arbitrary path or a vanished change.
    let repo = repository(root)?
        .ok_or_else(|| failure(root, "This workspace is not in a Git repository."))?;
    let change = repo
        .changes
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| {
            failure(
                root,
                "This Git change is no longer available. Refresh the changes list.",
            )
        })?;
    if (staged && !change.staged) || (!staged && !change.unstaged) {
        return Err(failure(
            root,
            "This Git change is no longer available. Refresh the changes list.",
        ));
    }
    let truncated;
    let text = if change.index == "?" {
        let id = DocumentId::parse(path).map_err(|_| failure(root, "Invalid Git path."))?;
        let location = RealFilesystem::document_path(&repo.root, &id)?;
        let file = std::fs::File::open(&location).map_err(at(&location))?;
        let mut bytes = Vec::new();
        file.take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map_err(at(&location))?;
        truncated = bytes.len() as u64 > MAX_OUTPUT;
        bytes.truncate(MAX_OUTPUT as usize);
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            "Binary or non-UTF-8 file; no text preview available.".into()
        } else {
            format!(
                "Untracked file: {path}\n\n{}",
                String::from_utf8_lossy(&bytes)
            )
        }
    } else {
        let mut args = vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--submodule=short",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ];
        if staged {
            args.push("--cached");
        }
        args.extend(["--", path]);
        if let Some(original) = &change.original_path {
            args.push(original);
        }
        let (out, limited) = output(&repo.root, &args)?;
        truncated = limited;
        if !limited && !out.status.success() {
            return Err(failure(root, "Git diff failed. Refresh the changes list."));
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let text = if text.is_empty() {
        "No text diff is available for this change.".into()
    } else {
        text
    };
    let before = if staged {
        blob(&repo.root, &format!("HEAD:{path}"))
    } else {
        blob(&repo.root, &format!(":{path}"))
    };
    let after = if staged {
        blob(&repo.root, &format!(":{path}"))
    } else {
        let id = DocumentId::parse(path).map_err(|_| failure(root, "Invalid Git path."))?;
        match RealFilesystem::document_path(&repo.root, &id) {
            Ok(location) => std::fs::File::open(&location).ok().and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).ok()?;
                if bytes.len() as u64 > MAX_OUTPUT || bytes.contains(&0) {
                    None
                } else {
                    String::from_utf8(bytes).ok()
                }
            }),
            Err(_) if !repo.root.join(path).exists() => Some(String::new()),
            Err(_) => None,
        }
    };
    Ok(GitDiff {
        before,
        after,
        path: path.into(),
        staged,
        text,
        truncated,
    })
}

fn blob(root: &Path, spec: &str) -> Option<String> {
    let (out, truncated) = output(
        root,
        &["show", "--no-ext-diff", "--no-textconv", "--no-color", spec],
    )
    .ok()?;
    if truncated {
        return None;
    }
    if !out.status.success() {
        return Some(String::new());
    }
    if out.stdout.contains(&0) {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}
