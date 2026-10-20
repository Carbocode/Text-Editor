use super::*;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn repository() -> tempfile::TempDir {
    let dir = workspace(&[
        ("file.txt", "original\n"),
        ("removed.txt", "deleted\n"),
        ("sub/note.txt", "note\n"),
    ]);
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Initial"]);
    dir
}

fn session(root: &Path) -> Session {
    let mut session = Session::new();
    session
        .dispatch(AppCommand::OpenWorkspace(root.into()))
        .unwrap();
    session
}

fn refresh(session: &mut Session) -> AppSnapshot {
    session.dispatch(AppCommand::RefreshGit).unwrap();
    settle(session)
}

#[test]
fn git_distinguishes_index_worktree_and_untracked_without_writing() {
    let dir = repository();
    let root = dir.path();
    fs::write(root.join("file.txt"), "staged\n").unwrap();
    git(root, &["add", "file.txt"]);
    fs::write(root.join("file.txt"), "unstaged\n").unwrap();
    fs::remove_file(root.join("removed.txt")).unwrap();
    fs::write(root.join("new space\nü.txt"), "new\n").unwrap();
    let before = fs::read(root.join(".git/index")).unwrap();
    let mut app = session(root);
    let view = refresh(&mut app);
    let repo = view.git.repository.as_ref().unwrap();
    let changed = repo.changes.iter().find(|c| c.path == "file.txt").unwrap();
    assert!(changed.staged && changed.unstaged);
    assert_eq!((&*changed.index, &*changed.worktree), ("M", "M"));
    assert!(
        repo.changes
            .iter()
            .any(|c| c.path == "removed.txt" && c.worktree == "D")
    );
    assert!(
        repo.changes
            .iter()
            .any(|c| c.path == "new space\nü.txt" && !c.staged && c.unstaged)
    );
    for (staged, removed, added) in [
        (true, "-original", "+staged"),
        (false, "-staged", "+unstaged"),
    ] {
        app.dispatch(AppCommand::ShowGitDiff {
            path: "file.txt".into(),
            staged,
        })
        .unwrap();
        let view = settle(&mut app);
        let text = &view.git.diff.as_ref().unwrap().text;
        assert!(text.contains(removed) && text.contains(added), "{text}");
    }
    app.dispatch(AppCommand::ShowGitDiff {
        path: "new space\nü.txt".into(),
        staged: false,
    })
    .unwrap();
    assert!(
        settle(&mut app)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("new\n")
    );
    assert_eq!(
        before,
        fs::read(root.join(".git/index")).unwrap(),
        "inspection wrote the index"
    );
    assert_eq!(
        fs::read_to_string(root.join("file.txt")).unwrap(),
        "unstaged\n"
    );
}

#[test]
fn git_handles_unborn_clean_non_repository_and_workspace_changes() {
    let dir = workspace(&[("new.txt", "new")]);
    let mut app = session(dir.path());
    let view = refresh(&mut app);
    assert!(view.git.loaded && view.git.repository.is_none());
    git(dir.path(), &["init", "-q"]);
    let view = refresh(&mut app);
    assert_eq!(view.git.repository.as_ref().unwrap().changes.len(), 1);
    git(dir.path(), &["add", "."]);
    let view = refresh(&mut app);
    assert!(view.git.repository.as_ref().unwrap().changes[0].staged);
    app.dispatch(AppCommand::ShowGitDiff {
        path: "new.txt".into(),
        staged: true,
    })
    .unwrap();
    assert!(
        settle(&mut app)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("+new")
    );
    git(dir.path(), &["commit", "-qm", "Initial"]);
    assert!(
        refresh(&mut app)
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .is_empty()
    );
    let other = workspace(&[("other", "other")]);
    app.dispatch(AppCommand::OpenWorkspace(other.path().into()))
        .unwrap();
    assert!(!app.snapshot().git.loaded);
    assert!(app.snapshot().git.diff.is_none());
}

