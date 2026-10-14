use super::*;
use std::{
    fs,
    sync::{Condvar, Mutex},
};

struct GatedFilesystem {
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl WorkspaceFilesystem for GatedFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        RealFilesystem.open_workspace(path)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        RealFilesystem.read_document(root, id)
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
        let (lock, condition) = &*self.gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
        RealFilesystem.rename_entry(root, id, target)
    }
}

#[test]
fn optimistic_folder_move_is_immediate_and_rollback_preserves_edits_and_queued_save() {
    for reject in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src/nested")).unwrap();
        fs::create_dir(directory.path().join("dest")).unwrap();
        fs::write(directory.path().join("src/nested/a.rs"), "original").unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let mut application =
            Application::with_filesystem(Box::new(GatedFilesystem { gate: gate.clone() }));
        let id = |path| DocumentId::parse(path).unwrap();
        application
            .dispatch(AppCommand::OpenWorkspace(directory.path().into()))
            .unwrap();
        application
            .dispatch(AppCommand::OpenDocument(id("src/nested/a.rs")))
            .unwrap();
        let started = Instant::now();
        let moved = application
            .dispatch(AppCommand::MoveEntryOptimistic {
                id: id("src"),
                directory: "dest".into(),
            })
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "Drop waited for filesystem confirmation"
        );
        assert_eq!(
            moved.active_document.unwrap().id,
            id("dest/src/nested/a.rs")
        );
        assert!(moved.directories.contains(&"dest/src/nested".into()));
        assert_eq!(application.pending_move_target(), Some(&id("dest/src")));
        assert!(directory.path().join("src/nested/a.rs").exists());
        assert!(!directory.path().join("dest/src").exists());
        assert!(
            application
                .dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap()
                .is_none()
        );
        application
            .dispatch(AppCommand::ChangeDocument {
                id: id("dest/src/nested/a.rs"),
                content: "typed while moving".into(),
            })
            .unwrap();
        application
            .dispatch(AppCommand::SaveDocument(id("dest/src/nested/a.rs")))
            .unwrap();
        application
            .dispatch(AppCommand::OpenDocument(id("dest/src/nested/a.rs")))
            .unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("src/nested/a.rs")).unwrap(),
            "original"
        );
        if reject {
            fs::create_dir(directory.path().join("dest/src")).unwrap();
            fs::write(directory.path().join("dest/src/external.txt"), "external").unwrap();
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(3);
        while application.pending_move.is_some() && Instant::now() < deadline {
            application
                .dispatch_if_changed(AppCommand::PollSearchResults)
                .unwrap();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(application.pending_move.is_none());
        assert!(application.pending_move_target().is_none());
        let final_path = if reject {
            "src/nested/a.rs"
        } else {
            "dest/src/nested/a.rs"
        };
        let snapshot = application.snapshot();
        let active = snapshot.active_document.unwrap();
        assert_eq!(active.id, id(final_path));
        assert_eq!(active.content, "typed while moving");
        assert!(!active.dirty);
        assert_eq!(
            fs::read_to_string(directory.path().join(final_path)).unwrap(),
            "typed while moving"
        );
        assert_eq!(application.take_background_error().is_some(), reject);
        if reject {
            assert_eq!(
                fs::read_to_string(directory.path().join("dest/src/external.txt")).unwrap(),
                "external"
            );
        }
    }
}

struct GatedScanFilesystem {
    gate: Arc<(Mutex<bool>, Condvar)>,
    calls: std::sync::atomic::AtomicUsize,
    started: mpsc::Sender<()>,
}
impl WorkspaceFilesystem for GatedScanFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            let _ = self.started.send(());
            let (lock, condition) = &*self.gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        }
        RealFilesystem.open_workspace(path)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        RealFilesystem.read_document(root, id)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        RealFilesystem.write_document(root, id, content)
    }
}

