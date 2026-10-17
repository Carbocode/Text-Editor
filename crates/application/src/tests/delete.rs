use super::*;
use std::sync::mpsc;

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
    search(&mut app, "needle");
    let (events, snapshot) = outcome(&mut app, AppCommand::DeleteEntry(id("notes"))).unwrap();
    assert!(
        matches!(&events[0], AppEvent::EntryDeleted { id: deleted } if deleted == &id("notes"))
    );
    assert!(!dir.path().join("notes").exists());
    assert_eq!(snapshot.active_document.unwrap().id, id("notes-other"));
    assert_eq!(snapshot.open_documents, [id("notes-other")]);
    assert!(snapshot.directories.is_empty());
    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(snapshot.documents[0].id, id("notes-other"));
    assert!(snapshot.documents[0].dirty);
    let searched = wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |snapshot| !snapshot.search_in_progress,
    );
    assert!(searched.search_results.is_empty());
    let kept = app
        .dispatch(AppCommand::OpenDocument(id("notes-other")))
        .unwrap();
    assert_eq!(&*kept.active_document.unwrap().content, "unsaved");
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

#[test]
fn session_deletes_off_thread_pauses_editing_and_reports_failures() {
    for fail in [false, true] {
        let dir = workspace(&[("a", "saved")]);
        let gate = Gate::default();
        let (started, observed) = mpsc::channel();
        let hook_gate = gate.clone();
        let started = std::sync::Mutex::new(started);
        let filesystem = TestFilesystem::with(move |operation| match operation {
            Operation::Delete(_) => {
                started.lock().unwrap().send(()).unwrap();
                hook_gate.wait();
                if fail {
                    Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        });
        let mut core = open(dir.path(), filesystem);
        core.dispatch(AppCommand::OpenDocument(id("a"))).unwrap();
        let mut session = Session::with_application(core);
        let before = session.snapshot();
        session.dispatch(AppCommand::DeleteEntry(id("a"))).unwrap();
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let deleting = session.snapshot();
        assert!(deleting.activity.editing_paused);
        assert_eq!(deleting.documents, before.documents);
        assert!(matches!(
            session.dispatch(AppCommand::ChangeDocument {
                id: id("a"),
                content: "late edit".into()
            }),
            Err(AppError::OperationInProgress)
        ));
        gate.open();
        let done = settle(&mut session);
        if fail {
            assert_eq!(done.documents, before.documents);
            assert_eq!(done.active_document, before.active_document);
            assert!(done.error.unwrap().contains("denied"));
            assert!(dir.path().join("a").exists());
        } else {
            assert!(done.documents.is_empty());
            assert!(done.active_document.is_none());
            assert_eq!(
                done.error.as_deref(),
                Some(AppError::OperationInProgress.to_string().as_str())
            );
            assert!(!dir.path().join("a").exists());
        }
    }
}

#[test]
fn deletion_waits_for_a_pending_move_instead_of_queueing_a_destructive_command() {
    let dir = workspace(&[("a", "saved"), ("destination/.keep", "")]);
    let gate = Gate::default();
    let hook_gate = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(..)) {
            hook_gate.wait();
        }
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    session
        .dispatch(AppCommand::MoveEntry {
            id: id("a"),
            directory: "destination".into(),
        })
        .unwrap();
    assert!(matches!(
        session.dispatch(AppCommand::DeleteEntry(id("destination/a"))),
        Err(AppError::OperationInProgress)
    ));
    gate.open();
    let done = settle(&mut session);
    assert_eq!(
        fs::read_to_string(dir.path().join("destination/a")).unwrap(),
        "saved"
    );
    assert!(done.document_summary("destination/a").is_some());
}