#[test]
fn git_supports_subfolder_workspaces_renames_and_literal_pathspecs() {
    let dir = repository();
    let root = dir.path();
    git(root, &["mv", "file.txt", "renamed.txt"]);
    fs::write(root.join("sub/note.txt"), "subfolder edit\n").unwrap();
    fs::write(root.join("sub/:(glob)*.txt"), "literal\n").unwrap();
    let mut app = session(root);
    let view = refresh(&mut app);
    let change = view
        .git
        .repository
        .as_ref()
        .unwrap()
        .changes
        .iter()
        .find(|c| c.path == "renamed.txt")
        .unwrap();
    assert_eq!(change.original_path.as_deref(), Some("file.txt"));
    app.dispatch(AppCommand::ShowGitDiff {
        path: "renamed.txt".into(),
        staged: true,
    })
    .unwrap();
    assert!(
        settle(&mut app)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("renamed.txt")
    );
    let mut nested = session(&root.join("sub"));
    let view = refresh(&mut nested);
    let changes = &view.git.repository.as_ref().unwrap().changes;
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| c.path.starts_with("sub/")));
    nested
        .dispatch(AppCommand::ShowGitDiff {
            path: "sub/note.txt".into(),
            staged: false,
        })
        .unwrap();
    assert!(
        settle(&mut nested)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("+subfolder edit")
    );
    nested
        .dispatch(AppCommand::ShowGitDiff {
            path: "sub/:(glob)*.txt".into(),
            staged: false,
        })
        .unwrap();
    assert!(
        settle(&mut nested)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("literal")
    );
    nested
        .dispatch(AppCommand::ShowGitDiff {
            path: "renamed.txt".into(),
            staged: true,
        })
        .unwrap();
    assert!(
        settle(&mut nested).error.is_some(),
        "diff escaped the workspace scope"
    );
}

#[test]
fn git_preserves_edits_and_queued_saves_during_inspection() {
    let dir = repository();
    let mut app = session(dir.path());
    app.dispatch(AppCommand::OpenDocument(id("file.txt")))
        .unwrap();
    app.delay_next_background_job(Duration::from_millis(80));
    app.dispatch(AppCommand::RefreshGit).unwrap();
    assert!(app.snapshot().activity.busy && !app.snapshot().activity.editing_paused);
    app.dispatch(AppCommand::ChangeDocument {
        id: id("file.txt"),
        content: "typed while inspecting\n".into(),
    })
    .unwrap();
    assert_eq!(
        &*app.snapshot().active_document.unwrap().content,
        "typed while inspecting\n"
    );
    app.dispatch(AppCommand::SaveDocument(id("file.txt")))
        .unwrap();
    let view = settle(&mut app);
    assert!(view.error.is_none(), "{:?}", view.error);
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "typed while inspecting\n"
    );
    assert!(!view.has_unsaved_documents());
    assert!(
        refresh(&mut app)
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "file.txt")
    );
}

