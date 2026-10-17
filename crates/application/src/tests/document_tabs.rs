use super::*;
fn fixture() -> (tempfile::TempDir, Application) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        fs::write(dir.path().join(name), format!("original {name}")).unwrap();
    }
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    for name in ["b", "a", "c"] {
        app.dispatch(AppCommand::OpenDocument(id(name))).unwrap();
    }
    (dir, app)
}

fn preview_fixture() -> (tempfile::TempDir, Application) {
    let (dir, mut app) = fixture();
    for name in ["a", "b", "c"] {
        app.dispatch(AppCommand::CloseDocument(id(name))).unwrap();
    }
    (dir, app)
}

#[test]
fn preview_replaces_only_the_previous_preview_and_releases_its_buffer() {
    let (dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    let next = app.dispatch(AppCommand::PreviewDocument(id("b"))).unwrap();
    assert_eq!(next.open_documents, [id("b")]);
    assert_eq!(next.preview_document, Some(id("b")));
    fs::write(dir.path().join("a"), "new disk a").unwrap();
    let next = app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    assert_eq!(next.open_documents, [id("a")]);
    assert_eq!(&*next.active_document.unwrap().content, "new disk a");
}

#[test]
fn selecting_an_existing_tab_keeps_both_the_permanent_tab_and_the_preview() {
    let (_dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::OpenDocument(id("c"))).unwrap();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    let selected = app.dispatch(AppCommand::PreviewDocument(id("c"))).unwrap();
    assert_eq!(selected.open_documents, [id("c"), id("a")]);
    assert_eq!(selected.preview_document, Some(id("a")));
    let next = app.dispatch(AppCommand::PreviewDocument(id("b"))).unwrap();
    assert_eq!(next.open_documents, [id("c"), id("b")]);
    assert_eq!(next.preview_document, Some(id("b")));
}

#[test]
fn editing_a_preview_promotes_it_even_after_undo_and_save() {
    let (_dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a"),
        content: "edited".into(),
    })
    .unwrap();
    assert!(app.snapshot().preview_document.is_none());
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a"),
        content: "original a".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::SaveDocument(id("a"))).unwrap();
    let next = app.dispatch(AppCommand::PreviewDocument(id("b"))).unwrap();
    assert_eq!(next.open_documents, [id("a"), id("b")]);
    assert_eq!(next.preview_document, Some(id("b")));
}

