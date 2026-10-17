use super::*;

fn external(path: &Path) -> DocumentId {
    DocumentId::from_document_path(fs::canonicalize(path).unwrap().to_str().unwrap()).unwrap()
}

#[test]
fn os_files_open_permanently_without_changing_workspace_or_leaking_into_its_tree() {
    let dir = workspace(&[("inside.txt", "workspace needle")]);
    let outside = workspace(&[("same name ü.txt", "outside needle")]);
    let other = workspace(&[("same name ü.txt", "different")]);
    let mut app = open(dir.path(), TestFilesystem::real());
    let first = outside.path().join("same name ü.txt");
    let second = other.path().join("same name ü.txt");
    let root = app.snapshot().workspace;
    let snapshot = app
        .dispatch(AppCommand::OpenFiles(vec![
            first.clone(),
            dir.path().join("inside.txt"),
            second.clone(),
            first.clone(),
        ]))
        .unwrap();
    assert_eq!(snapshot.workspace, root);
    assert_eq!(snapshot.open_documents.len(), 3);
    assert_eq!(snapshot.active_id(), Some(&external(&first)));
    assert!(snapshot.preview_document.is_none());
    assert_eq!(
        snapshot
            .sidebar_children("")
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>(),
        ["inside.txt"]
    );
    let snapshot = search(&mut app, "needle");
    assert_eq!(snapshot.search_results.len(), 1);
    assert_eq!(snapshot.search_results[0].id, id("inside.txt"));
    let snapshot = app
        .dispatch(AppCommand::CloseDocument(external(&first)))
        .unwrap();
    assert!(
        snapshot
            .document_summary(external(&first).as_str())
            .is_none()
    );
}

#[test]
fn unreadable_os_file_keeps_previous_successes_and_the_existing_preview() {
    let dir = workspace(&[("preview", "keep")]);
    let outside = workspace(&[("good", "loaded"), ("bad", "")]);
    fs::write(outside.path().join("bad"), [0xff, 0xfe]).unwrap();
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::PreviewDocument(id("preview")))
        .unwrap();
    assert!(
        app.dispatch(AppCommand::OpenFiles(vec![
            outside.path().join("good"),
            outside.path().join("bad")
        ]))
        .is_err()
    );
    let snapshot = app.snapshot();
    assert_eq!(
        snapshot.open_documents,
        [id("preview"), external(&outside.path().join("good"))]
    );
    assert_eq!(snapshot.preview_document, Some(id("preview")));
    assert!(
        snapshot
            .document_summary(external(&outside.path().join("bad")).as_str())
            .is_none()
    );
}

#[test]
fn external_save_and_refresh_detect_changes_and_never_overwrite_implicitly() {
    let dir = workspace(&[]);
    let outside = workspace(&[("note", "original")]);
    let path = outside.path().join("note");
    let document = external(&path);
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenFile(path.clone())).unwrap();
    fs::write(&path, "updated").unwrap();
    let snapshot = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    assert_eq!(&*snapshot.active_document.unwrap().content, "updated");
    app.dispatch(AppCommand::ChangeDocument {
        id: document.clone(),
        content: "local".into(),
    })
    .unwrap();
    fs::write(&path, "disk").unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::SaveAndCloseDocument(document.clone())),
        Err(AppError::ExternalChanges(_))
    ));
    let snapshot = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    assert!(snapshot.is_open(&document));
    let active = snapshot.active_document.unwrap();
    assert_eq!(&*active.content, "local");
    assert!(active.external_change);
    app.dispatch(AppCommand::OverwriteDocument(document.clone()))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "local");
    app.dispatch(AppCommand::CloseDocument(document)).unwrap();
}

#[test]
fn moving_an_open_external_file_retains_its_dirty_buffer_and_editor_identity() {
    let dir = workspace(&[("dest/.keep", "")]);
    let outside = workspace(&[("note", "saved")]);
    let path = outside.path().join("note");
    let document = external(&path);
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenFile(path.clone())).unwrap();
    let generation = app.snapshot().editor_generation;
    app.dispatch(AppCommand::ChangeDocument {
        id: document,
        content: "unsaved".into(),
    })
    .unwrap();
    let snapshot = app
        .dispatch(AppCommand::MoveFiles {
            paths: vec![path.clone()],
            directory: "dest".into(),
        })
        .unwrap();
    assert!(!path.exists());
    assert_eq!(snapshot.open_documents, [id("dest/note")]);
    assert_eq!(snapshot.editor_generation, generation);
    assert_eq!(&*snapshot.active_document.unwrap().content, "unsaved");
    assert_eq!(
        fs::read_to_string(dir.path().join("dest/note")).unwrap(),
        "saved"
    );
    app.dispatch(AppCommand::SaveAllDocuments).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("dest/note")).unwrap(),
        "unsaved"
    );
}