#[test]
fn git_rejects_stale_and_unsafe_paths_and_limits_large_previews() {
    let dir = repository();
    fs::write(dir.path().join("file.txt"), "changed\n").unwrap();
    let mut app = session(dir.path());
    refresh(&mut app);
    git(dir.path(), &["restore", "file.txt"]);
    for path in ["file.txt", "../outside", "/etc/passwd"] {
        app.dispatch(AppCommand::ShowGitDiff {
            path: path.into(),
            staged: false,
        })
        .unwrap();
        assert!(settle(&mut app).error.is_some());
    }
    fs::write(dir.path().join("large.txt"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
    fs::write(dir.path().join("binary"), [0, 1, 2, 255]).unwrap();
    refresh(&mut app);
    app.dispatch(AppCommand::ShowGitDiff {
        path: "large.txt".into(),
        staged: false,
    })
    .unwrap();
    assert!(settle(&mut app).git.diff.as_ref().unwrap().truncated);
    app.dispatch(AppCommand::ShowGitDiff {
        path: "binary".into(),
        staged: false,
    })
    .unwrap();
    assert!(
        settle(&mut app)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("Binary")
    );
}

#[test]
fn git_supports_linked_worktrees_detached_head_and_ignored_files() {
    let dir = repository();
    let linked = tempfile::tempdir().unwrap();
    let checkout = linked.path().join("checkout");
    git(
        dir.path(),
        &["worktree", "add", "--detach", checkout.to_str().unwrap()],
    );
    fs::write(checkout.join("file.txt"), "linked edit\n").unwrap();
    fs::write(checkout.join(".gitignore"), "ignored\n").unwrap();
    fs::write(checkout.join("ignored"), "hidden").unwrap();
    let mut app = session(&checkout);
    let view = refresh(&mut app);
    let repo = view.git.repository.as_ref().unwrap();
    assert!(repo.branch.starts_with("Detached HEAD"));
    assert!(repo.changes.iter().any(|c| c.path == "file.txt"));
    assert!(!repo.changes.iter().any(|c| c.path == "ignored"));
    app.dispatch(AppCommand::ShowGitDiff {
        path: "file.txt".into(),
        staged: false,
    })
    .unwrap();
    assert!(
        settle(&mut app)
            .git
            .diff
            .as_ref()
            .unwrap()
            .text
            .contains("+linked edit")
    );
}

#[cfg(unix)]
#[test]
fn git_does_not_follow_untracked_symlinks_or_external_diff_drivers() {
    let dir = repository();
    let external = workspace(&[("secret", "outside workspace")]);
    std::os::unix::fs::symlink(external.path().join("secret"), dir.path().join("link")).unwrap();
    git(dir.path(), &["config", "diff.external", "false"]);
    git(dir.path(), &["config", "diff.preview.textconv", "false"]);
    fs::write(dir.path().join(".gitattributes"), "file.txt diff=preview\n").unwrap();
    fs::write(dir.path().join("file.txt"), "changed\n").unwrap();
    let mut app = session(dir.path());
    refresh(&mut app);
    app.dispatch(AppCommand::ShowGitDiff {
        path: "link".into(),
        staged: false,
    })
    .unwrap();
    let view = settle(&mut app);
    assert!(view.error.is_some() && view.git.diff.is_none());
    app.dispatch(AppCommand::ShowGitDiff {
        path: "file.txt".into(),
        staged: false,
    })
    .unwrap();
    let view = settle(&mut app);
    assert!(view.error.is_none(), "{:?}", view.error);
    assert!(view.git.diff.as_ref().unwrap().text.contains("+changed"));
}

fn operate(app: &mut Session, operation: GitOperation) -> AppSnapshot {
    app.dispatch(AppCommand::Git(operation)).unwrap();
    let snapshot = settle(app);
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
    snapshot
}

#[test]
fn git_stage_unstage_commit_amend_and_undo_use_the_real_index() {
    let dir = repository();
    let mut app = session(dir.path());
    refresh(&mut app);
    operate(
        &mut app,
        GitOperation::Identity {
            name: "Native Test".into(),
            email: "native@example.invalid".into(),
        },
    );
    fs::write(dir.path().join("file.txt"), "first edit\n").unwrap();
    let staged = operate(
        &mut app,
        GitOperation::Stage {
            path: Some("file.txt".into()),
        },
    );
    assert!(staged.git.repository.as_ref().unwrap().changes[0].staged);
    let unstaged = operate(&mut app, GitOperation::Unstage { path: None });
    assert!(!unstaged.git.repository.as_ref().unwrap().changes[0].staged);
    operate(&mut app, GitOperation::Stage { path: None });
    let first = operate(
        &mut app,
        GitOperation::Commit {
            message: "Native commit".into(),
            amend: false,
            signoff: true,
        },
    );
    assert_eq!(first.git.history[0].commit.subject, "Native commit");
    let amended = operate(
        &mut app,
        GitOperation::Commit {
            message: "Amended commit".into(),
            amend: true,
            signoff: false,
        },
    );
    assert_eq!(amended.git.history.len(), first.git.history.len());
    assert_eq!(amended.git.history[0].commit.subject, "Amended commit");
    let undo = operate(&mut app, GitOperation::UndoCommit);
    assert_eq!(undo.git.history.len() + 1, amended.git.history.len());
    assert!(undo.git.repository.as_ref().unwrap().changes[0].staged);
}

#[test]
fn git_partial_staging_and_unstaging_preserve_unselected_lines_and_reject_stale_patches() {
    let dir = repository();
    let mut app = session(dir.path());
    refresh(&mut app);
    fs::write(
        dir.path().join("file.txt"),
        "original\nselected\nunselected\n",
    )
    .unwrap();
    app.dispatch(AppCommand::ShowGitDiff {
        path: "file.txt".into(),
        staged: false,
    })
    .unwrap();
    let view = settle(&mut app);
    let line = view
        .git
        .patch_lines
        .iter()
        .find(|l| l.text == "+selected\n")
        .unwrap()
        .index;
    app.git_stage_lines(vec![line]).unwrap();
    let view = settle(&mut app);
    assert!(view.error.is_none(), "{:?}", view.error);
    let index = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["show", ":file.txt"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(index.stdout).unwrap(),
        "original\nselected\n"
    );
    app.dispatch(AppCommand::ShowGitDiff {
        path: "file.txt".into(),
        staged: true,
    })
    .unwrap();
    let view = settle(&mut app);
    let line = view
        .git
        .patch_lines
        .iter()
        .find(|l| l.text == "+selected\n")
        .unwrap()
        .index;
    app.git_stage_lines(vec![line]).unwrap();
    assert!(settle(&mut app).error.is_none());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "original\nselected\nunselected\n"
    );
    app.dispatch(AppCommand::ShowGitDiff {
        path: "file.txt".into(),
        staged: false,
    })
    .unwrap();
    let view = settle(&mut app);
    let line = view
        .git
        .patch_lines
        .iter()
        .find(|l| l.text == "+selected\n")
        .unwrap()
        .index;
    fs::write(dir.path().join("file.txt"), "changed externally\n").unwrap();
    app.git_stage_lines(vec![line]).unwrap();
    assert!(settle(&mut app).error.is_some());
}

#[test]
fn git_branch_merge_graph_stash_and_history_are_real_repository_operations() {
    let dir = repository();
    let mut app = session(dir.path());
    let initial = refresh(&mut app);
    let base = initial.git.repository.as_ref().unwrap().branch.clone();
    operate(
        &mut app,
        GitOperation::Identity {
            name: "Native Test".into(),
            email: "native@example.invalid".into(),
        },
    );
    operate(
        &mut app,
        GitOperation::CreateBranch {
            name: "feature".into(),
            start: "HEAD".into(),
        },
    );
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Feature".into(),
            amend: false,
            signoff: false,
        },
    );
    operate(&mut app, GitOperation::Checkout { reference: base });
    fs::write(dir.path().join("main.txt"), "main\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Main".into(),
            amend: false,
            signoff: false,
        },
    );
    let merged = operate(
        &mut app,
        GitOperation::Merge {
            reference: "feature".into(),
        },
    );
    assert_eq!(merged.git.history[0].commit.parents.len(), 2);
    assert!(
        merged
            .git
            .history
            .iter()
            .any(|row| row.columns > 1 && row.edges.iter().any(|e| e.from != e.to))
    );
    // Every row reserves the same gutter, and the merge joins two lane colors.
    assert_eq!(merged.git.graph_width, 2);
    assert!(merged.git.history[0].merge && merged.git.history[0].head);
    let colors: std::collections::HashSet<_> = merged.git.history[0]
        .edges
        .iter()
        .map(|e| e.color)
        .collect();
    assert_eq!(colors.len(), 2);
    let commit = merged.git.history[0].commit.id.clone();
    app.dispatch(AppCommand::InspectGit(GitQuery::Commit(commit.clone())))
        .unwrap();
    let inspected = settle(&mut app);
    let inspection = inspected.git.inspection.as_ref().unwrap();
    assert!(inspection.text.contains("Merge"));
    let detail = inspection.commit.as_ref().unwrap();
    assert_eq!(detail.parents.len(), 2);
    assert_eq!(detail.email, "native@example.invalid");
    // A merge lists its first-parent changes: the branch's file.
    assert_eq!(detail.files.len(), 1);
    assert_eq!(
        (
            detail.files[0].path.as_str(),
            detail.files[0].status.as_str()
        ),
        ("feature.txt", "A")
    );
    app.git_request("show_commit_file", commit, "feature.txt".into(), false)
        .unwrap();
    let file = settle(&mut app);
    let inspection = file.git.inspection.as_ref().unwrap();
    assert_eq!(inspection.path.as_deref(), Some("feature.txt"));
    assert!(inspection.commit.is_some());
    let added = file
        .git
        .patch_lines
        .iter()
        .find(|l| l.text == "+feature\n")
        .unwrap();
    assert_eq!((added.kind, added.new), (PatchKind::Added, Some(1)));
    app.git_request(
        "show_commit_file",
        merged.git.history[0].commit.id.clone(),
        "missing.txt".into(),
        false,
    )
    .unwrap();
    assert!(settle(&mut app).error.is_some());
    fs::write(dir.path().join("file.txt"), "stashed\n").unwrap();
    let stashed = operate(
        &mut app,
        GitOperation::StashSave {
            message: "WIP".into(),
            include_untracked: true,
        },
    );
    assert_eq!(stashed.git.repository.as_ref().unwrap().stashes.len(), 1);
    // Stash commits are not user history.
    assert!(
        stashed
            .git
            .history
            .iter()
            .all(|row| row.commit.subject != "On feature: WIP"
                && !row.commit.subject.contains("WIP"))
    );
    operate(
        &mut app,
        GitOperation::StashApply {
            index: 0,
            pop: true,
        },
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "stashed\n"
    );
    operate(
        &mut app,
        GitOperation::Discard {
            path: "file.txt".into(),
        },
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "original\n"
    );
}

