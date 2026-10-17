use super::*;
use native_workspace_storage::SqliteStorage;

#[test]
fn creates_files_and_folders_without_overwriting_existing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let snapshot = app
        .dispatch(AppCommand::CreateEntry {
            id: id("notes"),
            directory: true,
        })
        .unwrap();
    assert_eq!(snapshot.directories[..], ["notes"]);
    let snapshot = app
        .dispatch(AppCommand::CreateEntry {
            id: id("notes/todo.md"),
            directory: false,
        })
        .unwrap();
    assert!(
        snapshot
            .documents
            .iter()
            .any(|entry| entry.id == id("notes/todo.md"))
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("notes/todo.md")).unwrap(),
        ""
    );
    fs::write(dir.path().join("notes/todo.md"), "keep me").unwrap();
    assert!(
        app.dispatch(AppCommand::CreateEntry {
            id: id("notes/todo.md"),
            directory: false,
        })
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("notes/todo.md")).unwrap(),
        "keep me"
    );
}

#[test]
fn workspace_edit_save_round_trip_and_dirty_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "original").unwrap();
    let mut app = Application::new();
    let listed = app
        .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    assert_eq!(listed.documents.len(), 1);
    assert_eq!(listed.documents[0].id, id("note.md"));
    let opened = app
        .dispatch(AppCommand::OpenDocument(id("note.md")))
        .unwrap();
    assert_eq!(&*opened.active_document.unwrap().content, "original");
    let changed = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "edited".into(),
        })
        .unwrap();
    assert!(changed.documents[0].dirty);
    assert!(changed.active_document.unwrap().dirty);
    let saved = app
        .dispatch(AppCommand::SaveDocument(id("note.md")))
        .unwrap();
    assert!(!saved.documents[0].dirty);
    assert_eq!(
        fs::read_to_string(dir.path().join("note.md")).unwrap(),
        "edited"
    );
}

#[test]
fn refresh_updates_clean_documents_and_workspace_search() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "old text").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("note.md")))
        .unwrap();
    search(&mut app, "new");
    fs::write(dir.path().join("note.md"), "new text").unwrap();
    fs::write(dir.path().join("added.txt"), "new file").unwrap();
    let refreshed = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    assert_eq!(&*refreshed.active_document.unwrap().content, "new text");
    assert_eq!(refreshed.documents.len(), 2);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
        if snapshot.search_results.len() == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background search did not finish"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let revision = app.snapshot().revision;
    assert_eq!(
        app.dispatch(AppCommand::RefreshWorkspace).unwrap().revision,
        revision
    );
}

#[test]
fn watcher_refreshes_workspace_after_external_change() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    let path = dir.path().join("nested/note.md");
    fs::write(&path, "before").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    assert!(app.watcher.is_some(), "native watcher should start");
    app.dispatch(AppCommand::OpenDocument(id("nested/note.md")))
        .unwrap();
    let initial_revision = app.snapshot().revision;
    assert_eq!(
        app.dispatch(AppCommand::PollWorkspaceChanges)
            .unwrap()
            .revision,
        initial_revision
    );
    fs::write(&path, "after").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
        if snapshot.revision != initial_revision {
            assert_eq!(&*snapshot.active_document.unwrap().content, "after");
            break;
        }
        assert!(Instant::now() < deadline, "watcher did not report the edit");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn queued_search_discards_stale_queries_in_nested_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("nested/note.rs"), "alpha beta beta").unwrap();
    let mut app = Application::new();
    let opened = app
        .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    assert_eq!(opened.directories[..], ["empty", "nested"]);
    assert_eq!(opened.documents[0].id, id("nested/note.rs"));
    app.dispatch(AppCommand::QueueSearchQuery("alpha".into()))
        .unwrap();
    let queued = app
        .dispatch(AppCommand::QueueSearchQuery("beta".into()))
        .unwrap();
    assert!(queued.search_in_progress);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
        if !snapshot.search_in_progress {
            assert_eq!(snapshot.search_query, "beta");
            assert_eq!(snapshot.search_results.len(), 2);
            assert!(
                snapshot
                    .search_results
                    .iter()
                    .all(|result| result.id == id("nested/note.rs"))
            );
            break;
        }
        assert!(Instant::now() < deadline, "queued search did not finish");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn typing_a_query_leaves_searching_and_unchanged_lists_to_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("alpha.md"), "beta").unwrap();
    fs::write(dir.path().join("other.md"), "alpha beta").unwrap();
    let mut app = Application::new();
    let opened = app
        .dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let queued = app
        .dispatch(AppCommand::QueueSearchQuery("alpha".into()))
        .unwrap();
    assert!(queued.search_in_progress);
    assert!(
        queued.search_results.is_empty(),
        "no scan before the worker"
    );
    assert!(Arc::ptr_eq(&opened.documents, &queued.documents));
    assert!(Arc::ptr_eq(&opened.directories, &queued.directories));
    let deadline = Instant::now() + Duration::from_secs(5);
    let found = loop {
        let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
        if !snapshot.search_in_progress {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "queued search did not finish");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(found.search_results.len(), 1, "content matches only");
    assert_eq!(found.search_results[0].id, id("other.md"));
    let refined = app
        .dispatch(AppCommand::QueueSearchQuery("alpha b".into()))
        .unwrap();
    assert!(refined.search_in_progress);
    assert!(
        refined.search_results.is_empty(),
        "stale results are cleared"
    );
    let cleared = app
        .dispatch(AppCommand::QueueSearchQuery(String::new()))
        .unwrap();
    assert!(!cleared.search_in_progress);
    assert!(
        cleared.search_results.is_empty(),
        "an empty query lists nothing"
    );
}