#[test]
fn keep_open_and_explicit_open_promote_without_changing_the_selected_editor() {
    let (_dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    app.dispatch(AppCommand::OpenDocument(id("b"))).unwrap();
    let kept = app.dispatch(AppCommand::KeepDocumentOpen(id("a"))).unwrap();
    assert!(kept.preview_document.is_none());
    assert_eq!(kept.active_document.unwrap().id, id("b"));
    app.dispatch(AppCommand::PreviewDocument(id("c"))).unwrap();
    let kept = app.dispatch(AppCommand::OpenDocument(id("c"))).unwrap();
    assert!(kept.preview_document.is_none());
    assert_eq!(kept.open_documents, [id("a"), id("b"), id("c")]);
}

#[test]
fn failed_preview_open_preserves_the_previous_preview() {
    let (dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    fs::remove_file(dir.path().join("b")).unwrap();
    for target in ["b", "missing"] {
        assert!(
            app.dispatch(AppCommand::PreviewDocument(id(target)))
                .is_err()
        );
        assert_eq!(app.snapshot().preview_document, Some(id("a")));
        assert_eq!(app.snapshot().open_documents, [id("a")]);
        assert_eq!(app.snapshot().active_document.unwrap().id, id("a"));
    }
}

#[test]
fn preview_follows_renames_and_is_promoted_by_tab_reordering() {
    let (_dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::OpenDocument(id("c"))).unwrap();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    let renamed = app
        .dispatch(AppCommand::RenameEntry {
            id: id("a"),
            new_name: "renamed".into(),
        })
        .unwrap();
    assert_eq!(renamed.preview_document, Some(id("renamed")));
    let moved = app
        .dispatch(AppCommand::MoveDocumentTab {
            id: id("renamed"),
            index: 0,
        })
        .unwrap();
    assert!(moved.preview_document.is_none());
    let next = app.dispatch(AppCommand::PreviewDocument(id("b"))).unwrap();
    assert_eq!(next.open_documents, [id("renamed"), id("b"), id("c")]);
}

#[test]
fn save_promotes_a_clean_preview_but_save_all_leaves_it_temporary() {
    let (_dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    let saved = app.dispatch(AppCommand::SaveAllDocuments).unwrap();
    assert_eq!(saved.preview_document, Some(id("a")));
    let saved = app.dispatch(AppCommand::SaveDocument(id("a"))).unwrap();
    assert!(saved.preview_document.is_none());
}

#[test]
fn preview_is_cleared_by_close_delete_and_workspace_switch() {
    let (dir, mut app) = preview_fixture();
    app.dispatch(AppCommand::PreviewDocument(id("a"))).unwrap();
    assert!(
        app.dispatch(AppCommand::CloseDocument(id("a")))
            .unwrap()
            .preview_document
            .is_none()
    );
    app.dispatch(AppCommand::PreviewDocument(id("b"))).unwrap();
    assert!(
        app.dispatch(AppCommand::DeleteEntry(id("b")))
            .unwrap()
            .preview_document
            .is_none()
    );
    app.dispatch(AppCommand::PreviewDocument(id("c"))).unwrap();
    assert!(
        app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
            .unwrap()
            .preview_document
            .is_none()
    );
}

#[test]
fn tabs_preserve_open_order_and_close_inactive_or_neighbor_without_losing_buffers() {
    let (_dir, mut app) = fixture();
    app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    assert_eq!(app.snapshot().open_documents, [id("b"), id("a"), id("c")]);
    app.dispatch(AppCommand::ChangeDocument {
        id: id("b"),
        content: "unsaved b".into(),
    })
    .unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::CloseDocument(id("b"))),
        Err(AppError::UnsavedChanges)
    ));
    app.dispatch(AppCommand::CloseDocument(id("a"))).unwrap();
    assert_eq!(app.snapshot().active_document.unwrap().id, id("c"));
    app.dispatch(AppCommand::CloseDocument(id("c"))).unwrap();
    assert_eq!(
        &*app.snapshot().active_document.unwrap().content,
        "unsaved b"
    );
    app.dispatch(AppCommand::DiscardAndCloseDocument(id("b")))
        .unwrap();
    assert!(app.snapshot().open_documents.is_empty());
    assert!(app.snapshot().active_document.is_none());
    let reopened = app.dispatch(AppCommand::OpenDocument(id("b"))).unwrap();
    assert_eq!(&*reopened.active_document.unwrap().content, "original b");
}

#[test]
fn save_all_saves_inactive_buffers_and_keeps_successful_saves_on_conflict() {
    let (dir, mut app) = fixture();
    for name in ["b", "a", "c"] {
        app.dispatch(AppCommand::ChangeDocument {
            id: id(name),
            content: format!("edited {name}"),
        })
        .unwrap();
    }
    fs::write(dir.path().join("a"), "external a").unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::SaveAllDocuments),
        Err(AppError::ExternalChanges(_))
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("b")).unwrap(),
        "edited b"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("a")).unwrap(),
        "external a"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("c")).unwrap(),
        "original c"
    );
    assert!(
        !app.snapshot()
            .documents
            .iter()
            .find(|d| d.id == id("b"))
            .unwrap()
            .dirty
    );
    app.dispatch(AppCommand::OverwriteDocument(id("a")))
        .unwrap();
    let saved = app.dispatch(AppCommand::SaveAllDocuments).unwrap();
    assert!(saved.documents.iter().all(|d| !d.dirty));
    assert_eq!(saved.active_document.unwrap().id, id("c"));
    assert_eq!(
        fs::read_to_string(dir.path().join("c")).unwrap(),
        "edited c"
    );
}

