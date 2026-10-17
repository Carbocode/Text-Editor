use super::*;
use native_workspace_storage::{
    MemoryStorage, MetadataStorage, SqliteStorage, StorageError, WorkspaceSession,
};

fn persistent(db: &Path) -> Application {
    let mut app = Application::new();
    app.enable_persistence(SqliteStorage::open(db).unwrap())
        .unwrap();
    app
}

#[test]
fn restart_restores_order_active_preview_and_external_tabs_from_current_disk() {
    let root = workspace(&[("a.txt", "a"), ("b.txt", "b"), ("c.txt", "c")]);
    let external = workspace(&[("outside.txt", "external")]);
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("state.sqlite3");
    let mut app = persistent(&db);
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("a.txt"))).unwrap();
    app.execute(AppCommand::PreviewDocument(id("b.txt")))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("c.txt"))).unwrap();
    app.execute(AppCommand::OpenFile(external.path().join("outside.txt")))
        .unwrap();
    app.execute(AppCommand::MoveDocumentTab {
        id: id("c.txt"),
        index: 0,
    })
    .unwrap();
    app.execute(AppCommand::PreviewDocument(id("a.txt")))
        .unwrap();
    let saved = app.storage.session().unwrap().unwrap();
    assert_eq!(saved.preview_document.as_deref(), Some("b.txt"));
    app.execute(AppCommand::ChangeDocument {
        id: id("a.txt"),
        content: "unsaved draft".into(),
    })
    .unwrap();
    assert_eq!(fs::read_to_string(root.path().join("a.txt")).unwrap(), "a");
    drop(app);
    fs::write(root.path().join("a.txt"), "changed while closed").unwrap();
    let mut session = Session::with_application(persistent(&db));
    assert!(
        session.snapshot().workspace.is_none(),
        "enabling storage must not start restoration"
    );
    session.dispatch(AppCommand::RestoreSession).unwrap();
    assert!(session.snapshot().activity.editing_paused);
    let restored = settle(&mut session);
    assert!(restored.error.is_none(), "{:?}", restored.error);
    assert_eq!(
        restored
            .open_documents
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        saved.documents
    );
    assert_eq!(restored.preview_document, Some(id("b.txt")));
    let active = restored.active_document.unwrap();
    assert_eq!(active.id, id("a.txt"));
    assert_eq!(&*active.content, "changed while closed");
    assert!(!active.dirty);
    assert!(restored.editor_generation > 0);
}

#[test]
fn restore_handles_first_launch_and_a_workspace_with_no_tabs() {
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("state.sqlite3");
    let mut app = persistent(&db);
    app.execute(AppCommand::RestoreSession).unwrap();
    assert!(app.snapshot().workspace.is_none());
    let root = workspace(&[("a.txt", "a")]);
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("a.txt"))).unwrap();
    app.execute(AppCommand::CloseDocument(id("a.txt"))).unwrap();
    drop(app);
    let mut app = persistent(&db);
    app.execute(AppCommand::RestoreSession).unwrap();
    assert!(app.snapshot().workspace.is_some());
    assert!(app.snapshot().open_documents.is_empty());
    assert!(app.snapshot().active_document.is_none());
}

#[test]
fn missing_tabs_do_not_stop_restore_or_erase_the_saved_layout() {
    let root = workspace(&[("a.txt", "a"), ("b.txt", "b"), ("c.txt", "c")]);
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("state.sqlite3");
    let mut app = persistent(&db);
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        app.execute(AppCommand::OpenDocument(id(name))).unwrap();
    }
    app.execute(AppCommand::PreviewDocument(id("b.txt")))
        .unwrap();
    let saved = app.storage.session().unwrap();
    drop(app);
    fs::remove_file(root.path().join("b.txt")).unwrap();
    let mut session = Session::with_application(persistent(&db));
    session.dispatch(AppCommand::RestoreSession).unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.open_documents, [id("a.txt"), id("c.txt")]);
    assert_eq!(snapshot.active_document.unwrap().id, id("a.txt"));
    session
        .dispatch(AppCommand::PrepareToQuit { save: false })
        .unwrap();
    assert_eq!(SqliteStorage::open(&db).unwrap().session().unwrap(), saved);
    fs::write(root.path().join("b.txt"), "returned").unwrap();
    let mut app = persistent(&db);
    app.execute(AppCommand::RestoreSession).unwrap();
    assert_eq!(app.snapshot().open_documents.len(), 3);
}

#[test]
fn missing_workspace_keeps_home_and_the_saved_session() {
    let root = workspace(&[("a.txt", "a")]);
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("state.sqlite3");
    let mut app = persistent(&db);
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    let saved = app.storage.session().unwrap();
    drop(app);
    root.close().unwrap();
    let mut session = Session::with_application(persistent(&db));
    session.dispatch(AppCommand::RestoreSession).unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.workspace.is_none());
    assert!(snapshot.error.is_some());
    assert_eq!(SqliteStorage::open(&db).unwrap().session().unwrap(), saved);
}