#[test]
fn git_failed_merge_publishes_conflicts_and_abort_reconciles_open_documents() {
    let dir = repository();
    let mut app = session(dir.path());
    let initial = refresh(&mut app);
    let base = initial.git.repository.as_ref().unwrap().branch.clone();
    operate(
        &mut app,
        GitOperation::Identity {
            name: "Test".into(),
            email: "test@example.invalid".into(),
        },
    );
    operate(
        &mut app,
        GitOperation::CreateBranch {
            name: "conflict".into(),
            start: "HEAD".into(),
        },
    );
    fs::write(dir.path().join("file.txt"), "incoming\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Incoming".into(),
            amend: false,
            signoff: false,
        },
    );
    operate(&mut app, GitOperation::Checkout { reference: base });
    fs::write(dir.path().join("file.txt"), "current\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Current".into(),
            amend: false,
            signoff: false,
        },
    );
    app.dispatch(AppCommand::OpenDocument(id("file.txt")))
        .unwrap();
    app.dispatch(AppCommand::Git(GitOperation::Merge {
        reference: "conflict".into(),
    }))
    .unwrap();
    let conflict = settle(&mut app);
    assert!(conflict.error.is_some());
    assert_eq!(
        conflict
            .git
            .repository
            .as_ref()
            .unwrap()
            .operation
            .as_deref(),
        Some("merge")
    );
    assert!(
        conflict
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.conflicted)
    );
    assert!(
        conflict
            .active_document
            .unwrap()
            .content
            .contains("<<<<<<<")
    );
    let aborted = operate(&mut app, GitOperation::Abort);
    assert_eq!(&*aborted.active_document.unwrap().content, "current\n");
    app.dispatch(AppCommand::ChangeDocument {
        id: id("file.txt"),
        content: "unsaved".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::Git(GitOperation::Checkout {
        reference: "conflict".into(),
    }))
    .unwrap();
    let blocked = settle(&mut app);
    assert!(blocked.error.is_some());
    assert_eq!(&*blocked.active_document.unwrap().content, "unsaved");
}

