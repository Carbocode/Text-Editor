use super::*;

fn prepare(replacement: &str, target: ReplacementTarget) -> AppCommand {
    AppCommand::PrepareReplacement {
        replacement: replacement.into(),
        preserve_case: false,
        target,
    }
}

fn configure(app: &mut Application, options: SearchOptions) -> AppSnapshot {
    app.dispatch(AppCommand::ConfigureSearch(options)).unwrap();
    wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    )
}

#[test]
fn options_apply_to_streamed_results_and_subsequent_buffer_edits() {
    let directory = workspace(&[
        ("src/a.rs", "cat Cat concatenate cat_ caté cat"),
        ("skip.txt", "cat"),
    ]);
    let mut app = open(directory.path(), TestFilesystem::real());
    search(&mut app, "cat");
    let options = SearchOptions {
        match_case: true,
        whole_word: true,
        include: "*.rs".into(),
        ..Default::default()
    };
    let result = configure(&mut app, options);
    assert_eq!(result.search_results.len(), 2);
    assert!(
        result
            .search_results
            .iter()
            .all(|result| result.id == id("src/a.rs"))
    );
    app.dispatch(AppCommand::OpenDocument(id("src/a.rs")))
        .unwrap();
    let edited = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("src/a.rs"),
            content: "Cat cat_ cat".into(),
        })
        .unwrap();
    assert_eq!(edited.search_results.len(), 1);
    assert_eq!(edited.search_results[0].location.start_char, 9);
    configure(
        &mut app,
        SearchOptions {
            exclude: "src".into(),
            ..Default::default()
        },
    );
    let excluded = app
        .dispatch(AppCommand::ChangeDocument {
            id: id("src/a.rs"),
            content: "cat cat".into(),
        })
        .unwrap();
    assert!(
        excluded
            .search_results
            .iter()
            .all(|result| result.id != id("src/a.rs"))
    );
}

#[test]
fn include_exclude_braces_and_ignore_toggle_are_shared() {
    let directory = workspace(&[
        (".gitignore", "target/\n"),
        ("target/a.rs", "needle"),
        ("src/a.rs", "needle"),
        ("src/a.md", "needle"),
        ("src/a.txt", "needle"),
        ("nested/src/a.rs", "needle"),
    ]);
    let mut app = open(directory.path(), TestFilesystem::real());
    search(&mut app, "needle");
    let found = configure(
        &mut app,
        SearchOptions {
            include: "./src/*.{rs,md}, target".into(),
            exclude: "*.md".into(),
            use_ignore_files: false,
            ..Default::default()
        },
    );
    assert_eq!(
        found
            .search_results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["src/a.rs", "target/a.rs"]
    );
    let found = configure(
        &mut app,
        SearchOptions {
            include: "*.rs".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        found
            .search_results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["nested/src/a.rs", "src/a.rs"]
    );
}

#[test]
fn open_editors_include_external_buffers_and_drop_closed_tabs() {
    let directory = workspace(&[("a", "needle"), ("b", "needle")]);
    let external = workspace(&[("external", "needle")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    app.dispatch(AppCommand::OpenFile(external.path().join("external")))
        .unwrap();
    configure(
        &mut app,
        SearchOptions {
            only_open_editors: true,
            ..Default::default()
        },
    );
    let found = search(&mut app, "needle");
    assert_eq!(found.search_results.len(), 2);
    assert!(found.search_results.iter().any(|r| r.id.is_external()));
    app.dispatch(AppCommand::CloseDocument(id("a"))).unwrap();
    let found = wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    assert_eq!(found.search_results.len(), 1);
    assert!(found.search_results[0].id.is_external());
}

#[test]
fn invalid_regex_or_glob_clears_old_results_and_recovers() {
    let directory = workspace(&[("a", "needle")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    search(&mut app, "needle");
    configure(
        &mut app,
        SearchOptions {
            regex: true,
            ..Default::default()
        },
    );
    let invalid = search(&mut app, "[");
    assert!(invalid.search_error.is_some());
    assert!(invalid.search_results.is_empty());
    assert!(!invalid.search_in_progress);
    assert_eq!(search(&mut app, "needle").search_results.len(), 1);
    let invalid = configure(
        &mut app,
        SearchOptions {
            include: "[".into(),
            ..Default::default()
        },
    );
    assert!(invalid.search_error.is_some());
    assert!(invalid.search_results.is_empty());
    assert_eq!(
        configure(&mut app, SearchOptions::default())
            .search_results
            .len(),
        1
    );
}

#[test]
fn regex_captures_and_unicode_offsets_replace_exactly_the_preview() {
    let directory = workspace(&[("a", "💡 café-12\ncafé-34")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    configure(
        &mut app,
        SearchOptions {
            regex: true,
            ..Default::default()
        },
    );
    let found = search(&mut app, r"(café)-(\d+)");
    assert_eq!(found.search_results.len(), 2);
    assert_eq!(found.search_results[0].location.start_char, 2);
    assert_eq!(found.search_results[0].location.start_utf16, 3);
    let preview = app
        .dispatch(prepare("${2}:$1", ReplacementTarget::All))
        .unwrap();
    assert_eq!(preview.replacement_preview.as_ref().unwrap().occurrences, 2);
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "💡 café-12\ncafé-34"
    );
    app.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "💡 12:café\n34:café"
    );
    assert!(app.snapshot().replacement_preview.is_none());
}

#[test]
fn replacement_preserves_case_and_leaves_open_buffers_unsaved() {
    let directory = workspace(&[("a", "cat CAT Cat cAt"), ("b", "cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a"),
        content: "cat CAT Cat cAt local".into(),
    })
    .unwrap();
    let generation = app.snapshot().editor_generation;
    search(&mut app, "cat");
    app.dispatch(AppCommand::PrepareReplacement {
        replacement: "dog".into(),
        preserve_case: true,
        target: ReplacementTarget::All,
    })
    .unwrap();
    let replaced = app.dispatch(AppCommand::ApplyReplacement).unwrap();
    let active = replaced.active_document.unwrap();
    assert_eq!(&*active.content, "dog DOG Dog dog local");
    assert!(active.dirty);
    assert_eq!(replaced.editor_generation, generation);
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat CAT Cat cAt"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("b")).unwrap(),
        "dog"
    );
    app.dispatch(AppCommand::SaveDocument(id("a"))).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "dog DOG Dog dog local"
    );
}

#[test]
fn replace_match_and_file_leave_other_results_untouched() {
    let directory = workspace(&[("a", "cat cat"), ("b", "cat cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    let found = search(&mut app, "cat");
    let result = &found.search_results[1];
    app.dispatch(prepare(
        "dog",
        ReplacementTarget::Match {
            id: result.id.clone(),
            location: result.location.clone(),
        },
    ))
    .unwrap();
    app.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat dog"
    );
    wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    app.dispatch(prepare("", ReplacementTarget::Document(id("b"))))
        .unwrap();
    app.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat dog"
    );
    assert_eq!(fs::read_to_string(directory.path().join("b")).unwrap(), " ");
}

#[test]
fn stale_disk_preview_refuses_all_files_before_any_write() {
    let directory = workspace(&[("a", "cat"), ("b", "cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    search(&mut app, "cat");
    app.dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    fs::write(directory.path().join("b"), "outside edit").unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::ApplyReplacement),
        Err(AppError::StaleReplacement(_))
    ));
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("b")).unwrap(),
        "outside edit"
    );
}

#[test]
fn stale_buffer_preview_and_external_conflicts_keep_local_edits() {
    let directory = workspace(&[("a", "cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    search(&mut app, "cat");
    app.dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("a"),
        content: "cat typed after preview".into(),
    })
    .unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::ApplyReplacement),
        Err(AppError::StaleReplacement(_))
    ));
    fs::write(directory.path().join("a"), "external cat").unwrap();
    assert!(matches!(
        app.dispatch(prepare("dog", ReplacementTarget::All)),
        Err(AppError::ExternalChanges(_))
    ));
    assert_eq!(
        &*app.snapshot().active_document.unwrap().content,
        "cat typed after preview"
    );
}

