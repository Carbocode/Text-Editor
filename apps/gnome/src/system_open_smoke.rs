//! Drives GApplication's real open signal, including its cold-start path.
use crate::ui::Ui;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub(super) fn schedule(app: &adw::Application, state: Rc<RefCell<Option<Ui>>>) {
    let Some(path) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_SYSTEM_OPEN") else {
        return;
    };
    let root = PathBuf::from(path);
    let app = app.clone();
    glib::idle_add_local_once(move || {
        let file = gio::File::for_path(root.join("README.md"));
        app.open(std::slice::from_ref(&file), "");
        let ui = state.borrow().clone().expect("open signal creates the UI");
        settle(&ui);
        assert!(ui.snapshot.borrow().error.is_none());
        assert_eq!(
            ui.snapshot.borrow().active_id().unwrap().as_str(),
            "README.md"
        );
        assert!(ui.snapshot.borrow().preview_document.is_none());
        ui.buffer.set_text("unsaved system-open smoke");
        let folder = gio::File::for_path(root.join("src"));
        app.open(std::slice::from_ref(&folder), "");
        settle(&ui);
        assert!(ui.snapshot.borrow().error.is_some());
        assert_eq!(
            &*ui.snapshot
                .borrow()
                .active_document
                .as_ref()
                .unwrap()
                .content,
            "unsaved system-open smoke"
        );
        // Receiving a file while Home is frontmost must raise the workspace.
        ui.window.set_visible(false);
        ui.home_window.present();
        app.open(std::slice::from_ref(&file), "");
        settle(&ui);
        assert!(ui.window.is_visible());
        assert!(!ui.home_window.is_visible());
        assert_eq!(ui.snapshot.borrow().open_documents.len(), 1);
        println!("GNOME system-open cold/warm delivery/dirty-buffer/window smoke test passed");
        app.quit();
    });
}

fn settle(ui: &Ui) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ui.poll_core();
        if !ui.core.borrow().snapshot().activity.busy {
            return;
        }
        assert!(Instant::now() < deadline, "system open did not settle");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(5));
    }
}