#[test]
fn git_remote_publish_fetch_pull_and_push_use_a_local_bare_remote() {
    let dir = repository();
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q"]);
    let mut app = session(dir.path());
    let view = refresh(&mut app);
    let branch = view.git.repository.as_ref().unwrap().branch.clone();
    operate(
        &mut app,
        GitOperation::AddRemote {
            name: "origin".into(),
            url: bare.path().to_str().unwrap().into(),
        },
    );
    let published = operate(
        &mut app,
        GitOperation::Publish {
            remote: "origin".into(),
        },
    );
    assert!(
        published
            .git
            .repository
            .as_ref()
            .unwrap()
            .branches
            .iter()
            .any(|b| b.upstream == format!("origin/{branch}"))
    );
    let checkout = tempfile::tempdir().unwrap();
    git(
        checkout.path(),
        &[
            "clone",
            "--branch",
            &branch,
            bare.path().to_str().unwrap(),
            "other",
        ],
    );
    let other = checkout.path().join("other");
    fs::write(other.join("incoming.txt"), "remote\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "Remote"]);
    git(&other, &["push"]);
    let fetched = operate(
        &mut app,
        GitOperation::Fetch {
            remote: "origin".into(),
        },
    );
    assert_eq!(fetched.git.repository.as_ref().unwrap().behind, 1);
    operate(&mut app, GitOperation::Pull { rebase: false });
    assert!(dir.path().join("incoming.txt").exists());
    operate(
        &mut app,
        GitOperation::Push {
            force_with_lease: false,
        },
    );
}

#[test]
fn git_merge_editor_rejects_stale_results_and_stages_a_reviewed_resolution() {
    let dir = repository();
    let mut app = session(dir.path());
    let base = refresh(&mut app)
        .git
        .repository
        .as_ref()
        .unwrap()
        .branch
        .clone();
    operate(
        &mut app,
        GitOperation::Identity {
            name: "Test".into(),
            email: "test@example.invalid".into(),
        },
    );
    operate(
        &mut app,
        GitOperation::CreateBranch {
            name: "incoming".into(),
            start: "HEAD".into(),
        },
    );
    fs::write(dir.path().join("file.txt"), "shared\nincoming\nend\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Incoming".into(),
            amend: false,
            signoff: false,
        },
    );
    operate(&mut app, GitOperation::Checkout { reference: base });
    fs::write(dir.path().join("file.txt"), "shared\ncurrent\nend\n").unwrap();
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Current".into(),
            amend: false,
            signoff: false,
        },
    );
    app.dispatch(AppCommand::Git(GitOperation::Merge {
        reference: "incoming".into(),
    }))
    .unwrap();
    assert!(settle(&mut app).error.is_some());
    app.dispatch(AppCommand::Git(GitOperation::Stage { path: None }))
        .unwrap();
    assert!(settle(&mut app).error.is_some());
    app.dispatch(AppCommand::InspectGit(GitQuery::Conflict(
        "file.txt".into(),
    )))
    .unwrap();
    let view = settle(&mut app);
    let conflict = view
        .git
        .inspection
        .as_ref()
        .unwrap()
        .conflict
        .as_ref()
        .unwrap();
    assert_eq!(conflict.accept_both, "shared\ncurrent\nincoming\nend\n");
    let resolution = conflict.accept_both.clone();
    fs::write(dir.path().join("file.txt"), "external\n").unwrap();
    app.git_resolve(resolution.clone()).unwrap();
    assert!(settle(&mut app).error.is_some());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "external\n"
    );
    app.dispatch(AppCommand::InspectGit(GitQuery::Conflict(
        "file.txt".into(),
    )))
    .unwrap();
    settle(&mut app);
    app.git_resolve(resolution).unwrap();
    let resolved = settle(&mut app);
    assert!(resolved.error.is_none(), "{:?}", resolved.error);
    assert!(
        resolved
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .all(|c| !c.conflicted)
    );
    operate(&mut app, GitOperation::Continue);
    assert!(
        app.snapshot()
            .git
            .repository
            .as_ref()
            .unwrap()
            .operation
            .is_none()
    );
}