#[test]
fn watcher_scan_never_blocks_dispatch_or_overwrites_edits_made_while_scanning() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("a.txt"), "original").unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, observed) = mpsc::channel();
    let mut app = Application::with_filesystem(Box::new(GatedScanFilesystem {
        gate: gate.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
        started,
    }));
    let id = DocumentId::parse("a.txt").unwrap();
    app.dispatch(AppCommand::OpenWorkspace(directory.path().into()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id.clone())).unwrap();
    app.watcher = None;
    app.last_reconcile = Instant::now() - Duration::from_secs(3);
    let start = Instant::now();
    assert!(
        app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
            .unwrap()
            .is_none()
    );
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "Poll blocked on directory listing"
    );
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    let start = Instant::now();
    assert!(
        app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
            .unwrap()
            .is_none()
    );
    app.dispatch(AppCommand::ChangeDocument {
        id,
        content: "local edit".into(),
    })
    .unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "Pending scan froze subsequent UI commands"
    );
    fs::write(directory.path().join("a.txt"), "external edit").unwrap();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        app.dispatch(AppCommand::PollWorkspaceChanges).unwrap();
        let active = app.snapshot().active_document.unwrap();
        assert_eq!(
            active.content, "local edit",
            "Stale scan overwrote the editor"
        );
        assert!(active.dirty);
        if active.external_change {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn native_drop_keeps_dispatch_responsive_during_one_second_backend_stall() {
    for reject in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("dest")).unwrap();
        fs::write(directory.path().join("a.rs"), "original").unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let mut core =
            Application::with_filesystem(Box::new(GatedFilesystem { gate: gate.clone() }));
        let id = |path| DocumentId::parse(path).unwrap();
        core.dispatch(AppCommand::OpenWorkspace(directory.path().into()))
            .unwrap();
        core.dispatch(AppCommand::OpenDocument(id("a.rs"))).unwrap();
        let mut app = NativeApplication::with_application(core);
        let started = Instant::now();
        let snapshot = app
            .dispatch_sidebar_move_with_edit(
                id("a.rs"),
                "dest".into(),
                SidebarDropTrace::default(),
                Some((id("a.rs"), "local edit before drop".into())),
            )
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "release handler waited for the backend"
        );
        let active = snapshot.active_document.unwrap();
        assert_eq!(active.id, id("dest/a.rs"));
        assert_eq!(active.content, "local edit before drop");
        assert_eq!(app.pending_move_target(), Some(&id("dest/a.rs")));
        let until = Instant::now() + Duration::from_secs(1);
        let mut ticks = 0;
        while Instant::now() < until {
            let poll = Instant::now();
            assert!(
                app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                    .unwrap()
                    .is_none()
            );
            assert!(
                poll.elapsed() < Duration::from_millis(50),
                "UI poll waited for the backend"
            );
            ticks += 1;
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ticks > 50);
        app.dispatch(AppCommand::ChangeDocument {
            id: id("dest/a.rs"),
            content: "typed while moving".into(),
        })
        .unwrap();
        app.dispatch(AppCommand::SaveDocument(id("dest/a.rs")))
            .unwrap();
        assert_eq!(
            app.snapshot().active_document.unwrap().content,
            "typed while moving"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("a.rs")).unwrap(),
            "original"
        );
        if reject {
            fs::write(directory.path().join("dest/a.rs"), "external collision").unwrap();
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.has_pending_move() && Instant::now() < deadline {
            app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.has_pending_move());
        assert!(app.pending_move_target().is_none());
        let target = if reject { "a.rs" } else { "dest/a.rs" };
        let active = app.snapshot().active_document.unwrap();
        assert_eq!(active.id, id(target));
        assert_eq!(active.content, "typed while moving");
        assert!(!active.dirty);
        assert_eq!(
            fs::read_to_string(directory.path().join(target)).unwrap(),
            "typed while moving"
        );
        assert_eq!(app.take_background_error().is_some(), reject);
        if reject {
            assert_eq!(
                fs::read_to_string(directory.path().join("dest/a.rs")).unwrap(),
                "external collision"
            );
        }
    }
}

struct GatedWriteFilesystem {
    gate: Arc<(Mutex<bool>, Condvar)>,
    entered: mpsc::Sender<()>,
}
impl WorkspaceFilesystem for GatedWriteFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        RealFilesystem.open_workspace(path)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        RealFilesystem.read_document(root, id)
    }
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        RealFilesystem.rename_entry(root, id, target)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        let _ = self.entered.send(());
        let mut released = self.gate.0.lock().unwrap();
        while !*released {
            released = self.gate.1.wait(released).unwrap();
        }
        RealFilesystem.write_document(root, id, content)
    }
}

#[test]
fn native_replay_of_a_slow_queued_save_does_not_block_or_overwrite_new_typing() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("dest")).unwrap();
    fs::write(directory.path().join("a.rs"), "original").unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered, observed) = mpsc::channel();
    let mut core = Application::with_filesystem(Box::new(GatedWriteFilesystem {
        gate: gate.clone(),
        entered,
    }));
    let id = |path| DocumentId::parse(path).unwrap();
    core.dispatch(AppCommand::OpenWorkspace(directory.path().into()))
        .unwrap();
    core.dispatch(AppCommand::OpenDocument(id("a.rs"))).unwrap();
    let mut app = NativeApplication::with_application(core);
    app.dispatch(AppCommand::MoveEntryOptimistic {
        id: id("a.rs"),
        directory: "dest".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("dest/a.rs"),
        content: "first edit".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::SaveDocument(id("dest/a.rs")))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.pending_move_target().is_some() && Instant::now() < deadline {
        app.dispatch_if_changed(AppCommand::PollSearchResults)
            .unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        app.has_pending_move(),
        "queued save must stay on the worker"
    );
    let typed_at = Instant::now();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("dest/a.rs"),
        content: "second edit during slow save".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::SaveDocument(id("dest/a.rs")))
        .unwrap();
    assert!(typed_at.elapsed() < Duration::from_millis(100));
    assert_eq!(
        app.snapshot().active_document.unwrap().content,
        "second edit during slow save"
    );
    assert!(
        app.dispatch_if_changed(AppCommand::PollSearchResults)
            .unwrap()
            .is_none()
    );
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    while app.has_pending_move() && Instant::now() < deadline {
        app.dispatch_if_changed(AppCommand::PollSearchResults)
            .unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.has_pending_move());
    let active = app.snapshot().active_document.unwrap();
    assert_eq!(active.content, "second edit during slow save");
    assert!(!active.dirty);
    assert_eq!(
        fs::read_to_string(directory.path().join("dest/a.rs")).unwrap(),
        active.content
    );
}
