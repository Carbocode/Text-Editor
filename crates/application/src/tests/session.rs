use super::*;
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

/// A session whose renames wait for `gate`.
fn gated_session(dir: &Path, gate: &Gate) -> Session {
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(..)) {
            hook_gate.wait();
        }
        Ok(())
    });
    Session::with_application(open(dir, filesystem))
}

#[test]
fn relocation_is_projected_at_once_and_rollback_keeps_edits_and_queued_saves() {
    for reject in [false, true] {
        let dir = workspace(&[("src/nested/a.rs", "original"), ("dest/.keep", "")]);
        let gate = Gate::default();
        let mut session = gated_session(dir.path(), &gate);
        session
            .dispatch(AppCommand::OpenDocument(id("src/nested/a.rs")))
            .unwrap();
        let shown = session.snapshot().editor_generation;
        let started = Instant::now();
        session
            .dispatch(AppCommand::MoveEntry {
                id: id("src"),
                directory: "dest".into(),
            })
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "the drop waited for filesystem confirmation"
        );
        let moved = session.snapshot();
        assert_eq!(moved.active_id(), Some(&id("dest/src/nested/a.rs")));
        assert!(moved.directories.contains(&"dest/src/nested".into()));
        assert_eq!(
            moved.activity.relocation,
            Some(Relocation {
                source: id("src"),
                target: id("dest/src"),
            })
        );
        assert_eq!(moved.editor_generation, shown, "a move keeps the editor");
        assert!(dir.path().join("src/nested/a.rs").exists());
        session
            .dispatch(AppCommand::ChangeDocument {
                id: id("dest/src/nested/a.rs"),
                content: "typed while moving".into(),
            })
            .unwrap();
        session
            .dispatch(AppCommand::SaveDocument(id("dest/src/nested/a.rs")))
            .unwrap();
        assert_eq!(
            &*session.snapshot().active_document.unwrap().content,
            "typed while moving"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("src/nested/a.rs")).unwrap(),
            "original"
        );
        if reject {
            fs::create_dir(dir.path().join("dest/src")).unwrap();
            fs::write(dir.path().join("dest/src/external.txt"), "external").unwrap();
        }
        gate.open();
        let done = settle(&mut session);
        let final_path = if reject {
            "src/nested/a.rs"
        } else {
            "dest/src/nested/a.rs"
        };
        let active = done.active_document.unwrap();
        assert_eq!(active.id, id(final_path));
        assert_eq!(&*active.content, "typed while moving");
        assert!(!active.dirty);
        assert_eq!(
            fs::read_to_string(dir.path().join(final_path)).unwrap(),
            "typed while moving"
        );
        assert_eq!(done.error.is_some(), reject);
        if reject {
            assert_eq!(
                fs::read_to_string(dir.path().join("dest/src/external.txt")).unwrap(),
                "external"
            );
        }
    }
}

#[test]
fn polling_stays_responsive_and_slow_relocations_show_progress() {
    let dir = workspace(&[("a.rs", "original"), ("dest/.keep", "")]);
    let gate = Gate::default();
    let mut session = gated_session(dir.path(), &gate);
    session
        .dispatch(AppCommand::OpenDocument(id("a.rs")))
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("a.rs"),
            content: "local edit before drop".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::MoveEntry {
            id: id("a.rs"),
            directory: "dest".into(),
        })
        .unwrap();
    let projected = session.snapshot();
    assert_eq!(projected.active_id(), Some(&id("dest/a.rs")));
    assert!(
        !projected.activity.relocation_slow,
        "a fresh move must not flash a progress indicator"
    );
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        let poll = Instant::now();
        session.poll();
        assert!(poll.elapsed() < Duration::from_millis(50));
        thread::sleep(Duration::from_millis(10));
    }
    assert!(session.snapshot().activity.relocation_slow);
    assert!(session.wants_fast_poll());
    gate.open();
    let done = settle(&mut session);
    assert_eq!(
        &*done.active_document.unwrap().content,
        "local edit before drop"
    );
    assert!(!done.activity.relocation_slow);
}

#[test]
fn replaying_a_slow_queued_save_never_blocks_or_overwrites_new_typing() {
    let dir = workspace(&[("a.rs", "original"), ("dest/.keep", "")]);
    let gate = Gate::default();
    let (entered, observed) = mpsc::channel();
    let entered = StdMutex::new(entered);
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Write(_)) {
            let _ = entered.lock().unwrap().send(());
            hook_gate.wait();
        }
        Ok(())
    });
    let mut core = open(dir.path(), filesystem);
    core.dispatch(AppCommand::OpenDocument(id("a.rs"))).unwrap();
    let mut session = Session::with_application(core);
    session
        .dispatch(AppCommand::MoveEntry {
            id: id("a.rs"),
            directory: "dest".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("dest/a.rs"),
            content: "first edit".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::SaveDocument(id("dest/a.rs")))
        .unwrap();
    // Polling confirms the move and replays the queued save on the worker.
    wait_until(
        || {
            session.poll();
            observed.try_recv().is_ok()
        },
        |entered| *entered,
    );
    assert!(session.snapshot().activity.relocation.is_none());
    let typed_at = Instant::now();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("dest/a.rs"),
            content: "second edit during slow save".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::SaveDocument(id("dest/a.rs")))
        .unwrap();
    assert!(typed_at.elapsed() < Duration::from_millis(100));
    assert_eq!(
        &*session.snapshot().active_document.unwrap().content,
        "second edit during slow save"
    );
    gate.open();
    let done = wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |snapshot| !snapshot.activity.busy && !snapshot.is_dirty(&id("dest/a.rs")),
    );
    let active = done.active_document.unwrap();
    assert_eq!(&*active.content, "second edit during slow save");
    assert_eq!(
        fs::read_to_string(dir.path().join("dest/a.rs")).unwrap(),
        "second edit during slow save"
    );
}

