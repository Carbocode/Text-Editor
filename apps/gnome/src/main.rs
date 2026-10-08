use adw::prelude::*;
use gtk::{gio, glib};
use native_workspace_application::{AppCommand, AppSnapshot, Application};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

#[derive(Clone)]
struct Ui {
    window: adw::ApplicationWindow,
    core: Rc<RefCell<Application>>,
    snapshot: Rc<RefCell<AppSnapshot>>,
    files: adw::Sidebar,
    search: gtk::SearchEntry,
    files_placeholder: gtk::Label,
    editor: gtk::TextView,
    buffer: gtk::TextBuffer,
    heading: gtk::Label,
    status: gtk::Label,
    counter: gtk::Label,
    save: gtk::Button,
    recent_button: gtk::MenuButton,
    recent_list: gtk::Box,
    updating: Rc<Cell<bool>>,
}

impl Ui {
    fn dispatch(&self, command: AppCommand) {
        let result = self.core.borrow_mut().dispatch(command);
        match result {
            Ok(snapshot) => self.render(snapshot),
            Err(error) => self.status.set_label(&error.to_string()),
        }
    }

    fn render(&self, snapshot: AppSnapshot) {
        self.updating.set(true);
        let results_changed = self.snapshot.borrow().search_results != snapshot.search_results;
        let recent_changed = self.snapshot.borrow().recent_workspaces != snapshot.recent_workspaces;
        self.window.set_title(Some(
            snapshot
                .workspace_name
                .as_deref()
                .unwrap_or("Native Workspace"),
        ));
        self.heading.set_label(
            snapshot
                .workspace_name
                .as_deref()
                .unwrap_or("Open a workspace"),
        );
        if self.search.text().as_str() != snapshot.search_query.0 {
            self.search.set_text(&snapshot.search_query.0);
        }
        self.files_placeholder
            .set_label(if snapshot.workspace_name.is_none() {
                "Open a workspace"
            } else {
                "No matching files"
            });
        self.counter
            .set_label(&format!("Counter: {}", snapshot.diagnostic_counter));
        self.save.set_sensitive(
            snapshot
                .active_document
                .as_ref()
                .is_some_and(|doc| doc.dirty),
        );
        self.editor.set_editable(snapshot.active_document.is_some());
        self.status
            .set_label(match snapshot.active_document.as_ref() {
                Some(doc) if doc.dirty => "Modified",
                Some(_) => "Saved",
                None => "Select a document",
            });
        self.recent_button
            .set_sensitive(!snapshot.recent_workspaces.is_empty());
        if recent_changed {
            while let Some(child) = self.recent_list.first_child() {
                self.recent_list.remove(&child);
            }
            for workspace in &snapshot.recent_workspaces {
                let button = gtk::Button::with_label(&workspace.name);
                button.set_tooltip_text(Some(&workspace.root.to_string_lossy()));
                button.add_css_class("flat");
                button.set_halign(gtk::Align::Fill);
                let ui = self.clone();
                let path = workspace.root.clone();
                button
                    .connect_clicked(move |_| ui.dispatch(AppCommand::OpenWorkspace(path.clone())));
                self.recent_list.append(&button);
            }
        }
        if results_changed {
            self.files.remove_all();
            let section = adw::SidebarSection::new();
            for document in &snapshot.search_results {
                let item = adw::SidebarItem::new(&document.title);
                item.set_icon_paintable(Some(&icon_for(document.id.as_str())));
                item.set_subtitle(document.snippet.as_deref());
                if document.dirty {
                    item.set_suffix(Some(&gtk::Label::new(Some("●"))));
                }
                section.append(item);
            }
            self.files.append(section);
        }
        if let Some(active) = snapshot.active_document.as_ref() {
            if let Some(index) = snapshot
                .search_results
                .iter()
                .position(|doc| doc.id == active.id)
            {
                self.files.set_selected(index as u32);
            }
        } else {
            self.files.set_selected(u32::MAX);
        }
        let content = snapshot
            .active_document
            .as_ref()
            .map(|doc| doc.content.as_str())
            .unwrap_or("");
        let (start, end) = self.buffer.bounds();
        if self.buffer.text(&start, &end, false).as_str() != content {
            self.buffer.set_text(content);
        }
        self.snapshot.replace(snapshot);
        self.updating.set(false);
    }
}

fn icon_for(name: &str) -> gtk::gdk::Texture {
    let bytes: &'static [u8] = if name.eq_ignore_ascii_case("readme.md") {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/material-icon-theme/readme.png"
        ))
    } else if name.to_ascii_lowercase().ends_with(".md") {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/material-icon-theme/markdown.png"
        ))
    } else {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/material-icon-theme/document.png"
        ))
    };
    gtk::gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes)).expect("bundled PNG icon")
}

fn main() {
    let app = adw::Application::builder()
        .application_id("org.nativeworkspace.App")
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();
    app.connect_activate(build_window);
    app.run();
}