#[test]
fn failed_write_retains_prior_success_and_clears_preview() {
    let directory = workspace(&[("a", "cat"), ("b", "cat")]);
    let mut app = open(
        directory.path(),
        TestFilesystem::with(|operation| {
            if matches!(operation, Operation::Write(id) if id.as_str() == "b") {
                return Err(io::Error::other("write failed"));
            }
            Ok(())
        }),
    );
    search(&mut app, "cat");
    app.dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    assert!(app.dispatch(AppCommand::ApplyReplacement).is_err());
    assert!(app.snapshot().replacement_preview.is_none());
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "dog"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("b")).unwrap(),
        "cat"
    );
}

#[test]
fn overlapping_and_zero_width_matches_replace_once_in_text_order() {
    let directory = workspace(&[("a", "ababa")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    assert_eq!(search(&mut app, "aba").search_results.len(), 2);
    assert_eq!(
        configure(
            &mut app,
            SearchOptions {
                match_case: true,
                ..Default::default()
            }
        )
        .search_results
        .len(),
        2
    );
    let preview = app.dispatch(prepare("X", ReplacementTarget::All)).unwrap();
    assert_eq!(preview.replacement_preview.unwrap().occurrences, 1);
    app.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "Xba"
    );
    configure(
        &mut app,
        SearchOptions {
            regex: true,
            ..Default::default()
        },
    );
    search(&mut app, "^|$");
    app.dispatch(prepare("!", ReplacementTarget::All)).unwrap();
    app.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "!Xba!"
    );
}