#[test]
fn stored_paths_are_validated_and_unreadable_files_do_not_block_later_tabs() {
    let root = workspace(&[("invalid.txt", "unused"), ("good.txt", "good")]);
    fs::write(root.path().join("invalid.txt"), [0xff, 0xfe]).unwrap();
    let mut app = Application::new();
    app.storage
        .save_session(&WorkspaceSession {
            root: root.path().into(),
            documents: vec![
                "../escape.txt".into(),
                "invalid.txt".into(),
                "good.txt".into(),
            ],
            active_document: Some("../escape.txt".into()),
            preview_document: Some("invalid.txt".into()),
        })
        .unwrap();
    assert!(app.execute(AppCommand::RestoreSession).is_err());
    let snapshot = app.snapshot();
    assert_eq!(snapshot.open_documents, [id("good.txt")]);
    assert!(snapshot.preview_document.is_none());
    assert_eq!(&*snapshot.active_document.unwrap().content, "good");
}

#[test]
fn explicit_open_prevents_restore_and_late_system_opens_run_after_it() {
    let original = workspace(&[("old.txt", "old")]);
    let requested = workspace(&[("new.txt", "new")]);
    let mut app = Application::new();
    app.execute(AppCommand::OpenWorkspace(original.path().into()))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("old.txt")))
        .unwrap();
    let saved = app.storage.session().unwrap().unwrap();
    let mut fresh = Application::new();
    fresh.storage.save_session(&saved).unwrap();
    fresh
        .execute(AppCommand::OpenPaths(vec![
            requested.path().join("new.txt"),
        ]))
        .unwrap();
    fresh.execute(AppCommand::RestoreSession).unwrap();
    assert_eq!(fresh.snapshot().active_document.unwrap().id, id("new.txt"));

    let mut fresh = Application::new();
    fresh.storage.save_session(&saved).unwrap();
    let mut session = Session::with_application(fresh);
    session.delay_next_background_job(Duration::from_millis(50));
    session.dispatch(AppCommand::RestoreSession).unwrap();
    session
        .dispatch(AppCommand::OpenPaths(vec![
            requested.path().into(),
            requested.path().join("new.txt"),
        ]))
        .unwrap();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
    assert_eq!(
        snapshot.workspace.unwrap().root,
        fs::canonicalize(requested.path()).unwrap()
    );
    assert_eq!(snapshot.open_documents, [id("new.txt")]);
}

#[test]
fn failed_relocation_never_persists_the_projected_path_and_replays_edits() {
    let root = workspace(&[("a.txt", "a")]);
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("state.sqlite3");
    let gate = Gate::default();
    let worker_gate = gate.clone();
    let fs = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(_, _)) {
            worker_gate.wait();
            return Err(io::Error::other("rename rejected"));
        }
        Ok(())
    });
    let mut app = Application::with_filesystem(Box::new(fs));
    app.enable_persistence(SqliteStorage::open(&db).unwrap())
        .unwrap();
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("a.txt"))).unwrap();
    let mut session = Session::with_application(app);
    session
        .dispatch(AppCommand::RenameEntry {
            id: id("a.txt"),
            new_name: "renamed.txt".into(),
        })
        .unwrap();
    assert_eq!(session.snapshot().open_documents, [id("renamed.txt")]);
    assert_eq!(
        SqliteStorage::open(&db)
            .unwrap()
            .session()
            .unwrap()
            .unwrap()
            .documents,
        ["a.txt"]
    );
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("renamed.txt"),
            content: "edited during move".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::SaveDocument(id("renamed.txt")))
        .unwrap();
    gate.open();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.open_documents, [id("a.txt")]);
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "edited during move"
    );
    assert_eq!(
        SqliteStorage::open(&db)
            .unwrap()
            .session()
            .unwrap()
            .unwrap()
            .documents,
        ["a.txt"]
    );
}

#[test]
fn successful_rename_and_partial_file_batches_persist_only_committed_tabs() {
    let root = workspace(&[("a.txt", "a"), ("b.txt", "b")]);
    let mut app = Application::new();
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    assert!(
        app.execute(AppCommand::OpenFiles(vec![
            root.path().join("a.txt"),
            root.path().join("missing.txt")
        ]))
        .is_err()
    );
    assert_eq!(app.storage.session().unwrap().unwrap().documents, ["a.txt"]);
    app.execute(AppCommand::RenameEntry {
        id: id("a.txt"),
        new_name: "renamed.txt".into(),
    })
    .unwrap();
    assert_eq!(
        app.storage.session().unwrap().unwrap().documents,
        ["renamed.txt"]
    );
    app.execute(AppCommand::DeleteEntry(id("renamed.txt")))
        .unwrap();
    assert!(app.storage.session().unwrap().unwrap().documents.is_empty());
}

