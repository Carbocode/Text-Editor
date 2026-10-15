use super::*;
use std::{
    fs,
    sync::{Condvar, Mutex, atomic::AtomicUsize},
};

/// Blocks every search read below the root until released, counting reads.
struct GatedSearchFilesystem {
    gate: Arc<(Mutex<bool>, Condvar)>,
    nested_reads: Arc<AtomicUsize>,
}

impl WorkspaceFilesystem for GatedSearchFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        RealFilesystem.open_workspace(path)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        RealFilesystem.read_document(root, id)
    }
    fn read_document_for_search_in(
        &self,
        root: &Path,
        id: &DocumentId,
        verified: &mut HashSet<String>,
    ) -> Option<String> {
        if id.as_str().contains('/') {
            self.nested_reads.fetch_add(1, Ordering::SeqCst);
            let (lock, condition) = &*self.gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        }
        RealFilesystem.read_document_for_search_in(root, id, verified)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        RealFilesystem.write_document(root, id, content)
    }
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        RealFilesystem.rename_entry(root, id, target)
    }
}

fn poll_until(app: &mut Application, done: impl Fn(&AppSnapshot) -> bool) -> AppSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = app.dispatch(AppCommand::PollSearchResults).unwrap();
        if done(&snapshot) {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "search state never arrived");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn root_matches_stream_before_nested_files_and_clearing_cancels_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("root.txt"), "needle at the root").unwrap();
    fs::create_dir_all(dir.path().join("deep/er")).unwrap();
    for index in 0..64 {
        fs::write(dir.path().join(format!("deep/er/{index}.txt")), "needle").unwrap();
    }
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let nested_reads = Arc::new(AtomicUsize::new(0));
    let mut app = Application::with_filesystem(Box::new(GatedSearchFilesystem {
        gate: gate.clone(),
        nested_reads: nested_reads.clone(),
    }));
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
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
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    thread::sleep(Duration::from_millis(300));
    let after = app.dispatch(AppCommand::PollSearchResults).unwrap();
    assert!(
        after
            .search_results
            .iter()
            .all(|result| result.location.is_none())
    );
    // Each worker finishes at most the file it was reading.
    assert_eq!(nested_reads.load(Ordering::SeqCst), blocked);
    assert!(blocked < 64);
}

#[test]
fn a_search_stops_at_the_result_limit_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("many.txt"),
        "x ".repeat(search::RESULT_LIMIT + 10),
    )
    .unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::QueueSearchQuery("x".into()))
        .unwrap();
    let done = poll_until(&mut app, |snapshot| !snapshot.search_in_progress);
    assert!(done.search_truncated);
    assert_eq!(done.search_results.len(), search::RESULT_LIMIT);
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
    let mut ids: Vec<_> = renamed
        .search_results
        .iter()
        .map(|result| result.id.as_str())
        .collect();
    ids.sort();
    assert_eq!(ids, ["b.rs", "lib/a.rs"]);
    fs::remove_file(dir.path().join("b.rs")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let refreshed = loop {
        let snapshot = app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
        if snapshot.documents.len() == 1 {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "watcher never reported the removal"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        !refreshed.search_in_progress,
        "disk changes do not rerun a search"
    );
    assert_eq!(refreshed.search_results.len(), 1);
    assert_eq!(refreshed.search_results[0].id.as_str(), "lib/a.rs");
}