#[test]
fn external_edits_block_save_until_reload_or_explicit_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.md");
    fs::write(&path, "base").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("note.md")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("note.md"),
        content: "local".into(),
    })
    .unwrap();
    fs::write(&path, "external").unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::SaveDocument(id("note.md"))),
        Err(AppError::ExternalChanges(_))
    ));
    let conflicted = app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let active = conflicted.active_document.unwrap();
    assert_eq!(&*active.content, "local");
    assert!(active.dirty && active.external_change);
    app.dispatch(AppCommand::ReloadDocument(id("note.md")))
        .unwrap();
    assert_eq!(
        &*app.snapshot().active_document.unwrap().content,
        "external"
    );
    app.dispatch(AppCommand::ChangeDocument {
        id: id("note.md"),
        content: "local again".into(),
    })
    .unwrap();
    fs::remove_file(&path).unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    assert!(app.snapshot().active_document.unwrap().external_change);
    app.dispatch(AppCommand::OverwriteDocument(id("note.md")))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "local again");
    assert!(!app.snapshot().active_document.unwrap().external_change);
}

#[test]
fn unsaved_content_survives_switch_and_blocks_workspace_replacement() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "a").unwrap();
    fs::write(dir.path().join("b.md"), "b").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a.md"),
        content: "edited".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("b.md"))).unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf())),
        Err(AppError::UnsavedChanges)
    ));
    let reopened = app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
    assert_eq!(&*reopened.active_document.unwrap().content, "edited");
    assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "a");
}

#[test]
fn invalid_operation_preserves_revision() {
    let mut app = Application::new();
    assert!(matches!(
        app.dispatch(AppCommand::OpenDocument(id("missing.md"))),
        Err(AppError::WorkspaceNotOpen)
    ));
    assert_eq!(app.snapshot().revision, 0);
}

#[test]
fn text_search_lists_content_matches_only_and_keeps_open_document() {
    let dir = tempfile::tempdir().unwrap();
    // A file name match alone is not a text match.
    fs::write(dir.path().join("Alpha.md"), "one alpha").unwrap();
    fs::write(dir.path().join("alpine.md"), "two").unwrap();
    fs::write(dir.path().join("beta.txt"), "two").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("beta.txt")))
        .unwrap();
    let (events, _) = outcome(&mut app, AppCommand::QueueSearchQuery("  ALP  ".into())).unwrap();
    assert_eq!(
        events,
        [AppEvent::SearchQueryQueued {
            query: "  ALP  ".into()
        }]
    );
    let found = search(&mut app, "  ALP  ");
    assert_eq!(found.search_results.len(), 1);
    assert_eq!(found.search_results[0].id, id("Alpha.md"));
    assert_eq!(found.active_document.unwrap().id, id("beta.txt"));
    let revision = found.revision;
    let unchanged = app
        .dispatch(AppCommand::QueueSearchQuery("  ALP  ".into()))
        .unwrap();
    assert_eq!(unchanged.revision, revision);
    let cleared = app
        .dispatch(AppCommand::QueueSearchQuery(String::new()))
        .unwrap();
    assert!(cleared.search_results.is_empty() && !cleared.search_in_progress);
}