#[test]
fn background_scans_never_block_dispatch_or_overwrite_edits_made_meanwhile() {
    let dir = workspace(&[("a.txt", "original")]);
    let gate = Gate::default();
    let (started, observed) = mpsc::channel();
    let started = StdMutex::new(started);
    let opens = AtomicUsize::new(0);
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Open) && opens.fetch_add(1, Ordering::SeqCst) > 0 {
            let _ = started.lock().unwrap().send(());
            hook_gate.wait();
        }
        Ok(())
    });
    let mut app = open(dir.path(), filesystem);
    app.dispatch(AppCommand::OpenDocument(id("a.txt"))).unwrap();
    app.watcher = None;
    app.last_reconcile = Instant::now() - Duration::from_secs(3);
    let start = Instant::now();
    app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "poll blocked on listing"
    );
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    let start = Instant::now();
    app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a.txt"),
        content: "local edit".into(),
    })
    .unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "a pending scan froze later commands"
    );
    fs::write(dir.path().join("a.txt"), "external edit").unwrap();
    gate.open();
    let conflicted = wait_until(
        || {
            app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
            let active = app.snapshot().active_document.unwrap();
            assert_eq!(
                &*active.content, "local edit",
                "a stale scan overwrote the editor"
            );
            active
        },
        |active| active.external_change,
    );
    assert!(conflicted.dirty);
}

#[test]
fn the_name_filter_publishes_on_poll_and_follows_path_changes() {
    let dir = workspace(&[("src/main.rs", ""), ("README.md", "")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    session.filter_files("MAIN");
    assert!(session.snapshot().file_filter_in_progress);
    let filtered = wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |snapshot| !snapshot.file_filter_in_progress,
    );
    assert_eq!(filtered.file_filter.files, ["src/main.rs"]);
    assert_eq!(filtered.file_filter.directories, ["src"]);
    session
        .dispatch(AppCommand::CreateEntry {
            id: id("src/domain.rs"),
            directory: false,
        })
        .unwrap();
    let refiltered = wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |snapshot| snapshot.file_filter.files.len() == 2,
    );
    assert_eq!(
        refiltered.file_filter.files,
        ["src/domain.rs", "src/main.rs"]
    );
    session.filter_files(" ");
    assert!(!session.snapshot().file_filter.is_active());
}

#[test]
fn errors_stay_published_until_the_next_successful_command() {
    let dir = workspace(&[("a.md", "a")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    assert!(
        session
            .dispatch(AppCommand::OpenDocument(id("missing.md")))
            .is_err()
    );
    let failed = session.snapshot().error.unwrap();
    assert!(failed.contains("missing.md"));
    session.poll();
    assert_eq!(session.snapshot().error.as_deref(), Some(failed.as_str()));
    session
        .dispatch(AppCommand::OpenDocument(id("a.md")))
        .unwrap();
    assert!(session.snapshot().error.is_none());
}

#[test]
fn a_panicking_job_returns_the_core_and_preserves_unsaved_buffers() {
    let dir = workspace(&[("note", "original"), ("dest/.keep", "")]);
    let filesystem = TestFilesystem::with(|operation| {
        assert!(
            !matches!(operation, Operation::Rename(..)),
            "injected worker panic"
        );
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    session
        .dispatch(AppCommand::MoveEntry {
            id: id("note"),
            directory: "dest".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("dest/note"),
            content: "keep this edit".into(),
        })
        .unwrap();
    let done = settle(&mut session);
    assert_eq!(done.active_id(), Some(&id("note")));
    assert_eq!(&*done.active_document.unwrap().content, "keep this edit");
    assert!(done.error.is_some());
    session
        .dispatch(AppCommand::SaveDocument(id("note")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("note")).unwrap(),
        "keep this edit"
    );
}

#[test]
fn unchanged_document_summaries_stay_shared_across_typing() {
    let dir = workspace(&[("note", "original")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "first edit".into(),
        })
        .unwrap();
    let first = session.snapshot();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "second edit".into(),
        })
        .unwrap();
    let second = session.snapshot();
    assert!(Arc::ptr_eq(&first.documents, &second.documents));
    assert!(Arc::ptr_eq(&first.directories, &second.directories));
    assert!(Arc::ptr_eq(&first.search_results, &second.search_results));
    assert!(second.revision > first.revision);
}

#[test]
fn an_earlier_jobs_success_does_not_clear_a_newer_failure() {
    let directory = workspace(&[("note", "text"), ("dest/.keep", "")]);
    let gate = Gate::default();
    let mut session = gated_session(directory.path(), &gate);
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    session
        .dispatch(AppCommand::MoveEntry {
            id: id("note"),
            directory: "dest".into(),
        })
        .unwrap();
    assert!(matches!(
        session.dispatch(AppCommand::DeleteEntry(id("dest/note"))),
        Err(AppError::OperationInProgress)
    ));
    let failure = session.snapshot().error;
    gate.open();
    let done = settle(&mut session);
    assert_eq!(done.error, failure);
    session
        .dispatch(AppCommand::SaveDocument(id("dest/note")))
        .unwrap();
    assert!(session.snapshot().error.is_none());
}
