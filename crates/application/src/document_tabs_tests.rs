use super::*;
use std::fs;

fn id(path: &str) -> DocumentId {
    DocumentId::parse(path).unwrap()
}
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
    assert_eq!(app.snapshot().active_document.unwrap().content, "unsaved b");
    app.dispatch(AppCommand::DiscardAndCloseDocument(id("b")))
        .unwrap();
    assert!(app.snapshot().open_documents.is_empty());
    assert!(app.snapshot().active_document.is_none());
    let reopened = app.dispatch(AppCommand::OpenDocument(id("b"))).unwrap();
    assert_eq!(reopened.active_document.unwrap().content, "original b");
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
    assert_eq!(reopened.active_document.unwrap().content, "new disk a");
    assert_eq!(reopened.open_documents, [id("renamed"), id("c"), id("a")]);
}

#[test]
fn native_adapter_publishes_completed_saves_when_save_all_fails() {
    let (dir, mut app) = fixture();
    for name in ["b", "a"] {
        app.dispatch(AppCommand::ChangeDocument {
            id: id(name),
            content: format!("edited {name}"),
        })
        .unwrap();
    }
    fs::write(dir.path().join("a"), "external a").unwrap();
    let mut native = NativeApplication::with_application(app);
    assert!(matches!(
        native.dispatch(AppCommand::SaveAllDocuments),
        Err(AppError::ExternalChanges(_))
    ));
    let snapshot = native.snapshot();
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