#[test]
fn closed_tabs_reload_disk_and_renamed_tabs_keep_order() {
    let (dir, mut app) = fixture();
    app.dispatch(AppCommand::RenameEntry {
        id: id("b"),
        new_name: "renamed".into(),
    })
    .unwrap();
    assert_eq!(
        app.snapshot().open_documents,
        [id("renamed"), id("a"), id("c")]
    );
    app.dispatch(AppCommand::CloseDocument(id("a"))).unwrap();
    assert_eq!(app.snapshot().active_document.unwrap().id, id("c"));
    fs::write(dir.path().join("a"), "new disk a").unwrap();
    let reopened = app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    assert_eq!(&*reopened.active_document.unwrap().content, "new disk a");
    assert_eq!(reopened.open_documents, [id("renamed"), id("c"), id("a")]);
}

#[test]
fn session_publishes_completed_saves_when_save_all_fails() {
    let (dir, mut app) = fixture();
    for name in ["b", "a"] {
        app.dispatch(AppCommand::ChangeDocument {
            id: id(name),
            content: format!("edited {name}"),
        })
        .unwrap();
    }
    fs::write(dir.path().join("a"), "external a").unwrap();
    let mut session = Session::with_application(app);
    assert!(matches!(
        session.dispatch(AppCommand::SaveAllDocuments),
        Err(AppError::ExternalChanges(_))
    ));
    let snapshot = session.snapshot();
    assert!(snapshot.error.is_some());
    assert!(
        !snapshot
            .documents
            .iter()
            .find(|d| d.id == id("b"))
            .unwrap()
            .dirty
    );
    assert!(
        snapshot
            .documents
            .iter()
            .find(|d| d.id == id("a"))
            .unwrap()
            .dirty
    );
    assert_eq!(snapshot.open_documents, [id("b"), id("a"), id("c")]);
}

#[test]
fn moving_native_tabs_keeps_buffers_and_selects_the_visual_neighbor_on_close() {
    let (_dir, mut app) = fixture();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a"),
        content: "unsaved a".into(),
    })
    .unwrap();
    let before = app.snapshot();
    let moved = outcome(
        &mut app,
        AppCommand::MoveDocumentTab {
            id: id("c"),
            index: 0,
        },
    )
    .unwrap();
    assert_eq!(
        moved.0,
        [AppEvent::DocumentTabMoved {
            id: id("c"),
            index: 0
        }]
    );
    assert_eq!(moved.1.open_documents, [id("c"), id("b"), id("a")]);
    assert!(Arc::ptr_eq(&before.documents, &moved.1.documents));
    assert_eq!(moved.1.active_document.unwrap().id, id("c"));
    let closed = app.dispatch(AppCommand::CloseDocument(id("c"))).unwrap();
    assert_eq!(closed.active_document.unwrap().id, id("b"));
    assert!(
        closed
            .documents
            .iter()
            .find(|doc| doc.id == id("a"))
            .unwrap()
            .dirty
    );
}

#[test]
fn save_and_close_closes_only_after_a_successful_save() {
    let (dir, mut app) = fixture();
    for name in ["a", "c"] {
        app.dispatch(AppCommand::ChangeDocument {
            id: id(name),
            content: format!("edited {name}"),
        })
        .unwrap();
    }
    let closed = app
        .dispatch(AppCommand::SaveAndCloseDocument(id("a")))
        .unwrap();
    assert_eq!(closed.open_documents, [id("b"), id("c")]);
    assert_eq!(
        fs::read_to_string(dir.path().join("a")).unwrap(),
        "edited a"
    );
    fs::write(dir.path().join("c"), "external c").unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::SaveAndCloseDocument(id("c"))),
        Err(AppError::ExternalChanges(_))
    ));
    assert_eq!(app.snapshot().open_documents, [id("b"), id("c")]);
    assert!(app.snapshot().is_dirty(&id("c")));
}

#[test]
fn editor_generation_changes_on_switches_but_not_on_renames() {
    let (_dir, mut app) = fixture();
    let shown = app.snapshot().editor_generation;
    let renamed = app
        .dispatch(AppCommand::RenameEntry {
            id: id("c"),
            new_name: "renamed".into(),
        })
        .unwrap();
    assert_eq!(renamed.active_id(), Some(&id("renamed")));
    assert_eq!(renamed.editor_generation, shown);
    let switched = app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    assert_ne!(switched.editor_generation, shown);
    let typed = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("a"),
            content: "typed".into(),
        })
        .unwrap();
    assert_eq!(typed.editor_generation, switched.editor_generation);
}