#[test]
fn opening_workspace_resets_search_query() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    fs::write(first.path().join("first.md"), "one").unwrap();
    fs::write(second.path().join("second.md"), "two").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(first.path().to_path_buf()))
        .unwrap();
    search(&mut app, "first");
    let snapshot = app
        .dispatch(AppCommand::OpenWorkspace(second.path().to_path_buf()))
        .unwrap();
    assert_eq!(snapshot.search_query, "");
    assert!(snapshot.search_results.is_empty());
}

#[test]
fn content_search_finds_unopened_files_and_returns_matching_line() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("alpha.md"),
        "heading\n  A Hidden Phrase here\nend",
    )
    .unwrap();
    fs::write(dir.path().join("beta.txt"), "unrelated").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let found = search(&mut app, "  hidden phrase  ");
    assert_eq!(found.search_results.len(), 1);
    assert_eq!(found.search_results[0].id, id("alpha.md"));
    assert_eq!(found.search_results[0].snippet, ("A Hidden Phrase here"));
    assert!(found.active_document.is_none());
}

#[test]
fn content_preview_keeps_match_visible_in_long_line() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("note.md"),
        format!("{}needle", "x".repeat(300)),
    )
    .unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let found = search(&mut app, "needle");
    assert!(found.search_results[0].snippet.contains("needle"));
}

#[test]
fn search_returns_each_occurrence_with_editor_offsets() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "A 💡 café café\ncafé").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let found = search(&mut app, "CAFÉ");
    assert_eq!(found.search_results.len(), 3);
    let locations: Vec<_> = found
        .search_results
        .iter()
        .map(|result| result.location.clone())
        .collect();
    assert_eq!(
        (
            locations[0].line,
            locations[0].column,
            locations[0].start_char,
            locations[0].start_utf16
        ),
        (1, 5, 4, 5)
    );
    assert_eq!(
        (
            locations[1].line,
            locations[1].column,
            locations[1].start_char,
            locations[1].start_utf16
        ),
        (1, 10, 9, 10)
    );
    assert_eq!(
        (
            locations[2].line,
            locations[2].column,
            locations[2].start_char,
            locations[2].start_utf16
        ),
        (2, 1, 14, 15)
    );
    assert_eq!(locations[2].end_utf16, 19);
}

#[test]
fn content_occurrences_remain_individually_selectable() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "note note").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let found = search(&mut app, "note");
    assert_eq!(found.search_results.len(), 2);
    assert_eq!(found.search_results[0].location.start_char, 0);
    assert_eq!(found.search_results[1].location.start_char, 5);
}

#[test]
fn overlapping_text_occurrences_are_both_listed() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("text.md"), "banana").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    let found = search(&mut app, "ana");
    assert_eq!(found.search_results.len(), 2);
    assert_eq!(found.search_results[0].location.start_char, 1);
    assert_eq!(found.search_results[1].location.start_char, 3);
}

#[test]
fn content_search_tracks_unsaved_edits_and_saved_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "original").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("note.md")))
        .unwrap();
    search(&mut app, "draft");
    let changed = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "a draft line".into(),
        })
        .unwrap();
    assert_eq!(changed.search_results[0].snippet, ("a draft line"));
    let saved = app
        .dispatch(AppCommand::SaveDocument(id("note.md")))
        .unwrap();
    assert_eq!(saved.search_results.len(), 1, "saving keeps the matches");
    let changed_again = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "without that word".into(),
        })
        .unwrap();
    assert!(changed_again.search_results.is_empty());
}

#[test]
fn recent_workspaces_survive_application_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("metadata.sqlite3");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let mut app = Application::new();
    app.enable_persistence(SqliteStorage::open(&db).unwrap())
        .unwrap();
    assert_eq!(app.revision(), 1);
    app.dispatch(AppCommand::OpenWorkspace(first.clone()))
        .unwrap();
    app.dispatch(AppCommand::OpenWorkspace(second.clone()))
        .unwrap();
    let snapshot = app
        .dispatch(AppCommand::OpenWorkspace(first.clone()))
        .unwrap();
    assert_eq!(snapshot.recent_workspaces.len(), 2);
    assert_eq!(
        snapshot.recent_workspaces[0].root,
        fs::canonicalize(first).unwrap()
    );
    drop(app);

    let mut reopened = Application::new();
    reopened
        .enable_persistence(SqliteStorage::open(&db).unwrap())
        .unwrap();
    let restored = reopened.snapshot();
    assert_eq!(restored.revision, 1);
    assert!(restored.workspace.is_none());
    assert_eq!(restored.recent_workspaces, snapshot.recent_workspaces);
}

