use super::*;

#[test]
fn cold_file_open_uses_its_parent_and_keeps_later_external_tabs_distinct() {
    let dir = workspace(&[("same name ü.txt", "first")]);
    let outside = workspace(&[("same name ü.txt", "second")]);
    let mut session = Session::new();
    session
        .dispatch(AppCommand::OpenPaths(vec![
            dir.path().join("same name ü.txt"),
            outside.path().join("same name ü.txt"),
        ]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
    assert_eq!(
        snapshot.workspace_root(),
        Some(fs::canonicalize(dir.path()).unwrap().as_path())
    );
    assert_eq!(snapshot.open_documents.len(), 2);
    assert_eq!(snapshot.open_documents[0], id("same name ü.txt"));
    assert!(snapshot.open_documents[1].is_external());
    assert!(snapshot.preview_document.is_none());
    assert_eq!(&*snapshot.active_document.unwrap().content, "second");
}

#[test]
fn folder_requests_preserve_dirty_buffers_and_reopening_current_folder_is_a_noop() {
    let dir = workspace(&[("note", "saved")]);
    let other = workspace(&[("other", "other")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "unsaved".into(),
        })
        .unwrap();
    let before = session.snapshot();
    session
        .dispatch(AppCommand::OpenPaths(vec![dir.path().into()]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_none());
    assert_eq!(snapshot.editor_generation, before.editor_generation);
    assert_eq!(snapshot.open_documents, before.open_documents);
    session
        .dispatch(AppCommand::OpenPaths(vec![other.path().into()]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.workspace, before.workspace);
    assert_eq!(&*snapshot.active_document.unwrap().content, "unsaved");
    assert_eq!(
        fs::read_to_string(dir.path().join("note")).unwrap(),
        "saved"
    );
}

#[test]
fn ordered_folder_and_file_requests_keep_committed_steps_on_a_later_failure() {
    let dir = workspace(&[("good", "content"), ("bad", "")]);
    fs::write(dir.path().join("bad"), [0xff]).unwrap();
    let mut session = Session::new();
    session
        .dispatch(AppCommand::OpenPaths(vec![
            dir.path().into(),
            dir.path().join("good"),
            dir.path().join("bad"),
        ]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.open_documents, [id("good")]);
    assert_eq!(&*snapshot.active_document.unwrap().content, "content");
    // A later delivery can still replace a clean workspace.
    let next = workspace(&[("next", "next")]);
    session
        .dispatch(AppCommand::OpenPaths(vec![
            next.path().into(),
            next.path().join("next"),
        ]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_none());
    assert_eq!(snapshot.open_documents, [id("next")]);
}

#[test]
fn system_requests_queue_during_exclusive_work_and_do_not_block_the_caller() {
    let dir = workspace(&[("first", "first"), ("second", "second")]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Resolve) {
            hook_gate.wait();
        }
        Ok(())
    });
    let mut session = Session::with_filesystem(Box::new(filesystem));
    let start = Instant::now();
    session
        .dispatch(AppCommand::OpenPaths(vec![dir.path().join("first")]))
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    assert!(session.snapshot().activity.editing_paused);
    session
        .dispatch(AppCommand::OpenPaths(vec![dir.path().join("second")]))
        .unwrap();
    gate.open();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
    assert_eq!(snapshot.open_documents, [id("first"), id("second")]);
    assert_eq!(snapshot.active_id(), Some(&id("second")));
}

#[test]
fn invalid_os_paths_never_create_a_workspace() {
    let dir = workspace(&[]);
    for path in [
        "relative".into(),
        "https://example.test/note".into(),
        dir.path().join("missing"),
    ] {
        let mut session = Session::new();
        session.dispatch(AppCommand::OpenPaths(vec![path])).unwrap();
        let snapshot = settle(&mut session);
        assert!(snapshot.error.is_some());
        assert!(snapshot.workspace.is_none());
    }
}

#[test]
fn a_queued_folder_request_pauses_typing_and_retains_edits_already_projected() {
    let dir = workspace(&[("note", "saved")]);
    let next = workspace(&[]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let block_reads = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_block_reads = block_reads.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if hook_block_reads.load(std::sync::atomic::Ordering::SeqCst)
            && matches!(operation, Operation::Read(_))
        {
            hook_gate.wait();
        }
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    // First load without blocking; then block a background refresh.
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    block_reads.store(true, std::sync::atomic::Ordering::SeqCst);
    session.dispatch(AppCommand::RefreshWorkspace).unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "keep this edit".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::OpenPaths(vec![next.path().into()]))
        .unwrap();
    assert!(session.snapshot().activity.editing_paused);
    assert!(matches!(
        session.dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "too late".into()
        }),
        Err(AppError::OperationInProgress)
    ));
    gate.open();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(
        snapshot.workspace_root(),
        Some(fs::canonicalize(dir.path()).unwrap().as_path())
    );
    assert_eq!(
        &*snapshot.active_document.unwrap().content,
        "keep this edit"
    );
}

#[cfg(unix)]
#[test]
fn system_open_rejects_symlink_files_and_folders() {
    let dir = workspace(&[("note", "safe")]);
    let links = workspace(&[]);
    for (name, target) in [
        ("file", dir.path().join("note")),
        ("folder", dir.path().into()),
    ] {
        let link = links.path().join(name);
        std::os::unix::fs::symlink(target, &link).unwrap();
        let mut session = Session::new();
        session.dispatch(AppCommand::OpenPaths(vec![link])).unwrap();
        let snapshot = settle(&mut session);
        assert!(snapshot.error.is_some());
        assert!(snapshot.workspace.is_none());
    }
}