#[test]
fn git_initialize_tags_ignore_clone_and_worktree_lifecycle() {
    let dir = workspace(&[("new.txt", "new\n")]);
    let mut app = session(dir.path());
    operate(
        &mut app,
        GitOperation::Initialize {
            branch: "main".into(),
        },
    );
    operate(
        &mut app,
        GitOperation::Identity {
            name: "Test".into(),
            email: "test@example.invalid".into(),
        },
    );
    operate(&mut app, GitOperation::Stage { path: None });
    operate(
        &mut app,
        GitOperation::Commit {
            message: "Initial".into(),
            amend: false,
            signoff: false,
        },
    );
    operate(
        &mut app,
        GitOperation::CreateTag {
            name: "v1".into(),
            commit: "HEAD".into(),
        },
    );
    let tagged = refresh(&mut app);
    assert!(tagged.git.history[0].head);
    assert!(
        tagged.git.history[0]
            .commit
            .refs
            .iter()
            .any(|r| r.name == "v1" && r.kind == GitRefKind::Tag)
    );
    operate(&mut app, GitOperation::DeleteTag { name: "v1".into() });
    fs::write(dir.path().join("ignored ü.txt"), "ignore\n").unwrap();
    operate(
        &mut app,
        GitOperation::Ignore {
            path: "ignored ü.txt".into(),
        },
    );
    assert!(
        !refresh(&mut app)
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "ignored ü.txt")
    );
    let extra = tempfile::tempdir().unwrap();
    let location = extra.path().join("worktree");
    operate(
        &mut app,
        GitOperation::WorktreeAdd {
            directory: location.to_str().unwrap().into(),
            reference: "HEAD".into(),
        },
    );
    assert!(location.join("new.txt").exists());
    operate(
        &mut app,
        GitOperation::WorktreeRemove {
            directory: location.to_str().unwrap().into(),
        },
    );
    assert!(!location.exists());
    let clone = extra.path().join("clone");
    operate(
        &mut app,
        GitOperation::Clone {
            url: dir.path().to_str().unwrap().into(),
            directory: clone.to_str().unwrap().into(),
        },
    );
    assert!(clone.join("new.txt").exists());
}