#[test]
fn events_follow_successful_changes_and_keep_one_revision_per_command() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "original").unwrap();
    let mut app = Application::new();

    let listed = outcome(
        &mut app,
        AppCommand::OpenWorkspace(dir.path().to_path_buf()),
    )
    .unwrap();
    assert!(matches!(
        listed.0.as_slice(),
        [AppEvent::WorkspaceOpened { .. }]
    ));
    assert_eq!(listed.1.revision, 1);

    let opened = outcome(&mut app, AppCommand::OpenDocument(id("note.md"))).unwrap();
    assert_eq!(
        opened.0,
        [
            AppEvent::DocumentLoaded {
                id: id("note.md"),
                content: "original".into()
            },
            AppEvent::DocumentOpened { id: id("note.md") }
        ]
    );
    assert_eq!(opened.1.revision, 2);
    assert_eq!(app.state().snapshot(), opened.1);

    let no_op = outcome(&mut app, AppCommand::OpenDocument(id("note.md"))).unwrap();
    assert!(no_op.0.is_empty());
    assert_eq!(no_op.1.revision, 2);

    let changed = outcome(
        &mut app,
        AppCommand::ChangeDocument {
            id: id("note.md"),
            content: "edited".into(),
        },
    )
    .unwrap();
    assert_eq!(
        changed.0,
        [AppEvent::DocumentChanged {
            id: id("note.md"),
            content: "edited".into()
        }]
    );
    assert!(changed.1.documents[0].dirty);

    let saved = outcome(&mut app, AppCommand::SaveDocument(id("note.md"))).unwrap();
    assert_eq!(saved.0, [AppEvent::DocumentSaved { id: id("note.md") }]);
    assert!(!saved.1.documents[0].dirty);
    assert_eq!(
        fs::read_to_string(dir.path().join("note.md")).unwrap(),
        "edited"
    );

    let already_saved = outcome(&mut app, AppCommand::SaveDocument(id("note.md"))).unwrap();
    assert!(already_saved.0.is_empty());
    assert_eq!(already_saved.1.revision, saved.1.revision);

    let closed = outcome(&mut app, AppCommand::CloseDocument(id("note.md"))).unwrap();
    assert_eq!(closed.0, [AppEvent::DocumentClosed { id: id("note.md") }]);
    assert!(closed.1.active_document.is_none());
}

#[test]
fn move_folder_preserves_open_buffers_and_can_move_back_to_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::create_dir(dir.path().join("destination")).unwrap();
    fs::write(dir.path().join("src/nested/a.rs"), "original").unwrap();
    fs::write(dir.path().join("src/b.rs"), "second").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    for path in ["src/b.rs", "src/nested/a.rs"] {
        app.dispatch(AppCommand::OpenDocument(id(path))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id(path),
            content: "local needle".into(),
        })
        .unwrap();
    }
    search(&mut app, "needle");
    let moved = outcome(
        &mut app,
        AppCommand::MoveEntry {
            id: id("src"),
            directory: "destination".into(),
        },
    )
    .unwrap();
    assert!(
        matches!(&moved.0[0], AppEvent::EntryRelocated { id: source, target } if source.as_str() == "src" && target.as_str() == "destination/src")
    );
    let active = moved.1.active_document.unwrap();
    assert_eq!(active.id, id("destination/src/nested/a.rs"));
    assert_eq!(&*active.content, "local needle");
    assert!(active.dirty);
    assert_eq!(
        moved.1.directories[..],
        ["destination", "destination/src", "destination/src/nested"]
    );
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    // Also exercise a file move, preserving its base name, into the root.
    app.dispatch(AppCommand::MoveEntry {
        id: id("destination/src/b.rs"),
        directory: String::new(),
    })
    .unwrap();
    app.dispatch(AppCommand::SaveDocument(id("b.rs"))).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("b.rs")).unwrap(),
        "local needle"
    );
    app.dispatch(AppCommand::MoveEntry {
        id: id("destination/src"),
        directory: String::new(),
    })
    .unwrap();
    app.dispatch(AppCommand::SaveDocument(id("src/nested/a.rs")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("src/nested/a.rs")).unwrap(),
        "local needle"
    );
    assert!(!dir.path().join("destination/src").exists());
    assert!(
        app.snapshot()
            .search_results
            .iter()
            .all(|result| !result.id.as_str().starts_with("destination/"))
    );
}