#[test]
fn cancelled_or_truncated_search_cannot_replace_and_refresh_recovers() {
    let directory = workspace(&[("a", "cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    search(&mut app, "cat");
    app.dispatch(AppCommand::CancelSearch).unwrap();
    assert!(matches!(
        app.dispatch(prepare("dog", ReplacementTarget::All)),
        Err(AppError::IncompleteSearch)
    ));
    app.dispatch(AppCommand::RefreshSearch).unwrap();
    wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    app.dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    app.dispatch(AppCommand::DiscardReplacement).unwrap();
    assert!(app.snapshot().replacement_preview.is_none());
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat"
    );
}

#[test]
fn older_search_batches_cannot_overwrite_new_options_or_a_cancellation() {
    for cancel in [false, true] {
        let directory = workspace(&[("a", "cat")]);
        let gate = Gate::default();
        let hook_gate = gate.clone();
        let (started, observed) = std::sync::mpsc::channel();
        let started = Mutex::new(started);
        let mut app = open(
            directory.path(),
            TestFilesystem::with(move |operation| {
                if matches!(operation, Operation::SearchRead(_)) {
                    let _ = started.lock().unwrap().send(());
                    hook_gate.wait();
                }
                Ok(())
            }),
        );
        app.dispatch(AppCommand::QueueSearchQuery("CAT".into()))
            .unwrap();
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        if cancel {
            app.dispatch(AppCommand::CancelSearch).unwrap();
        } else {
            app.dispatch(AppCommand::ConfigureSearch(SearchOptions {
                match_case: true,
                ..Default::default()
            }))
            .unwrap();
        }
        gate.open();
        let done = wait_until(
            || app.dispatch(AppCommand::PollSearchResults).unwrap(),
            |s| !s.search_in_progress,
        );
        assert!(done.search_results.is_empty());
        assert_eq!(done.search_truncated, cancel);
    }
}

#[test]
fn relocation_rechecks_the_active_file_scope() {
    let directory = workspace(&[("a.rs", "cat"), ("b.txt", "cat")]);
    let mut app = open(directory.path(), TestFilesystem::real());
    configure(
        &mut app,
        SearchOptions {
            include: "*.rs".into(),
            ..Default::default()
        },
    );
    assert_eq!(search(&mut app, "cat").search_results.len(), 1);
    app.dispatch(AppCommand::RenameEntry {
        id: id("a.rs"),
        new_name: "a.txt".into(),
    })
    .unwrap();
    let done = wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    assert!(done.search_results.is_empty());
    app.dispatch(AppCommand::RenameEntry {
        id: id("b.txt"),
        new_name: "b.rs".into(),
    })
    .unwrap();
    let done = wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    assert_eq!(done.search_results[0].id, id("b.rs"));
}

#[test]
fn applying_replacements_is_exclusive_and_never_blocks_dispatch() {
    let directory = workspace(&[("a", "cat"), ("b", "unrelated")]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let mut session = Session::with_application(open(
        directory.path(),
        TestFilesystem::with(move |operation| {
            if matches!(operation, Operation::Write(_)) {
                hook_gate.wait();
            }
            Ok(())
        }),
    ));
    session.dispatch(AppCommand::OpenDocument(id("b"))).unwrap();
    session
        .dispatch(AppCommand::QueueSearchQuery("cat".into()))
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |s| !s.search_in_progress,
    );
    session
        .dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    assert!(settle(&mut session).replacement_preview.is_some());
    let started = Instant::now();
    session.dispatch(AppCommand::ApplyReplacement).unwrap();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(session.snapshot().activity.editing_paused);
    assert!(matches!(
        session.dispatch(AppCommand::ChangeDocument {
            id: id("b"),
            content: "late".into()
        }),
        Err(AppError::OperationInProgress)
    ));
    gate.open();
    let done = settle(&mut session);
    assert!(!done.activity.editing_paused);
    assert_eq!(&*done.active_document.unwrap().content, "unrelated");
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "dog"
    );
}

#[test]
fn edits_projected_during_preparation_make_the_preview_stale() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let directory = workspace(&[("a", "cat")]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let block = Arc::new(AtomicBool::new(false));
    let hook_block = block.clone();
    let mut session = Session::with_application(open(
        directory.path(),
        TestFilesystem::with(move |operation| {
            if matches!(operation, Operation::Read(_)) && hook_block.load(Ordering::SeqCst) {
                hook_gate.wait();
            }
            Ok(())
        }),
    ));
    session.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
    session
        .dispatch(AppCommand::QueueSearchQuery("cat".into()))
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |s| !s.search_in_progress,
    );
    block.store(true, Ordering::SeqCst);
    session
        .dispatch(prepare("dog", ReplacementTarget::All))
        .unwrap();
    assert!(!session.snapshot().activity.editing_paused);
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("a"),
            content: "cat typed during preview".into(),
        })
        .unwrap();
    gate.open();
    let prepared = settle(&mut session);
    assert!(prepared.replacement_preview.is_some());
    assert_eq!(
        &*prepared.active_document.unwrap().content,
        "cat typed during preview"
    );
    session.dispatch(AppCommand::ApplyReplacement).unwrap();
    let rejected = settle(&mut session);
    assert!(rejected.error.as_deref().unwrap().contains("changed since"));
    assert_eq!(
        &*rejected.active_document.unwrap().content,
        "cat typed during preview"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("a")).unwrap(),
        "cat"
    );
}
