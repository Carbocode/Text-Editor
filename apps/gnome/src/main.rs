mod document_tab_scroll;
mod editor_chrome;
mod material_icons;
#[cfg(debug_assertions)]
mod session_restore_smoke;
mod sidebar_interactions;
#[cfg(debug_assertions)]
mod smoke;
mod syntax;
#[cfg(debug_assertions)]
mod system_open_smoke;
mod terminal;

use adw::prelude::*;
use gtk::{gio, glib};
mod document_tabs;
mod search_panel;
mod sidebar;
mod ui;
mod window;
use native_workspace_application::AppCommand;
use std::{cell::RefCell, rc::Rc};
use window::build_window;

fn main() {
    let resource = gio::Resource::from_data(&glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/icons.gresource"
    ))))
    .expect("bundled GNOME icons");
    gio::resources_register(&resource);
    let app = adw::Application::builder()
        .application_id("org.nativeworkspace.App")
        .flags(
            gio::ApplicationFlags::HANDLES_OPEN
                | if std::env::var("NATIVE_WORKSPACE_NEW_INSTANCE").as_deref() == Ok("1") {
                    gio::ApplicationFlags::NON_UNIQUE
                } else {
                    gio::ApplicationFlags::FLAGS_NONE
                },
        )
        .build();
    let ui = Rc::new(RefCell::new(None::<ui::Ui>));
    #[cfg(debug_assertions)]
    {
        let state = ui.clone();
        app.connect_startup(move |app| {
            system_open_smoke::schedule(app, state.clone());
            session_restore_smoke::schedule(app, state.clone());
        });
    }
    let activate_ui = ui.clone();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            let existing = activate_ui.borrow().clone();
            if let Some(ui) = existing {
                ui.home_window.present();
            } else {
                let ui = build_window(app);
                if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_none()
                    && std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none()
                    && std::env::var_os("NATIVE_WORKSPACE_SMOKE_SYSTEM_OPEN").is_none()
                    && ui.core.borrow().snapshot().error.is_none()
                {
                    ui.dispatch(AppCommand::RestoreSession);
                }
                activate_ui.replace(Some(ui));
            }
        }
    });
    app.connect_open(move |app, files, _| {
        if ui.borrow().is_none() {
            ui.replace(Some(build_window(app)));
        }
        if let Some(ui) = ui.borrow().as_ref() {
            let paths = files
                .iter()
                .map(|file| file.path().unwrap_or_else(|| file.uri().as_str().into()))
                .collect();
            ui.dispatch(AppCommand::OpenPaths(paths));
            if ui.snapshot.borrow().workspace.is_some() {
                ui.home_window.set_visible(false);
                ui.window.present();
            } else {
                ui.home_window.present();
            }
        }
    });
    #[cfg(debug_assertions)]
    if std::env::var("NATIVE_WORKSPACE_SMOKE_RESTORE_MODE").as_deref() == Ok("priority")
        && let Some(root) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_RESTORE")
    {
        let path = std::path::PathBuf::from(root).with_extension("external.txt");
        app.run_with_args(&[
            "native-workspace-gnome",
            path.to_str().expect("UTF-8 smoke fixture"),
        ]);
        return;
    }
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_SYSTEM_OPEN") {
        let path = std::path::PathBuf::from(root).join("README.md");
        app.run_with_args(&[
            "native-workspace-gnome",
            path.to_str().expect("UTF-8 smoke fixture"),
        ]);
        return;
    }
    app.run();
}
