mod document_tab_scroll;
mod editor_chrome;
mod material_icons;
mod sidebar_interactions;
#[cfg(debug_assertions)]
mod smoke;
mod syntax;

use adw::prelude::*;
use gtk::{gio, glib};
mod document_tabs;
mod sidebar;
mod ui;
mod window;
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
            if std::env::var("NATIVE_WORKSPACE_NEW_INSTANCE").as_deref() == Ok("1") {
                gio::ApplicationFlags::NON_UNIQUE
            } else {
                gio::ApplicationFlags::FLAGS_NONE
            },
        )
        .build();
    app.connect_activate(build_window);
    app.run();
}
