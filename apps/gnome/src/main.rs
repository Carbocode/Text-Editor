use adw::prelude::*;
use gtk::gio;
use native_workspace_application::{AppCommand, Application};
use std::{cell::RefCell, rc::Rc};

fn main() {
    let app = adw::Application::builder()
        .application_id("org.nativeworkspace.Diagnostic")
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();
    app.connect_activate(build_window);
    app.run();
}

fn build_window(app: &adw::Application) {
    let core = Rc::new(RefCell::new(Application::new()));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Native Workspace")
        .default_width(360)
        .default_height(200)
        .build();
    let header = adw::HeaderBar::new();
    let label = gtk::Label::new(Some("Counter: 0"));
    let button = gtk::Button::with_label("Increment");
    button.add_css_class("suggested-action");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&label);
    content.append(&button);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&content);
    window.set_content(Some(&root));
    let label_for_click = label.clone();
    button.connect_clicked(move |_| {
        match core.borrow_mut().dispatch(AppCommand::IncrementDiagnostic) {
            Ok(snapshot) => {
                label_for_click.set_label(&format!("Counter: {}", snapshot.diagnostic_counter))
            }
            Err(error) => eprintln!("Increment failed: {error:?}"),
        }
    });
    window.present();
    if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_some() {
        button.emit_clicked();
        assert_eq!(label.text(), "Counter: 1");
        println!("GTK → Rust smoke test passed");
        app.quit();
    }
}