fn build_window(app: &adw::Application) {
    let mut core = Application::new();
    let persistence_error = if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_none()
        && std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none()
    {
        core.enable_persistence_default()
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    let snapshot = core.snapshot();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Native Workspace")
        .default_width(900)
        .default_height(600)
        .build();
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&gtk::Label::new(None)));
    let content_header = adw::HeaderBar::new();
    let toggle_sidebar = gtk::ToggleButton::new();
    let icons = gtk::IconTheme::for_display(&gtk::gdk::Display::default().expect("GTK display"));
    let sidebar_icon = if icons.has_icon("adw-sidebar-symbolic") {
        "adw-sidebar-symbolic"
    } else if icons.has_icon("sidebar-show-symbolic") {
        "sidebar-show-symbolic"
    } else {
        "view-list-symbolic"
    };
    toggle_sidebar.set_icon_name(sidebar_icon);
    toggle_sidebar.set_tooltip_text(Some("Toggle Sidebar"));
    content_header.pack_start(&toggle_sidebar);
    let open = gtk::Button::from_icon_name("document-open-symbolic");
    open.set_tooltip_text(Some("Open Workspace"));
    let save = gtk::Button::new();
    let save_content = adw::ButtonContent::new();
    save_content.set_icon_name("document-save-symbolic");
    save_content.set_label("Save");
    save.set_child(Some(&save_content));
    save.set_sensitive(false);
    sidebar_header.pack_start(&open);
    let recent_button = gtk::MenuButton::new();
    recent_button.set_icon_name("document-open-recent-symbolic");
    recent_button.set_tooltip_text(Some("Recent Workspaces"));
    recent_button.set_sensitive(false);
    let recent_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    recent_list.set_margin_top(6);
    recent_list.set_margin_bottom(6);
    recent_list.set_margin_start(6);
    recent_list.set_margin_end(6);
    let recent_popover = gtk::Popover::new();
    recent_popover.set_child(Some(&recent_list));
    recent_button.set_popover(Some(&recent_popover));
    sidebar_header.pack_start(&recent_button);
    content_header.pack_end(&save);
    let heading = gtk::Label::new(Some("Open a workspace"));
    content_header.set_title_widget(Some(&heading));
    let files = adw::Sidebar::new();
    files.add_css_class("compact-files");
    files.set_size_request(220, -1);
    let files_placeholder = gtk::Label::new(Some("Open a workspace"));
    files.set_placeholder(Some(&files_placeholder));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search files"));
    search.set_margin_top(8);
    search.set_margin_bottom(8);
    search.set_margin_start(8);
    search.set_margin_end(8);
    let sidebar_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_content.append(&search);
    files.set_vexpand(true);
    sidebar_content.append(&files);
    let sidebar_style = gtk::CssProvider::new();
    sidebar_style.load_from_data(include_str!("sidebar.css"));
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &sidebar_style,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let editor = gtk::TextView::new();
    editor.set_monospace(true);
    editor.set_editable(false);
    editor.set_wrap_mode(gtk::WrapMode::None);
    editor.set_top_margin(16);
    editor.set_left_margin(16);
    editor.set_right_margin(16);
    let buffer = editor.buffer();
    let editor_scroll = gtk::ScrolledWindow::new();
    editor_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    editor_scroll.set_child(Some(&editor));
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(&sidebar_content));
    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.set_content(Some(&editor_scroll));
    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar_view));
    split.set_content(Some(&content_view));
    split.set_sidebar_width_fraction(0.28);
    split.set_vexpand(true);
    toggle_sidebar.set_active(split.shows_sidebar());
    {
        let split = split.clone();
        toggle_sidebar.connect_toggled(move |button| {
            split.set_show_sidebar(button.is_active());
        });
    }
    {
        let toggle_sidebar = toggle_sidebar.clone();
        split.connect_show_sidebar_notify(move |split| {
            toggle_sidebar.set_active(split.shows_sidebar());
        });
    }
    let status = gtk::Label::new(Some("Select a document"));
    let counter = gtk::Label::new(Some("Counter: 0"));
    let diagnostic = gtk::Button::with_label("Increment");
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_top(8);
    footer.set_margin_bottom(8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    status.set_hexpand(true);
    status.set_xalign(0.0);
    footer.append(&status);
    if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_some() {
        footer.append(&counter);
        footer.append(&diagnostic);
    }
    content_view.add_bottom_bar(&footer);
    window.set_content(Some(&split));

    let ui = Ui {
        window: window.clone(),
        core: Rc::new(RefCell::new(core)),
        snapshot: Rc::new(RefCell::new(Application::new().snapshot())),
        files,
        search,
        files_placeholder,
        editor,
        buffer,
        heading,
        status,
        counter,
        save,
        recent_button,
        recent_list,
        updating: Rc::new(Cell::new(false)),
    };
    ui.render(snapshot);
    if let Some(error) = persistence_error {
        ui.status.set_label(&error);
    }
    {
        let ui = ui.clone();
        let window = window.clone();
        open.connect_clicked(move |_| {
            let dialog = gtk::FileChooserNative::new(
                Some("Open Workspace"),
                Some(&window),
                gtk::FileChooserAction::SelectFolder,
                Some("Open"),
                Some("Cancel"),
            );
            let ui = ui.clone();
            dialog.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept
                    && let Some(path) = dialog.file().and_then(|file| file.path())
                {
                    ui.dispatch(AppCommand::OpenWorkspace(path));
                }
                dialog.destroy();
            });
            dialog.show();
        });
    }
    {
        let ui = ui.clone();
        ui.search.clone().connect_search_changed(move |search| {
            if !ui.updating.get() {
                ui.dispatch(AppCommand::SetSearchQuery(search.text().to_string()));
            }
        });
    }
    {
        let ui = ui.clone();
        ui.files.clone().connect_selected_notify(move |sidebar| {
            if ui.updating.get() {
                return;
            }
            let id = ui
                .snapshot
                .borrow()
                .search_results
                .get(sidebar.selected() as usize)
                .map(|doc| doc.id.clone());
            if let Some(id) = id {
                ui.dispatch(AppCommand::OpenDocument(id));
            }
        });
    }
    {
        let ui = ui.clone();
        ui.buffer.clone().connect_changed(move |buffer| {
            if ui.updating.get() {
                return;
            }
            let id = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .map(|doc| doc.id.clone());
            if let Some(id) = id {
                let (start, end) = buffer.bounds();
                ui.dispatch(AppCommand::ChangeDocument {
                    id,
                    content: buffer.text(&start, &end, false).to_string(),
                });
            }
        });
    }
    {
        let ui = ui.clone();
        ui.save.clone().connect_clicked(move |_| {
            let id = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .map(|doc| doc.id.clone());
            if let Some(id) = id {
                ui.dispatch(AppCommand::SaveDocument(id));
            }
        });
    }
    {
        let ui = ui.clone();
        diagnostic.connect_clicked(move |_| ui.dispatch(AppCommand::IncrementDiagnostic));
    }
    window.present();
    if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_some() {
        assert!(icons.has_icon("document-open-symbolic"));
        assert!(icons.has_icon("document-save-symbolic"));
        assert!(icons.has_icon(sidebar_icon));
        assert!(sidebar_header.shows_start_title_buttons());
        assert!(sidebar_header.shows_end_title_buttons());
        assert!(content_header.shows_start_title_buttons());
        assert!(content_header.shows_end_title_buttons());
        let initial = split.shows_sidebar();
        toggle_sidebar.emit_clicked();
        assert_ne!(split.shows_sidebar(), initial);
        toggle_sidebar.emit_clicked();
        assert_eq!(split.shows_sidebar(), initial);
        diagnostic.emit_clicked();
        assert_eq!(ui.counter.text(), "Counter: 1");
        println!("GTK → Rust smoke test passed");
        app.quit();
    }
    if let Some(path) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE") {
        ui.dispatch(AppCommand::OpenWorkspace(PathBuf::from(path)));
        assert_eq!(
            window.title().as_deref(),
            ui.snapshot.borrow().workspace_name.as_deref()
        );
        let first = ui
            .snapshot
            .borrow()
            .documents
            .first()
            .map(|doc| doc.id.clone());
        if let Some(id) = first {
            ui.files.set_selected(0);
            assert_eq!(
                ui.snapshot
                    .borrow()
                    .active_document
                    .as_ref()
                    .map(|doc| &doc.id),
                Some(&id)
            );
            ui.buffer.set_text(&"M".repeat(500));
            let horizontal = editor_scroll.hadjustment();
            for _ in 0..100 {
                glib::MainContext::default().iteration(false);
                if horizontal.upper() > horizontal.page_size() {
                    break;
                }
            }
            assert!(horizontal.upper() > horizontal.page_size());
            ui.buffer.set_text("GTK Phase 1 smoke content");
            ui.save.emit_clicked();
            assert_eq!(ui.status.text(), "Saved");
            ui.dispatch(AppCommand::SetSearchQuery("phase 1".into()));
            assert_eq!(ui.snapshot.borrow().search_results.len(), 1);
            assert_eq!(
                ui.snapshot.borrow().search_results[0].snippet.as_deref(),
                Some("GTK Phase 1 smoke content")
            );
            ui.dispatch(AppCommand::SetSearchQuery("unlikely-no-match".into()));
            assert!(ui.snapshot.borrow().search_results.is_empty());
            assert_eq!(ui.search.text(), "unlikely-no-match");
            ui.dispatch(AppCommand::SetSearchQuery(String::new()));
            assert_eq!(ui.snapshot.borrow().search_results.len(), 1);
            println!("GTK workspace edit/save smoke test passed");
        }
        app.quit();
    }
}