#[test]
fn quit_stops_on_save_conflicts_keeps_tabs_and_retains_earlier_saves() {
    let root = workspace(&[("a.txt", "a"), ("b.txt", "b")]);
    let mut app = open(root.path(), TestFilesystem::real());
    for name in ["a.txt", "b.txt"] {
        app.execute(AppCommand::OpenDocument(id(name))).unwrap();
        app.execute(AppCommand::ChangeDocument {
            id: id(name),
            content: "edited".into(),
        })
        .unwrap();
    }
    fs::write(root.path().join("b.txt"), "outside").unwrap();
    assert!(matches!(
        app.execute(AppCommand::PrepareToQuit { save: true }),
        Err(AppError::ExternalChanges(_))
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "edited"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("b.txt")).unwrap(),
        "outside"
    );
    assert_eq!(app.snapshot().open_documents, [id("a.txt"), id("b.txt")]);
    assert!(app.snapshot().has_unsaved_documents());
    app.execute(AppCommand::PrepareToQuit { save: false })
        .unwrap();
    assert!(
        app.snapshot().has_unsaved_documents(),
        "discard on quit must not empty the session layout"
    );
}

#[test]
fn quit_is_rejected_during_background_work_even_when_discarding() {
    let root = workspace(&[("a.txt", "a")]);
    let mut session = Session::new();
    session
        .dispatch(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    session.delay_next_background_job(Duration::from_millis(100));
    session.dispatch(AppCommand::RefreshWorkspace).unwrap();
    for save in [false, true] {
        assert!(matches!(
            session.dispatch(AppCommand::PrepareToQuit { save }),
            Err(AppError::OperationInProgress)
        ));
    }
    session.dispatch(AppCommand::RestoreSession).unwrap();
    settle(&mut session);
    session
        .dispatch(AppCommand::PrepareToQuit { save: true })
        .unwrap();
}

struct FailingStorage {
    inner: MemoryStorage,
    fail: Arc<std::sync::atomic::AtomicBool>,
}
impl MetadataStorage for FailingStorage {
    fn session(&self) -> Result<Option<WorkspaceSession>, StorageError> {
        self.inner.session()
    }
    fn save_session(&mut self, session: &WorkspaceSession) -> Result<(), StorageError> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StorageError::Io(io::Error::other("storage unavailable")));
        }
        self.inner.save_session(session)
    }
    fn recent_workspaces(
        &self,
    ) -> Result<Vec<native_workspace_storage::RecentWorkspace>, StorageError> {
        self.inner.recent_workspaces()
    }
    fn record_workspace(
        &mut self,
        workspace: &native_workspace_domain::Workspace,
    ) -> Result<Vec<native_workspace_storage::RecentWorkspace>, StorageError> {
        self.inner.record_workspace(workspace)
    }
}

#[test]
fn metadata_failures_keep_committed_state_and_retry_before_quitting() {
    let root = workspace(&[("a.txt", "a")]);
    let mut app = Application::new();
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    app.storage = Box::new(FailingStorage {
        inner: MemoryStorage::default(),
        fail: fail.clone(),
    });
    assert!(matches!(
        app.execute(AppCommand::OpenWorkspace(root.path().into())),
        Err(AppError::Storage(_))
    ));
    assert!(app.snapshot().workspace.is_some());
    assert!(app.storage.session().unwrap().is_none());
    assert!(
        app.execute(AppCommand::PrepareToQuit { save: false })
            .is_err()
    );
    fail.store(false, std::sync::atomic::Ordering::SeqCst);
    app.execute(AppCommand::PrepareToQuit { save: false })
        .unwrap();
    assert!(app.storage.session().unwrap().is_some());
}

#[test]
fn metadata_failure_after_a_committed_move_keeps_edits_and_saves_at_the_new_path() {
    let root = workspace(&[("a.txt", "a")]);
    let gate = Gate::default();
    let worker_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(_, _)) {
            worker_gate.wait();
        }
        Ok(())
    });
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut app = Application::with_filesystem(Box::new(filesystem));
    app.storage = Box::new(FailingStorage {
        inner: MemoryStorage::default(),
        fail: fail.clone(),
    });
    app.execute(AppCommand::OpenWorkspace(root.path().into()))
        .unwrap();
    app.execute(AppCommand::OpenDocument(id("a.txt"))).unwrap();
    let mut session = Session::with_application(app);
    fail.store(true, std::sync::atomic::Ordering::SeqCst);
    session
        .dispatch(AppCommand::RenameEntry {
            id: id("a.txt"),
            new_name: "renamed.txt".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("renamed.txt"),
            content: "typed during rename".into(),
        })
        .unwrap();
    session
        .dispatch(AppCommand::SaveDocument(id("renamed.txt")))
        .unwrap();
    gate.open();
    let snapshot = settle(&mut session);
    assert!(snapshot.error.is_some());
    assert_eq!(snapshot.open_documents, [id("renamed.txt")]);
    let active = snapshot.active_document.unwrap();
    assert_eq!(&*active.content, "typed during rename");
    assert!(!active.dirty);
    assert!(!root.path().join("a.txt").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("renamed.txt")).unwrap(),
        "typed during rename"
    );
    assert!(
        session
            .dispatch(AppCommand::PrepareToQuit { save: false })
            .is_err()
    );
    fail.store(false, std::sync::atomic::Ordering::SeqCst);
    session
        .dispatch(AppCommand::PrepareToQuit { save: false })
        .unwrap();
}