#[test]
fn move_collisions_protect_disk_and_missing_files_with_retained_buffers() {
    for remove_target in [false, true] {
        let dir = workspace(&[("note", "destination")]);
        let outside = workspace(&[("note", "source")]);
        let path = outside.path().join("note");
        let mut app = open(dir.path(), TestFilesystem::real());
        app.dispatch(AppCommand::OpenDocument(id("note"))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "unsaved".into(),
        })
        .unwrap();
        if remove_target {
            fs::remove_file(dir.path().join("note")).unwrap();
        }
        assert!(matches!(
            app.dispatch(AppCommand::MoveFiles {
                paths: vec![path.clone()],
                directory: String::new()
            }),
            Err(AppError::DestinationExists(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "source");
        assert_eq!(&*app.snapshot().active_document.unwrap().content, "unsaved");
        if !remove_target {
            assert_eq!(
                fs::read_to_string(dir.path().join("note")).unwrap(),
                "destination"
            );
        }
    }
}

#[test]
fn move_batch_publishes_earlier_success_when_a_later_file_fails() {
    let dir = workspace(&[("dest/second", "existing")]);
    let outside = workspace(&[("first", "first"), ("second", "second")]);
    let mut app = open(dir.path(), TestFilesystem::real());
    assert!(
        app.dispatch(AppCommand::MoveFiles {
            paths: vec![outside.path().join("first"), outside.path().join("second")],
            directory: "dest".into()
        })
        .is_err()
    );
    assert!(app.snapshot().document_summary("dest/first").is_some());
    assert!(!outside.path().join("first").exists());
    assert_eq!(
        fs::read_to_string(outside.path().join("second")).unwrap(),
        "second"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("dest/second")).unwrap(),
        "existing"
    );
}

#[test]
fn failed_background_move_keeps_buffer_and_does_not_block_the_caller() {
    let dir = workspace(&[("dest/.keep", "")]);
    let outside = workspace(&[("note", "saved")]);
    let path = outside.path().join("note");
    let document = external(&path);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(..)) {
            hook_gate.wait();
            return Err(io::Error::other("denied"));
        }
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    session
        .dispatch(AppCommand::OpenFiles(vec![path.clone()]))
        .unwrap();
    settle(&mut session);
    session
        .dispatch(AppCommand::ChangeDocument {
            id: document.clone(),
            content: "unsaved".into(),
        })
        .unwrap();
    let start = Instant::now();
    session
        .dispatch(AppCommand::MoveFiles {
            paths: vec![path.clone()],
            directory: "dest".into(),
        })
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    assert!(session.snapshot().activity.editing_paused);
    assert!(matches!(
        session.dispatch(AppCommand::ChangeDocument {
            id: document.clone(),
            content: "rejected".into()
        }),
        Err(AppError::OperationInProgress)
    ));
    gate.open();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.active_id(), Some(&document));
    assert_eq!(&*snapshot.active_document.unwrap().content, "unsaved");
    assert_eq!(fs::read_to_string(path).unwrap(), "saved");
}

#[test]
fn os_drop_of_a_workspace_file_reuses_its_tab_and_moves_its_edits() {
    let dir = workspace(&[("note", "saved"), ("dest/.keep", "")]);
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenDocument(id("note"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("note"),
        content: "unsaved".into(),
    })
    .unwrap();
    let snapshot = app
        .dispatch(AppCommand::OpenFile(dir.path().join("note")))
        .unwrap();
    assert_eq!(snapshot.open_documents, [id("note")]);
    let snapshot = app
        .dispatch(AppCommand::MoveFile {
            path: dir.path().join("note"),
            directory: "dest".into(),
        })
        .unwrap();
    assert_eq!(snapshot.open_documents, [id("dest/note")]);
    assert_eq!(&*snapshot.active_document.unwrap().content, "unsaved");
}

#[test]
fn background_os_open_preserves_edits_and_saves_queued_for_the_previous_document() {
    let dir = workspace(&[("note", "original")]);
    let outside = workspace(&[("external", "external")]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Resolve) {
            hook_gate.wait();
        }
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    session
        .dispatch(AppCommand::OpenDocument(id("note")))
        .unwrap();
    let start = Instant::now();
    session
        .dispatch(AppCommand::OpenFiles(vec![outside.path().join("external")]))
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    assert!(!session.snapshot().activity.editing_paused);
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("note"),
            content: "typed while opening".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::SaveDocument(id("note")))
        .unwrap();
    gate.open();
    let snapshot = settle(&mut session);
    assert_eq!(
        snapshot.active_id(),
        Some(&external(&outside.path().join("external")))
    );
    assert!(snapshot.is_open(&id("note")));
    assert_eq!(
        fs::read_to_string(dir.path().join("note")).unwrap(),
        "typed while opening"
    );
}

#[test]
fn os_drop_validation_rejects_duplicate_targets_and_unsafe_directories() {
    let dir = workspace(&[("dest/.keep", "")]);
    let outside = workspace(&[("note", "source")]);
    let session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    let path = outside.path().join("note");
    assert!(session.can_move_files(std::slice::from_ref(&path), "dest"));
    for paths in [
        vec![],
        vec![path.clone(), path.clone()],
        vec!["relative".into()],
    ] {
        assert!(!session.can_move_files(&paths, "dest"));
    }
    for directory in ["../escape", "missing", "/tmp", "dest/.keep"] {
        assert!(!session.can_move_files(std::slice::from_ref(&path), directory));
    }
}

#[test]
fn workspace_entry_commands_cannot_mutate_external_tabs() {
    let dir = workspace(&[]);
    let outside = workspace(&[("note", "keep")]);
    let path = outside.path().join("note");
    let document = external(&path);
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenFile(path.clone())).unwrap();
    for command in [
        AppCommand::CreateEntry {
            id: document.clone(),
            directory: false,
        },
        AppCommand::DeleteEntry(document.clone()),
        AppCommand::RenameEntry {
            id: document.clone(),
            new_name: "new".into(),
        },
        AppCommand::MoveEntry {
            id: document.clone(),
            directory: String::new(),
        },
    ] {
        assert!(matches!(
            app.dispatch(command),
            Err(AppError::InvalidEntryName)
        ));
    }
    assert_eq!(fs::read_to_string(path).unwrap(), "keep");
    assert!(app.snapshot().is_open(&document));
}
