use super::*;
use std::{
    fs,
    sync::{Condvar, Mutex},
};

fn id(path: &str) -> DocumentId {
    DocumentId::parse(path).unwrap()
}

#[test]
fn deletes_subtree_closes_its_documents_and_keeps_siblings_and_dirty_buffers() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("notes/nested")).unwrap();
    fs::write(dir.path().join("notes/nested/a"), "needle").unwrap();
    fs::write(dir.path().join("notes-other"), "needle").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("notes-other")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("notes-other"),
        content: "unsaved".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("notes/nested/a")))
        .unwrap();
    app.dispatch(AppCommand::SetSearchQuery("needle".into()))
        .unwrap();
    let outcome = app
        .dispatch_with_events(AppCommand::DeleteEntry(id("notes")))
        .unwrap();
    assert!(
        matches!(&outcome.events[0], AppEvent::EntryDeleted { id: deleted } if deleted == &id("notes"))
    );
    assert!(!dir.path().join("notes").exists());
    assert_eq!(
        outcome.snapshot.active_document.unwrap().id,
        id("notes-other")
    );
    assert_eq!(outcome.snapshot.open_documents, [id("notes-other")]);
    assert!(outcome.snapshot.directories.is_empty());
    assert_eq!(outcome.snapshot.documents.len(), 1);
    assert_eq!(outcome.snapshot.documents[0].id, id("notes-other"));
    assert!(outcome.snapshot.documents[0].dirty);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.snapshot().search_in_progress {
        assert!(Instant::now() < deadline);
        app.dispatch(AppCommand::PollSearchResults).unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.snapshot().search_results.is_empty());
    let kept = app
        .dispatch(AppCommand::OpenDocument(id("notes-other")))
        .unwrap();
    assert_eq!(kept.active_document.unwrap().content, "unsaved");
}

#[test]
fn modified_inactive_descendants_block_deletion_without_changing_state_or_disk() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("notes")).unwrap();
    fs::write(dir.path().join("notes/a"), "saved").unwrap();
    fs::write(dir.path().join("other"), "other").unwrap();
    let mut app = Application::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("notes/a")))
        .unwrap();
    app.dispatch(AppCommand::ChangeDocument {
        id: id("notes/a"),
        content: "unsaved".into(),
    })
    .unwrap();
    app.dispatch(AppCommand::OpenDocument(id("other"))).unwrap();
    let before = app.snapshot();
    for path in ["notes", "notes/a"] {
        assert!(matches!(
            app.dispatch(AppCommand::DeleteEntry(id(path))),
            Err(AppError::DeleteUnsavedChanges)
        ));
        assert_eq!(app.snapshot(), before);
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/a")).unwrap(),
            "saved"
        );
    }
    app.dispatch(AppCommand::SaveDocument(id("notes/a")))
        .unwrap();
    let deleted = app
        .dispatch(AppCommand::DeleteEntry(id("notes/a")))
        .unwrap();
    assert_eq!(deleted.active_document.unwrap().id, id("other"));
    assert_eq!(deleted.directories.as_slice(), ["notes"]);
}

struct GatedDelete {
    started: mpsc::Sender<()>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    fail: bool,
}
impl WorkspaceFilesystem for GatedDelete {
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
    fn delete_entry(&self, root: &Path, id: &DocumentId) -> Result<(), FilesystemError> {
        self.started.send(()).unwrap();
        let (lock, condition) = &*self.gate;
        let _released = condition
            .wait_while(lock.lock().unwrap(), |released| !*released)
            .unwrap();
        if self.fail {
            return Err(FilesystemError::Io {
                path: root.join(id.as_str()),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            });
        }
        RealFilesystem.delete_entry(root, id)
    }
}

#[test]
fn native_deletion_runs_off_thread_and_failure_preserves_editor_and_reports_error() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "saved").unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, rx) = mpsc::channel();
        let core = Application::with_filesystem(Box::new(GatedDelete {
            started,
            gate: gate.clone(),
            fail,
        }));
        let mut app = NativeApplication::with_application(core);
        app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
            .unwrap();
        app.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
        let before = app.snapshot();
        assert_eq!(
            app.dispatch(AppCommand::DeleteEntry(id("a"))).unwrap(),
            before
        );
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(app.deletion_pending());
        assert!(matches!(
            app.dispatch(AppCommand::ChangeDocument {
                id: id("a"),
                content: "late edit".into()
            }),
            Err(AppError::DeleteInProgress)
        ));
        assert!(
            app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap()
                .is_none()
        );
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.has_pending_move() {
            assert!(Instant::now() < deadline);
            app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.deletion_pending());
        if fail {
            assert_eq!(app.snapshot(), before);
            assert!(app.take_background_error().unwrap().contains("denied"));
            assert!(dir.path().join("a").exists());
        } else {
            assert!(app.snapshot().documents.is_empty());
            assert!(app.snapshot().active_document.is_none());
            assert!(!dir.path().join("a").exists());
        }
    }
}

#[test]
fn deletion_waits_for_a_pending_move_instead_of_queueing_a_destructive_command() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a"), "saved").unwrap();
    fs::create_dir(dir.path().join("destination")).unwrap();
    let mut app = NativeApplication::new();
    app.dispatch(AppCommand::OpenWorkspace(dir.path().into()))
        .unwrap();
    app.dispatch(AppCommand::MoveEntryOptimistic {
        id: id("a"),
        directory: "destination".into(),
    })
    .unwrap();
    assert!(matches!(
        app.dispatch(AppCommand::DeleteEntry(id("destination/a"))),
        Err(AppError::DeleteInProgress)
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.has_pending_move() {
        assert!(Instant::now() < deadline);
        app.dispatch_if_changed(AppCommand::PollWorkspaceChanges)
            .unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("destination/a")).unwrap(),
        "saved"
    );
    assert_eq!(app.snapshot().documents[0].id, id("destination/a"));
}