#[test]
fn git_background_poll_keeps_errors_and_does_not_modify_editor_buffers() {
    let dir = repository();
    let mut app = session(dir.path());
    refresh(&mut app);
    app.dispatch(AppCommand::OpenDocument(id("file.txt")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("file.txt"),
        content: "unsaved".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::InspectGit(GitQuery::Commit(
        "missing-ref".into(),
    )))
    .unwrap();
    let failed = settle(&mut app).error.unwrap();
    app.dispatch(AppCommand::PollGit).unwrap();
    let view = settle(&mut app);
    assert_eq!(view.error.as_deref(), Some(failed.as_str()));
    assert_eq!(&*view.active_document.unwrap().content, "unsaved");
}

#[test]
fn git_repository_selection_keeps_nested_repositories_and_their_histories_isolated() {
    let dir = workspace(&[("a/a.txt", "a"), ("b/b.txt", "b")]);
    for name in ["a", "b"] {
        let root = dir.path().join(name);
        git(&root, &["init", "-q"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", name]);
    }
    let mut app = session(dir.path());
    let initial = refresh(&mut app);
    assert_eq!(initial.git.roots.len(), 2);
    assert_eq!(initial.git.history[0].commit.subject, "a");
    let second = initial.git.roots[1].clone();
    app.dispatch(AppCommand::SelectGitRepository(second.clone()))
        .unwrap();
    let selected = settle(&mut app);
    assert_eq!(selected.git.history[0].commit.subject, "b");
    let polled = refresh(&mut app);
    assert_eq!(polled.git.selected_root.as_ref(), Some(&second));
    assert_eq!(polled.git.history[0].commit.subject, "b");
    app.dispatch(AppCommand::SelectGitRepository(dir.path().join("unknown")))
        .unwrap();
    assert!(settle(&mut app).error.is_some());
    assert_eq!(app.snapshot().git.selected_root.as_ref(), Some(&second));
}

fn commit(id: &str, parents: &[&str]) -> GitCommit {
    GitCommit {
        id: id.into(),
        parents: parents.iter().map(|p| (*p).into()).collect(),
        author: String::new(),
        date: String::new(),
        refs: Vec::new(),
        subject: String::new(),
    }
}

#[test]
fn git_graph_keeps_lane_colors_and_starts_branch_tips_at_their_dot() {
    // m merges b into a; b and a fork from root r.
    let rows = crate::git::graph(vec![
        commit("m", &["a", "b"]),
        commit("b", &["r"]),
        commit("a", &["r"]),
        commit("r", &[]),
    ]);
    // The tip has no line above its dot.
    assert!(rows[0].edges.iter().all(|e| e.start == 1));
    let (main, side) = (rows[0].color, rows[1].color);
    assert_ne!(main, side);
    assert_eq!((rows[1].lane, rows[2].lane), (1, 0));
    // The first-parent lane keeps the merge commit's color down to `a`.
    assert_eq!(rows[2].color, main);
    // The side lane curves into the first-parent line, which stays straight.
    let joins: Vec<_> = rows[2]
        .edges
        .iter()
        .map(|e| (e.from, e.to, e.start, e.end, e.color))
        .collect();
    assert_eq!(
        joins,
        vec![(0, 0, 0, 1, main), (1, 0, 0, 2, side), (0, 0, 1, 2, main)]
    );
    let into_root: Vec<_> = rows[3].edges.iter().map(|e| (e.from, e.color)).collect();
    assert_eq!(into_root, vec![(0, main)]);
    assert_eq!(crate::git::graph_width(&rows), 2);
}

#[test]
fn git_patch_lines_number_hunks_and_keep_header_like_content_inside_hunks() {
    let text =
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -3,2 +3,2 @@\n context\n--- removed\n+++ added\n";
    let lines = crate::git::patch::lines(text);
    let kinds: Vec<_> = lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PatchKind::Meta,
            PatchKind::Meta,
            PatchKind::Meta,
            PatchKind::Hunk,
            PatchKind::Context,
            PatchKind::Removed,
            PatchKind::Added,
        ]
    );
    assert_eq!((lines[4].old, lines[4].new), (Some(3), Some(3)));
    assert_eq!((lines[5].old, lines[6].new), (Some(4), Some(4)));
    assert!(lines[5].changed && !lines[1].changed);
}

#[test]
fn files_tree_git_decorations_include_ancestors_ignored_and_subworkspaces() {
    let dir = repository();
    let root = dir.path();
    fs::write(root.join(".gitignore"), "cache/\n*.log\n").unwrap();
    fs::create_dir_all(root.join("sub/new/deep")).unwrap();
    fs::create_dir_all(root.join("cache/deep")).unwrap();
    fs::write(root.join("cache/deep/file.txt"), "ignored").unwrap();
    fs::write(root.join("sub/debug.log"), "ignored").unwrap();
    fs::write(root.join("sub/new/deep/added.txt"), "new").unwrap();
    fs::write(root.join("sub/note.txt"), "modified").unwrap();
    let mut app = session(root);
    let view = refresh(&mut app);
    for path in ["sub/new", "sub/new/deep", "sub/new/deep/added.txt"] {
        assert_eq!(view.sidebar_git_status(path), "A", "{path}");
    }
    assert_eq!(view.sidebar_git_status("sub"), "M");
    assert_eq!(view.sidebar_git_status("sub/note.txt"), "M");
    assert_eq!(view.sidebar_git_status("cache"), "!");
    assert_eq!(view.sidebar_git_status("cache/deep/file.txt"), "!");
    assert_eq!(view.sidebar_git_status("sub/debug.log"), "!");
    assert_eq!(view.sidebar_git_status("cache-other/file.txt"), "");
    assert_eq!(view.sidebar_git_status("file.txt"), "");
    assert!(
        !view
            .git
            .repository
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path.contains("debug.log"))
    );
    let mut app = session(&root.join("sub"));
    let view = refresh(&mut app);
    assert_eq!(view.sidebar_git_status("note.txt"), "M");
    assert_eq!(view.sidebar_git_status("new"), "A");
    assert_eq!(view.sidebar_git_status("debug.log"), "!");
    fs::write(root.join("sub/note.txt"), "note\n").unwrap();
    fs::remove_file(root.join("sub/new/deep/added.txt")).unwrap();
    let view = refresh(&mut app);
    assert_eq!(view.sidebar_git_status("note.txt"), "");
    assert_eq!(view.sidebar_git_status("new"), "");
}