#[test]
fn move_rejects_descendants_collisions_and_invalid_destinations_atomically() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::create_dir_all(dir.path().join("occupied/src")).unwrap();
    fs::write(dir.path().join("src/a.md"), "original").unwrap();
    fs::write(dir.path().join("file"), "not a directory").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("src/a.md")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("src/a.md"),
        content: "local".into(),
    })
    .unwrap();
    let before = app.snapshot();
    for directory in [
        "src",
        "src/nested",
        "occupied",
        "missing",
        "file",
        "..",
        "../outside",
        "/tmp",
        "a//b",
    ] {
        assert!(
            app.dispatch(AppCommand::MoveEntry {
                id: id("src"),
                directory: directory.into()
            })
            .is_err(),
            "{directory}"
        );
        assert_eq!(app.snapshot(), before);
    }
    let unchanged = outcome(
        &mut app,
        AppCommand::MoveEntry {
            id: id("src"),
            directory: String::new(),
        },
    )
    .unwrap();
    assert!(unchanged.0.is_empty());
    assert_eq!(unchanged.1, before);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.md")).unwrap(),
        "original"
    );
    assert!(
        app.dispatch(AppCommand::MoveEntry {
            id: id("nonexistent"),
            directory: String::new()
        })
        .is_err()
    );
    assert_eq!(app.snapshot(), before);
}

#[test]
fn move_protects_conflicts_and_missing_dirty_destination_buffers() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("dest")).unwrap();
    fs::write(dir.path().join("a.md"), "original").unwrap();
    fs::write(dir.path().join("dest/a.md"), "destination").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("dest/a.md")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("dest/a.md"),
        content: "local destination".into(),
    })
    .unwrap();
    fs::remove_file(dir.path().join("dest/a.md")).unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let before = app.snapshot();
    assert!(
        app.dispatch(AppCommand::MoveEntry {
            id: id("a.md"),
            directory: "dest".into()
        })
        .is_err()
    );
    assert_eq!(app.snapshot(), before);
    app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a.md"),
        content: "local source".into(),
    })
    .unwrap();
    fs::write(dir.path().join("a.md"), "external source").unwrap();
    fs::create_dir(dir.path().join("other")).unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let moved = app
        .dispatch(AppCommand::MoveEntry {
            id: id("a.md"),
            directory: "other".into(),
        })
        .unwrap();
    let active = moved.active_document.unwrap();
    assert_eq!(active.id, id("other/a.md"));
    assert_eq!(&*active.content, "local source");
    assert!(active.external_change);
    assert!(matches!(
        app.dispatch(AppCommand::SaveDocument(id("other/a.md"))),
        Err(AppError::ExternalChanges(_))
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("other/a.md")).unwrap(),
        "external source"
    );
}

#[test]
fn renaming_folder_preserves_all_open_buffers_and_search_paths() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::create_dir(dir.path().join("src-other")).unwrap();
    fs::write(dir.path().join("src/nested/a.rs"), "original").unwrap();
    fs::write(dir.path().join("src/b.rs"), "second").unwrap();
    fs::write(dir.path().join("src-other/c.rs"), "untouched").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    for path in ["src/b.rs", "src/nested/a.rs"] {
        app.dispatch(AppCommand::OpenDocument(id(path))).unwrap();
        app.dispatch(AppCommand::ChangeDocument {
            id: id(path),
            content: "local needle".into(),
        })
        .unwrap();
    }
    search(&mut app, "needle");
    let snapshot = app
        .dispatch(AppCommand::RenameEntry {
            id: id("src"),
            new_name: "lib".into(),
        })
        .unwrap();
    let active = snapshot.active_document.unwrap();
    assert_eq!(active.id, id("lib/nested/a.rs"));
    assert_eq!(&*active.content, "local needle");
    assert!(active.dirty);
    assert_eq!(snapshot.directories[..], ["lib", "lib/nested", "src-other"]);
    assert!(
        snapshot
            .search_results
            .iter()
            .all(|result| result.id.as_str().starts_with("lib/"))
    );
    assert!(!dir.path().join("src").exists());
    // Watcher reconciliation must not lose either unsaved buffer.
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    for path in ["lib/b.rs", "lib/nested/a.rs"] {
        assert_eq!(
            &*app
                .dispatch(AppCommand::OpenDocument(id(path)))
                .unwrap()
                .active_document
                .unwrap()
                .content,
            "local needle"
        );
        app.dispatch(AppCommand::SaveDocument(id(path))).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(path)).unwrap(),
            "local needle"
        );
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("src-other/c.rs")).unwrap(),
        "untouched"
    );
}

