//! Separate seed/restore launches exercise real startup with an isolated metadata DB.
use crate::ui::Ui;
use adw::prelude::*;
use gtk::glib;
use native_workspace_application::AppCommand;
use native_workspace_domain::DocumentId;
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub(super) fn schedule(app: &adw::Application, state: Rc<RefCell<Option<Ui>>>) {
    let Some(root) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_RESTORE") else {
        return;
    };
    assert!(
        std::env::var_os("NATIVE_WORKSPACE_STATE_DB").is_some(),
        "Restore smoke requires an isolated metadata database"
    );
    let mode =
        std::env::var("NATIVE_WORKSPACE_SMOKE_RESTORE_MODE").unwrap_or_else(|_| "restore".into());
    let root = PathBuf::from(root);
    let app = app.clone();
    glib::idle_add_local_once(move || {
        let ui = state.borrow().clone().expect("startup creates the UI");
        settle(&ui);
        if mode == "seed" {
            ui.dispatch(AppCommand::OpenWorkspace(root.clone()));
            ui.dispatch(AppCommand::OpenDocument(
                DocumentId::parse("README.md").unwrap(),
            ));
            ui.dispatch(AppCommand::PreviewDocument(
                DocumentId::parse("src/main.rs").unwrap(),
            ));
            ui.dispatch(AppCommand::OpenFile(root.with_extension("external.txt")));
            settle(&ui);
            ui.dispatch(AppCommand::PreviewDocument(
                DocumentId::parse("README.md").unwrap(),
            ));
            assert!(ui.snapshot.borrow().error.is_none());
        } else if mode == "priority" {
            assert_eq!(ui.snapshot.borrow().open_documents.len(), 1);
            assert_eq!(
                ui.snapshot.borrow().active_id().unwrap().as_str(),
                root.with_extension("external.txt")
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            );
            assert!(ui.snapshot.borrow().error.is_none());
        } else {
            let snapshot = ui.snapshot.borrow().clone();
            assert!(ui.window.is_visible() && !ui.home_window.is_visible());
            assert_eq!(snapshot.active_id().unwrap().as_str(), "README.md");
            if mode == "missing" {
                assert_eq!(snapshot.open_documents.len(), 2);
                assert!(snapshot.error.is_some());
            } else {
                assert_eq!(snapshot.open_documents.len(), 3);
                assert_eq!(snapshot.preview_document.unwrap().as_str(), "src/main.rs");
                assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
                // Native confirmation: cancelling and a save conflict both retain the app and text.
                ui.buffer.set_text("unsaved restore smoke");
                ui.request_quit();
                respond(&ui, "cancel");
                assert!(ui.snapshot.borrow().has_unsaved_documents());
                let previous_disk = std::fs::read_to_string(root.join("README.md")).unwrap();
                std::fs::write(
                    root.join("README.md"),
                    format!("outside edit\n{previous_disk}"),
                )
                .unwrap();
                ui.request_quit();
                respond(&ui, "save");
                assert!(ui.snapshot.borrow().error.is_some());
                assert_eq!(
                    &*ui.snapshot
                        .borrow()
                        .active_document
                        .as_ref()
                        .unwrap()
                        .content,
                    "unsaved restore smoke"
                );
            }
        }
        assert!(
            ui.core
                .borrow_mut()
                .dispatch(AppCommand::PrepareToQuit { save: false })
                .is_ok()
        );
        println!("GNOME session restore ({mode})/native quit confirmation smoke test passed");
        app.quit();
    });
}

fn respond(ui: &Ui, response: &str) {
    let dialog = ui
        .window
        .visible_dialog()
        .expect("quit confirmation is visible");
    dialog.emit_by_name::<()>("response", &[&response]);
    dialog.force_close();
    let deadline = Instant::now() + Duration::from_secs(2);
    while ui.window.visible_dialog().is_some() {
        assert!(Instant::now() < deadline, "dialog did not close");
        glib::MainContext::default().iteration(false);
    }
}

fn settle(ui: &Ui) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ui.poll_core();
        if !ui.core.borrow().snapshot().activity.busy {
            return;
        }
        assert!(Instant::now() < deadline, "session restore did not settle");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(5));
    }
}
