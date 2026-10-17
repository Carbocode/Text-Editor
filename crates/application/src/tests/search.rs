use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn poll_until(app: &mut Application, done: impl Fn(&AppSnapshot) -> bool) -> AppSnapshot {
    wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        done,
    )
}

#[test]
fn root_matches_stream_before_nested_files_and_clearing_cancels_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("root.txt"), "needle at the root").unwrap();
    fs::create_dir_all(dir.path().join("deep/er")).unwrap();
    for index in 0..64 {
        fs::write(dir.path().join(format!("deep/er/{index}.txt")), "needle").unwrap();
    }
    let gate = Gate::default();
    let nested_reads = Arc::new(AtomicUsize::new(0));
    let (hook_gate, reads) = (gate.clone(), nested_reads.clone());
    let filesystem = TestFilesystem::with(move |operation| {
        // Blocks every search read below the root until released.
        if let Operation::SearchRead(id) = operation
            && id.as_str().contains('/')
        {
            reads.fetch_add(1, Ordering::SeqCst);
            hook_gate.wait();
        }
        Ok(())
    });
    let mut app = open(dir.path(), filesystem);
    app.dispatch(AppCommand::QueueSearchQuery("needle".into()))
        .unwrap();
    // Nested reads are blocked, yet the root match is already published.
    let partial = poll_until(&mut app, |snapshot| !snapshot.search_results.is_empty());
    assert!(partial.search_in_progress);
    assert_eq!(partial.search_results.len(), 1);
    assert_eq!(partial.search_results[0].id.as_str(), "root.txt");

    let cleared = app
        .dispatch(AppCommand::QueueSearchQuery(String::new()))
        .unwrap();
    assert!(
        !cleared.search_in_progress,
        "clearing stops the search at once"
    );
    let blocked = nested_reads.load(Ordering::SeqCst);
    gate.open();
    thread::sleep(Duration::from_millis(300));
    let after = app.dispatch(AppCommand::PollSearchResults).unwrap();
    assert!(after.search_results.is_empty());
    // Each worker finishes at most the file it was reading.
    assert_eq!(nested_reads.load(Ordering::SeqCst), blocked);
    assert!(blocked < 64);
}

#[test]
fn a_search_stops_at_the_result_limit_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("many.txt"),
        "x ".repeat(crate::search::RESULT_LIMIT + 10),
    )
    .unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::QueueSearchQuery("x".into()))
        .unwrap();
    let done = poll_until(&mut app, |snapshot| !snapshot.search_in_progress);
    assert!(done.search_truncated);
    assert_eq!(done.search_results.len(), crate::search::RESULT_LIMIT);
    let narrowed = app
        .dispatch(AppCommand::QueueSearchQuery("x x".into()))
        .unwrap();
    assert!(!narrowed.search_truncated, "a new query clears the notice");
}

#[test]
fn renames_and_refreshes_update_matches_without_searching_again() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/a.rs"), "needle").unwrap();
    fs::write(dir.path().join("b.rs"), "needle").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::QueueSearchQuery("needle".into()))
        .unwrap();
    poll_until(&mut app, |snapshot| !snapshot.search_in_progress);
    let renamed = app
        .dispatch(AppCommand::RenameEntry {
            id: DocumentId::parse("src").unwrap(),
            new_name: "lib".into(),
        })
        .unwrap();
    assert!(!renamed.search_in_progress);
    let ids: Vec<_> = renamed
        .search_results
        .iter()
        .map(|result| result.id.as_str())
        .collect();
    assert_eq!(ids, ["b.rs", "lib/a.rs"], "results stay sorted by path");
    fs::remove_file(dir.path().join("b.rs")).unwrap();
    let refreshed = wait_until(
        || app.dispatch(AppCommand::PollWorkspaceChanges).unwrap(),
        |snapshot| snapshot.documents.len() == 1,
    );
    assert!(
        !refreshed.search_in_progress,
        "disk changes do not rerun a search"
    );
    assert_eq!(refreshed.search_results.len(), 1);
    assert_eq!(refreshed.search_results[0].id.as_str(), "lib/a.rs");
}

#[test]
fn relocating_during_search_cannot_publish_results_under_the_old_paths() {
    let directory = workspace(&[("src/a.txt", "needle"), ("sibling", "needle")]);
    let gate = Gate::default();
    let (started, observed) = std::sync::mpsc::channel();
    let started = std::sync::Mutex::new(started);
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::SearchRead(id) if id.as_str() == "src/a.txt") {
            let _ = started.lock().unwrap().send(());
            hook_gate.wait();
        }
        Ok(())
    });
    let mut app = open(directory.path(), filesystem);
    app.dispatch(AppCommand::QueueSearchQuery("needle".into()))
        .unwrap();
    observed.recv_timeout(Duration::from_secs(3)).unwrap();
    app.dispatch(AppCommand::RenameEntry {
        id: id("src"),
        new_name: "renamed".into(),
    })
    .unwrap();
    gate.open();
    let done = wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |s| !s.search_in_progress,
    );
    assert_eq!(
        done.search_results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["renamed/a.txt", "sibling"]
    );
}