#[test]
fn rename_rejects_collisions_and_invalid_names_without_changing_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "a").unwrap();
    fs::write(dir.path().join("b.md"), "b").unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
    let before = app.snapshot();
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "nested/new",
        "a\\b",
        "b.md",
        "folder",
    ] {
        assert!(
            app.dispatch(AppCommand::RenameEntry {
                id: id("a.md"),
                new_name: name.into()
            })
            .is_err(),
            "{name}"
        );
        assert_eq!(app.snapshot(), before);
    }
    let unchanged = outcome(
        &mut app,
        AppCommand::RenameEntry {
            id: id("a.md"),
            new_name: "a.md".into(),
        },
    )
    .unwrap();
    assert!(unchanged.0.is_empty());
    assert_eq!(unchanged.1, before);
    assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "a");
    assert_eq!(fs::read_to_string(dir.path().join("b.md")).unwrap(), "b");
    // Collision created after the last snapshot is still rejected by the filesystem.
    fs::write(dir.path().join("new.md"), "external").unwrap();
    assert!(
        app.dispatch(AppCommand::RenameEntry {
            id: id("a.md"),
            new_name: "new.md".into()
        })
        .is_err()
    );
    assert_eq!(app.snapshot(), before);
    fs::remove_file(dir.path().join("a.md")).unwrap();
    assert!(
        app.dispatch(AppCommand::RenameEntry {
            id: id("a.md"),
            new_name: "renamed.md".into()
        })
        .is_err()
    );
    assert_eq!(app.snapshot(), before);
}

#[test]
fn rename_keeps_external_conflict_and_protects_missing_dirty_destination() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "original").unwrap();
    fs::write(dir.path().join("b.md"), "second").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("a.md"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a.md"),
        content: "local".into(),
    })
    .unwrap();
    fs::write(dir.path().join("a.md"), "external").unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let renamed = app
        .dispatch(AppCommand::RenameEntry {
            id: id("a.md"),
            new_name: "renamed.md".into(),
        })
        .unwrap();
    assert!(renamed.active_document.unwrap().external_change);
    assert!(matches!(
        app.dispatch(AppCommand::SaveDocument(id("renamed.md"))),
        Err(AppError::ExternalChanges(_))
    ));
    fs::remove_file(dir.path().join("renamed.md")).unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let before = app.snapshot();
    assert!(
        app.dispatch(AppCommand::RenameEntry {
            id: id("b.md"),
            new_name: "renamed.md".into()
        })
        .is_err()
    );
    assert_eq!(app.snapshot(), before);
}

#[test]
fn failed_save_keeps_dirty_state_and_does_not_advance_revision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "original").unwrap();
    let mut app =
        Application::with_filesystem(Box::new(TestFilesystem::with(
            |operation| match operation {
                Operation::Write(_) => Err(io::Error::other("write failed")),
                _ => Ok(()),
            },
        )));
    app.dispatch(AppCommand::OpenWorkspace(dir.path().to_path_buf()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("note.md")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("note.md"),
        content: "edited".into(),
    })
    .unwrap();
    let before = app.snapshot();

    assert!(matches!(
        outcome(&mut app, AppCommand::SaveDocument(id("note.md"))),
        Err(AppError::Filesystem(_))
    ));
    assert_eq!(app.snapshot(), before);
    assert_eq!(
        fs::read_to_string(dir.path().join("note.md")).unwrap(),
        "original"
    );
}

#[test]
fn relocation_protects_missing_destination_descendants_after_a_similar_sibling() {
    let dir = workspace(&[
        ("src/a", "source"),
        ("dest/orphan", "unsaved"),
        ("dest-before", "sibling"),
    ]);
    let mut app = open(dir.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenDocument(id("dest/orphan")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("dest/orphan"),
        content: "keep this buffer".into(),
    })
    .unwrap();
    fs::remove_file(dir.path().join("dest/orphan")).unwrap();
    fs::remove_dir(dir.path().join("dest")).unwrap();
    app.dispatch(AppCommand::RefreshWorkspace).unwrap();
    let result = app.dispatch(AppCommand::RenameEntry {
        id: id("src"),
        new_name: "dest".into(),
    });
    assert!(matches!(result, Err(AppError::DestinationExists(_))));
    assert!(dir.path().join("src/a").exists());
    assert_eq!(
        &*app.snapshot().active_document.unwrap().content,
        "keep this buffer"
    );
}