#[test]
fn files_tree_loads_git_without_opening_source_control() {
    let dir = repository();
    fs::write(dir.path().join("sub/note.txt"), "changed").unwrap();
    let mut app = session(dir.path());
    let view = wait_until(
        || {
            app.poll();
            app.snapshot()
        },
        |s| s.git.loaded,
    );
    assert_eq!(view.sidebar_git_status("sub"), "M");
}

#[test]
fn files_tree_decorations_include_nested_repositories_independently_of_selection() {
    let dir = repository();
    let nested = dir.path().join("nested");
    fs::create_dir_all(nested.join("src")).unwrap();
    git(&nested, &["init", "-q"]);
    fs::write(nested.join("src/new.txt"), "new").unwrap();
    fs::write(dir.path().join("sub/note.txt"), "changed").unwrap();
    let mut app = session(dir.path());
    let view = refresh(&mut app);
    assert_eq!(view.sidebar_git_status("nested/src"), "A");
    assert_eq!(view.sidebar_git_status("nested/src/new.txt"), "A");
    assert_eq!(view.sidebar_git_status("sub"), "M");
    app.dispatch(AppCommand::SelectGitRepository(nested))
        .unwrap();
    let view = settle(&mut app);
    assert_eq!(view.sidebar_git_status("nested/src/new.txt"), "A");
    assert_eq!(view.sidebar_git_status("sub"), "M");
}
