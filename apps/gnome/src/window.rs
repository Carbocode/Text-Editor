//! Window construction and native signal wiring.
use crate::document_tabs::tab_page_at_widget;
use crate::sidebar::{build_sidebar, sidebar_resize_handle};
#[cfg(debug_assertions)]
use crate::smoke;
use crate::ui::{SidebarRow, SidebarTab, Ui};
use crate::{document_tab_scroll, editor_chrome, sidebar_interactions, syntax};
use adw::prelude::*;
use gtk::{gio, glib};
use native_workspace_application::{AppCommand, Session};
use native_workspace_domain::DocumentId;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn icon_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon_name));
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    content.append(&text);
    button.set_child(Some(&content));
    button
}

fn primary_accelerator(key: &str) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "<Meta>"
    } else {
        "<Control>"
    };
    format!("{modifier}{key}")
}

fn editor_font_points(editor: &gtk::TextView) -> f64 {
    editor
        .pango_context()
        .font_description()
        .map(|font| {
            let size = f64::from(font.size()) / f64::from(gtk::pango::SCALE);
            if font.is_size_absolute() {
                let dpi = f64::from(editor.settings().gtk_xft_dpi()) / 1024.0;
                size * 72.0 / if dpi > 0.0 { dpi } else { 96.0 }
            } else {
                size
            }
        })
        .filter(|size| *size > 0.0)
        .unwrap_or(12.0)
}

fn application_menu(devtools_available: bool) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_tooltip_text(Some("Main Menu"));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);
    let popover = gtk::Popover::new();
    for (label, icon, action) in [
        ("Home", "user-home-symbolic", "app.home"),
        ("Developer Tools", "system-run-symbolic", "app.devtools"),
        ("Open Folder…", "document-open-symbolic", "app.open"),
        ("Quit", "native-workspace-quit-symbolic", "app.quit"),
    ] {
        if action == "app.devtools" && !devtools_available {
            continue;
        }
        if action == "app.quit" {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let item = icon_button(label, icon);
        item.add_css_class("flat");
        item.set_action_name(Some(action));
        let popover = popover.clone();
        item.connect_clicked(move |_| popover.popdown());
        content.append(&item);
    }
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));
    button
}

pub(super) fn build_window(app: &adw::Application) -> Ui {
    let mut core = Session::new();
    core.enable_devtools("gtk");
    let persistence_error = if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_none()
        && std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none()
        && std::env::var_os("NATIVE_WORKSPACE_SMOKE_SYSTEM_OPEN").is_none()
    {
        core.enable_persistence_default()
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    let devtools_available = Session::devtools_supported();
    let snapshot = core.snapshot();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Native Workspace")
        .default_width(1280)
        .default_height(800)
        .build();
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&gtk::Label::new(None)));
    let content_header = adw::HeaderBar::new();
    let toggle_sidebar = gtk::ToggleButton::new();
    let icons = gtk::IconTheme::for_display(&gtk::gdk::Display::default().expect("GTK display"));
    icons.add_resource_path("/org/nativeworkspace/icons");
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
    if devtools_available {
        let devtools_action = gio::SimpleAction::new("devtools", None);
        app.add_action(&devtools_action);
    }
    let open = icon_button("Open Folder…", "document-open-symbolic");
    open.set_action_name(Some("app.open"));
    content_header.pack_end(&application_menu(devtools_available));
    let recent_list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    recent_list.set_margin_top(6);
    recent_list.set_margin_bottom(6);
    recent_list.set_margin_start(6);
    recent_list.set_margin_end(6);
    let heading = gtk::Label::new(Some("Open a workspace"));
    content_header.set_title_widget(Some(&heading));
    let current_snapshot = Rc::new(RefCell::new(Session::new().snapshot()));
    let sidebar_tab = Rc::new(Cell::new(SidebarTab::Files));
    let file_query = Rc::new(RefCell::new(String::new()));
    let collapsed_directories = Rc::new(RefCell::new(std::collections::HashSet::new()));
    let pending_move_target = Rc::new(RefCell::new(None));
    let open_documents = Rc::new(RefCell::new(std::collections::HashSet::new()));
    let sidebars = [
        build_sidebar(
            &collapsed_directories,
            &file_query,
            false,
            &pending_move_target,
            &open_documents,
        ),
        build_sidebar(
            &collapsed_directories,
            &file_query,
            true,
            &pending_move_target,
            &open_documents,
        ),
    ];
    let files_scroll = sidebars[0].scroll.clone();
    let search = gtk::SearchEntry::new();
    let search_spinner = gtk::Spinner::new();
    search_spinner.set_visible(false);
    search.set_placeholder_text(Some("Search in files"));
    let file_search = gtk::SearchEntry::new();
    file_search.set_placeholder_text(Some("Filter names"));
    // The core's search and name-filter workers already debounce for 150 ms;
    // GTK's own delay would double the wait before results appear.
    search.set_search_delay(0);
    file_search.set_search_delay(0);
    let sidebar_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_content.add_css_class("workspace-sidebar");
    let tab_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tab_row.add_css_class("linked");
    tab_row.set_homogeneous(true);
    tab_row.set_margin_start(8);
    tab_row.set_margin_end(8);
    tab_row.set_margin_top(6);
    tab_row.set_margin_bottom(6);
    let file_tab = gtk::ToggleButton::new();
    file_tab.set_icon_name("folder-symbolic");
    file_tab.set_tooltip_text(Some("Files"));
    file_tab.set_hexpand(true);
    file_tab.set_active(true);
    let text_tab = gtk::ToggleButton::new();
    text_tab.set_icon_name("edit-find-symbolic");
    text_tab.set_tooltip_text(Some("Search in files"));
    text_tab.set_hexpand(true);
    text_tab.set_group(Some(&file_tab));
    tab_row.append(&file_tab);
    tab_row.append(&text_tab);
    let extra_tabs = [
        ("Git", "native-workspace-branch-symbolic"),
        ("Bookmarks", "native-workspace-bookmark-symbolic"),
        ("Warnings", "dialog-warning-symbolic"),
        ("Errors", "dialog-error-symbolic"),
    ];
    let mut empty_tabs = Vec::new();
    for (title, icon_name) in extra_tabs {
        let tab = gtk::ToggleButton::new();
        tab.set_icon_name(icon_name);
        tab.set_tooltip_text(Some(title));
        tab.set_hexpand(true);
        tab.set_group(Some(&file_tab));
        tab_row.append(&tab);
        empty_tabs.push(tab);
    }
    sidebar_content.append(&tab_row);
    let search_controls = crate::search_panel::SearchControls::new(&search, &search_spinner);
    let text_search_row = search_controls.root.clone();
    text_search_row.set_visible(false);
    sidebar_content.append(&text_search_row);
    let search_truncated = gtk::Label::new(Some(
        "Results are incomplete. Refresh or refine the search before replacing.",
    ));
    search_truncated.set_wrap(true);
    search_truncated.set_xalign(0.0);
    search_truncated.set_margin_start(8);
    search_truncated.set_margin_end(8);
    search_truncated.set_margin_bottom(6);
    search_truncated.add_css_class("dim-label");
    search_truncated.add_css_class("caption");
    search_truncated.set_visible(false);
    sidebar_content.append(&search_truncated);
    let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    search_row.set_margin_start(8);
    search_row.set_margin_end(8);
    search_row.set_margin_top(8);
    search_row.set_margin_bottom(8);
    let add_button = gtk::MenuButton::new();
    add_button.set_icon_name("list-add-symbolic");
    add_button.set_tooltip_text(Some("Add file or folder"));
    let add_menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let new_file = icon_button("New File", "native-workspace-new-file-symbolic");
    let new_folder = icon_button("New Folder", "folder-new-symbolic");
    new_file.add_css_class("flat");
    new_folder.add_css_class("flat");
    add_menu.append(&new_file);
    add_menu.append(&new_folder);
    let add_popover = gtk::Popover::new();
    add_popover.set_child(Some(&add_menu));
    add_button.set_popover(Some(&add_popover));
    search_row.append(&add_button);
    file_search.set_hexpand(true);
    search_row.append(&file_search);
    let sidebar_pages = gtk::Stack::new();
    sidebar_pages.set_vexpand(true);
    sidebar_pages.set_transition_type(gtk::StackTransitionType::None);
    sidebar_pages.add_named(&sidebars[0].overlay, Some("files"));
    sidebar_pages.add_named(&sidebars[1].overlay, Some("search"));
    sidebar_pages.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("empty"));
    sidebar_content.append(&sidebar_pages);
    sidebar_content.append(&search_row);
    let sidebar_style = gtk::CssProvider::new();
    sidebar_style.load_from_data(include_str!("sidebar.css"));
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &sidebar_style,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let editor = gtk::TextView::new();
    editor.set_monospace(true);
    editor.add_css_class("workspace-editor-text");
    let editor_font_size = Rc::new(Cell::new(None::<f64>));
    let editor_base_font_size = Rc::new(Cell::new(None::<f64>));
    let editor_font_style = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &editor_font_style,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
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
    let conflict_banner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    conflict_banner.add_css_class("warning");
    conflict_banner.set_margin_start(8);
    conflict_banner.set_margin_end(8);
    conflict_banner.set_margin_top(8);
    conflict_banner.set_margin_bottom(8);
    let conflict_label = gtk::Label::new(Some("This file changed on disk"));
    conflict_label.set_hexpand(true);
    conflict_label.set_xalign(0.0);
    conflict_banner.append(&conflict_label);
    let reload = icon_button("Reload (discard edits)", "view-refresh-symbolic");
    let overwrite = icon_button("Overwrite disk", "document-save-symbolic");
    conflict_banner.append(&reload);
    conflict_banner.append(&overwrite);
    conflict_banner.set_visible(false);
    let content_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let document_tabs = adw::TabView::new();
    document_tabs.set_vexpand(true);
    document_tabs
        .remove_shortcuts(adw::TabViewShortcuts::CONTROL_HOME | adw::TabViewShortcuts::CONTROL_END);
    let tab_bar = adw::TabBar::new();
    tab_bar.set_view(Some(&document_tabs));
    tab_bar.add_css_class("workspace-document-tabs");
    tab_bar.set_autohide(false);
    let document_tab_scroll =
        document_tab_scroll::TouchpadScroll::install(&tab_bar, &document_tabs);
    let save_all = gtk::Button::from_icon_name("native-workspace-save-all-symbolic");
    save_all.set_tooltip_text(Some("Save All"));
    save_all.set_action_name(Some("app.save-all"));
    content_header.pack_end(&save_all);
    let save = gtk::Button::from_icon_name("document-save-symbolic");
    save.set_tooltip_text(Some("Save"));
    save.set_action_name(Some("app.save"));
    content_header.pack_end(&save);
    let decrease_text_size = gtk::Button::from_icon_name("zoom-out-symbolic");
    decrease_text_size.set_tooltip_text(Some("Decrease Text Size"));
    let increase_text_size = gtk::Button::from_icon_name("zoom-in-symbolic");
    increase_text_size.set_tooltip_text(Some("Increase Text Size"));
    let text_zoom = gtk::Label::new(Some("100%"));
    text_zoom.set_width_chars(4);
    text_zoom.set_margin_start(6);
    text_zoom.set_margin_end(6);
    text_zoom.set_tooltip_text(Some("Text Zoom"));
    let text_size_controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    text_size_controls.add_css_class("linked");
    text_size_controls.append(&decrease_text_size);
    text_size_controls.append(&text_zoom);
    text_size_controls.append(&increase_text_size);
    content_header.pack_end(&text_size_controls);
    // Share libadwaita's header surface and backdrop colors with the header bar.
    content_view.add_top_bar(&tab_bar);
    let editor_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor_panel.set_vexpand(true);
    editor_panel.append(&conflict_banner);
    editor_scroll.set_vexpand(true);
    let editor_chrome = editor_chrome::wrap(&editor, &editor_scroll);
    editor_panel.append(&editor_chrome.row);
    let empty_editor_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let hub = gtk::Box::new(gtk::Orientation::Vertical, 24);
    hub.set_widget_name("workspaceHub");
    hub.set_halign(gtk::Align::Center);
    hub.set_valign(gtk::Align::Center);
    hub.set_margin_top(32);
    hub.set_margin_bottom(32);
    hub.set_margin_start(32);
    hub.set_margin_end(32);
    let icon = gtk::Image::from_icon_name("text-x-generic-symbolic");
    icon.set_pixel_size(40);
    icon.set_halign(gtk::Align::Start);
    icon.add_css_class("dim-label");
    hub.append(&icon);
    let title = gtk::Label::new(Some("No Document Open"));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    hub.append(&title);
    let description = gtk::Label::new(Some("Choose a file from the sidebar to start editing."));
    description.add_css_class("dim-label");
    description.set_wrap(true);
    description.set_xalign(0.0);
    hub.append(&description);
    let open_folder = icon_button("Open Folder…", "document-open-symbolic");
    open_folder.add_css_class("suggested-action");
    open_folder.set_halign(gtk::Align::Start);
    open_folder.set_action_name(Some("app.open"));
    hub.append(&open_folder);
    hub.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let shortcuts_title = gtk::Label::new(Some("Keyboard Shortcuts"));
    shortcuts_title.add_css_class("heading");
    shortcuts_title.set_xalign(0.0);
    hub.append(&shortcuts_title);
    let shortcuts = gtk::Grid::new();
    shortcuts.set_column_spacing(32);
    shortcuts.set_row_spacing(12);
    for (row, (label, accelerator)) in [
        ("Open Folder", "o"),
        ("Save", "s"),
        ("Save All", "<Shift>s"),
    ]
    .into_iter()
    .enumerate()
    {
        let label = gtk::Label::new(Some(label));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.add_css_class("dim-label");
        shortcuts.attach(&label, 0, row as i32, 1, 1);
        shortcuts.attach(
            &gtk::ShortcutLabel::new(&primary_accelerator(accelerator)),
            1,
            row as i32,
            1,
            1,
        );
    }
    hub.append(&shortcuts);
    let hub_scroll = gtk::ScrolledWindow::new();
    hub_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    hub_scroll.set_vexpand(true);
    hub_scroll.set_child(Some(&hub));
    empty_editor_page.append(&hub_scroll);
    empty_editor_page.append(&editor_panel);
    let editor_stack = gtk::Stack::new();
    editor_stack.set_vexpand(true);
    editor_stack.add_named(&empty_editor_page, Some("empty"));
    editor_stack.add_named(&document_tabs, Some("documents"));
    content_body.append(&editor_stack);
    content_view.set_content(Some(&content_body));
    let split = adw::OverlaySplitView::new();
    let sidebar_frame = gtk::Overlay::new();
    sidebar_frame.set_child(Some(&sidebar_view));
    split.set_sidebar(Some(&sidebar_frame));
    split.set_content(Some(&content_view));
    split.set_sidebar_width_fraction(0.28);
    split.set_vexpand(true);
    let sidebar_resize = sidebar_resize_handle(&split);
    sidebar_frame.add_overlay(&sidebar_resize);
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
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_top(8);
    footer.set_margin_bottom(8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    status.set_hexpand(true);
    status.set_xalign(0.0);
    footer.append(&status);
    content_view.add_bottom_bar(&footer);
    let home_window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Native Workspace")
        .default_width(640)
        .default_height(560)
        .build();
    let home_header = adw::HeaderBar::new();
    home_header.set_decoration_layout(Some("menu:minimize,maximize,close"));
    home_header.pack_end(&application_menu(devtools_available));
    let home_view = adw::ToolbarView::new();
    home_view.add_top_bar(&home_header);
    let home = gtk::Box::new(gtk::Orientation::Vertical, 20);
    home.set_margin_top(24);
    home.set_margin_bottom(24);
    home.set_margin_start(28);
    home.set_margin_end(28);
    let title = gtk::Label::new(Some("Native Workspace"));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    home.append(&title);
    let subtitle = gtk::Label::new(Some("A place for your files and ideas."));
    subtitle.add_css_class("dim-label");
    subtitle.set_xalign(0.0);
    home.append(&subtitle);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    open.add_css_class("suggested-action");
    actions.append(&open);
    let resume = icon_button("Resume Workspace", "go-next-symbolic");
    actions.append(&resume);
    home.append(&actions);
    let recent_title = gtk::Label::new(Some("Recent Workspaces"));
    recent_title.add_css_class("heading");
    recent_title.set_xalign(0.0);
    home.append(&recent_title);
    let recent_empty =
        gtk::Label::new(Some("Open a folder. It will appear here for quick access."));
    recent_empty.set_wrap(true);
    recent_empty.add_css_class("dim-label");
    home.append(&recent_empty);
    let recent_scroll = gtk::ScrolledWindow::new();
    recent_scroll.set_vexpand(true);
    recent_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    recent_list.add_css_class("boxed-list");
    recent_scroll.set_child(Some(&recent_list));
    home.append(&recent_scroll);
    let home_error = gtk::Label::new(None);
    home_error.set_wrap(true);
    home_error.add_css_class("error");
    home.append(&home_error);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(640);
    clamp.set_child(Some(&home));
    home_view.set_content(Some(&clamp));
    home_window.set_content(Some(&home_view));
    window.set_content(Some(&split));

    let ui = Ui {
        window: window.clone(),
        core: Rc::new(RefCell::new(core)),
        snapshot: current_snapshot,
        sidebars,
        sidebar_pages,
        search,
        file_search,
        sidebar_tab,
        file_query,
        file_search_row: search_row,
        text_search_row,
        search_controls,
        search_spinner,
        search_truncated,
        conflict_banner,
        document_tabs,
        document_tab_bar: tab_bar.clone(),
        document_tab_scroll,
        document_pages: Rc::new(RefCell::new(Vec::new())),
        editor_panel,
        editor_stack,
        empty_editor_page,
        editor,
        editor_chrome,
        buffer,
        heading,
        status,
        home_window: home_window.clone(),
        home_error,
        resume: resume.clone(),
        recent_list,
        recent_empty,
        updating: Rc::new(Cell::new(false)),
        pending_move_target,
        drop_trace: Rc::new(RefCell::new(None)),
        collapsed_directories,
    };
    ui.render(snapshot);
    for (button, delta) in [(&decrease_text_size, -1.0), (&increase_text_size, 1.0)] {
        let ui = ui.clone();
        let size = editor_font_size.clone();
        let base_size = editor_base_font_size.clone();
        let text_zoom = text_zoom.clone();
        let style = editor_font_style.clone();
        let decrease = decrease_text_size.clone();
        let increase = increase_text_size.clone();
        button.connect_clicked(move |_| {
            let base = base_size
                .get()
                .unwrap_or_else(|| editor_font_points(&ui.editor));
            base_size.set(Some(base));
            let next = (size.get().unwrap_or(base) + delta).clamp(8.0, 40.0);
            size.set(Some(next));
            text_zoom.set_label(&format!("{:.0}%", next / base * 100.0));
            style.load_from_data(&format!(
                "textview.workspace-editor-text {{ font-size: {next}pt; }}"
            ));
            decrease.set_sensitive(next > 8.0);
            increase.set_sensitive(next < 40.0);
            ui.editor_chrome.font_size_changed(&ui.editor);
            ui.editor.grab_focus();
        });
    }
    {
        let ui = ui.clone();
        let bar = ui.document_tab_bar.clone();
        let gesture_bar = bar.clone();
        let gesture = gtk::GestureClick::new();
        gesture.set_button(1);
        gesture.set_name(Some("document-tab-keep-open"));
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_pressed(move |gesture, presses, x, y| {
            if presses != 2 {
                return;
            }
            let mut target = bar.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(widget) = target {
                // A double click on the native close control must only close.
                if widget.is::<gtk::Button>() {
                    return;
                }
                if let Some(page) = tab_page_at_widget(&widget) {
                    let id = ui
                        .document_pages
                        .borrow()
                        .iter()
                        .find(|(_, item)| item == &page)
                        .map(|(id, _)| id.clone());
                    if let Some(id) = id {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        ui.dispatch(AppCommand::KeepDocumentOpen(id));
                    }
                    return;
                }
                target = widget.parent();
            }
        });
        gesture_bar.add_controller(gesture);
    }
    {
        let target = Rc::new(RefCell::new(None::<DocumentId>));
        let action = gio::SimpleAction::new("keep-open", None);
        let ui_action = ui.clone();
        let action_target = target.clone();
        action.connect_activate(move |_, _| {
            if let Some(id) = action_target.borrow().clone() {
                ui_action.dispatch(AppCommand::KeepDocumentOpen(id));
            }
        });
        window.add_action(&action);
        let menu = gio::Menu::new();
        menu.append(Some("Keep Open"), Some("win.keep-open"));
        ui.document_tabs.set_menu_model(Some(&menu));
        let ui_menu = ui.clone();
        ui.document_tabs.connect_setup_menu(move |_, page| {
            let id = page.and_then(|page| {
                ui_menu
                    .document_pages
                    .borrow()
                    .iter()
                    .find(|(_, item)| item == page)
                    .map(|(id, _)| id.clone())
            });
            action.set_enabled(id.is_some() && id == ui_menu.snapshot.borrow().preview_document);
            target.replace(id);
        });
    }
    if let Some(action) = app
        .lookup_action("devtools")
        .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
    {
        let core = ui.core.clone();
        action.connect_activate(move |_, _| {
            let mut app = core.borrow_mut();
            app.start_devtools("gtk");
            app.diagnostic_observer().open_console();
        });
    }
    {
        let ui = ui.clone();
        file_tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(SidebarTab::Files);
        });
    }
    {
        let ui = ui.clone();
        text_tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(SidebarTab::Search);
        });
    }
    for (index, tab) in empty_tabs.into_iter().enumerate() {
        let ui = ui.clone();
        tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(SidebarTab::ALL[index + 2]);
        });
    }
    {
        let ui = ui.clone();
        ui.file_search
            .clone()
            .connect_search_changed(move |search| {
                if ui.updating.get() {
                    return;
                }
                ui.queue_file_filter(search.text().as_str());
            });
    }
    {
        let target = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY,
        );
        target.set_name(Some("os-files-open"));
        target.set_propagation_phase(gtk::PropagationPhase::Capture);
        let ui = ui.clone();
        target.connect_drop(move |_, value, _, _| {
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let Some(paths) = sidebar_interactions::file_paths(&files) else {
                return false;
            };
            let ui = ui.clone();
            glib::idle_add_local_once(move || {
                let _ = ui.core.borrow_mut().dispatch(AppCommand::OpenFiles(paths));
                ui.render_current();
            });
            true
        });
        content_view.add_controller(target);
    }
    for sidebar in &ui.sidebars {
        sidebar
            .interactions
            .attach_external_drop(&sidebar.scroll, "", false);
    }
    for sidebar in &ui.sidebars {
        let rename_ui = ui.clone();
        let delete_ui = ui.clone();
        let move_ui = ui.clone();
        let trace_ui = ui.clone();
        let validate_ui = ui.clone();
        let external_validate_ui = ui.clone();
        let external_move_ui = ui.clone();
        let expand_ui = ui.clone();
        let collapse_ui = ui.clone();
        sidebar
            .interactions
            .callbacks
            .replace(Some(sidebar_interactions::Callbacks {
                begin_drop: Rc::new(move || {
                    let trace = native_workspace_application::SidebarDropTrace::begin(
                        trace_ui.core.borrow().diagnostic_observer(),
                        0,
                    );
                    trace_ui.drop_trace.replace(Some(trace.clone()));
                    trace
                }),
                delete: Rc::new(move |path| delete_ui.confirm_delete(path)),
                rename: Rc::new(move |path, name| {
                    let Ok(id) = DocumentId::parse(path) else {
                        return false;
                    };
                    let result = rename_ui
                        .core
                        .borrow_mut()
                        .dispatch(AppCommand::RenameEntry { id, new_name: name });
                    match result {
                        Ok(()) => {
                            rename_ui.render_current();
                            true
                        }
                        Err(error) => {
                            rename_ui.status.set_label(&error.to_string());
                            false
                        }
                    }
                }),
                move_entry: Rc::new(move |path, directory| {
                    let Ok(id) = DocumentId::parse(path.clone()) else {
                        return false;
                    };
                    let trace = move_ui.drop_trace.borrow().clone().unwrap_or_default();
                    let name = path.rsplit('/').next().unwrap_or(&path);
                    let target = if directory.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{directory}/{name}")
                    };
                    let result = move_ui
                        .core
                        .borrow_mut()
                        .dispatch_traced(AppCommand::MoveEntry { id, directory }, trace.clone());
                    match result {
                        Ok(()) => {
                            move_ui.render_current();
                            if trace.is_active() {
                                let sidebar = move_ui.sidebar().interactions.clone();
                                let deadline =
                                    std::time::Instant::now() + std::time::Duration::from_secs(5);
                                move_ui.window.add_tick_callback(move |_, _| {
                                    if sidebar
                                        .row_widget(&target)
                                        .is_some_and(|row| row.is_mapped())
                                    {
                                        trace.sidebar_ready();
                                        glib::ControlFlow::Break
                                    } else if std::time::Instant::now() >= deadline {
                                        glib::ControlFlow::Break
                                    } else {
                                        glib::ControlFlow::Continue
                                    }
                                });
                            }
                            true
                        }
                        Err(error) => {
                            move_ui.status.set_label(&error.to_string());
                            false
                        }
                    }
                }),
                can_move_files: Rc::new(move |paths, directory| {
                    external_validate_ui
                        .core
                        .borrow()
                        .can_move_files(paths, directory)
                }),
                move_files: Rc::new(move |paths, directory| {
                    let _ = external_move_ui
                        .core
                        .borrow_mut()
                        .dispatch(AppCommand::MoveFiles { paths, directory });
                    external_move_ui.render_current();
                }),
                can_move: Rc::new(move |source, directory| {
                    DocumentId::parse(source).is_ok_and(|id| {
                        validate_ui
                            .core
                            .borrow()
                            .can_move_entry(id, directory.to_owned())
                    })
                }),
                expand: Rc::new(move |path| {
                    if expand_ui.sidebar_tab.get() == SidebarTab::Files
                        && expand_ui.file_query.borrow().trim().is_empty()
                        && expand_ui.collapsed_directories.borrow().contains(path)
                    {
                        expand_ui.toggle_directory(path);
                        true
                    } else {
                        false
                    }
                }),
                collapse: Rc::new(move |path| {
                    if collapse_ui.collapsed_directories.borrow().contains(path) {
                        return;
                    }
                    if collapse_ui.sidebar_tab.get() == SidebarTab::Files
                        && collapse_ui.file_query.borrow().trim().is_empty()
                    {
                        collapse_ui.toggle_directory(path);
                    } else {
                        collapse_ui
                            .collapsed_directories
                            .borrow_mut()
                            .insert(path.to_owned());
                        collapse_ui.sidebars[0].dirty.set(true);
                    }
                }),
            }));
        let interactions = sidebar.interactions.clone();
        let rows = sidebar.sidebar_rows.clone();
        let selection = sidebar.file_selection.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::F2 {
                let path = rows
                    .borrow()
                    .get(selection.selected() as usize)
                    .map(|row| match row {
                        SidebarRow::Directory(path) => path.clone(),
                        SidebarRow::File(id) => id.as_str().to_owned(),
                        SidebarRow::Match(result) => result.id.as_str().to_owned(),
                    });
                if let Some(path) = path.or_else(|| interactions.selected.borrow().clone()) {
                    interactions.begin_rename(&path);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        sidebar.files.add_controller(keys);
    }
    for (button, directory) in [(new_file, false), (new_folder, true)] {
        let ui = ui.clone();
        let add_popover = add_popover.clone();
        button.connect_clicked(move |_| {
            add_popover.popdown();
            let dialog = gtk::Dialog::builder()
                .title(if directory { "New Folder" } else { "New File" })
                .transient_for(&ui.window)
                .modal(true)
                .build();
            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
            dialog.add_button("Create", gtk::ResponseType::Accept);
            dialog.set_default_response(gtk::ResponseType::Accept);
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some("Path relative to workspace"));
            entry.set_activates_default(true);
            entry.set_margin_start(12);
            entry.set_margin_end(12);
            entry.set_margin_top(12);
            entry.set_margin_bottom(12);
            dialog.content_area().append(&entry);
            let ui = ui.clone();
            dialog.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    ui.create_entry(entry.text().trim(), directory);
                }
                dialog.close();
            });
            dialog.present();
        });
    }
    {
        let ui = ui.clone();
        let action = gio::SimpleAction::new("home", None);
        action.connect_activate(move |_, _| {
            ui.home_window.present();
            ui.window.set_visible(false);
        });
        app.add_action(&action);
    }
    {
        let ui = ui.clone();
        resume.connect_clicked(move |_| {
            ui.window.present();
            ui.home_window.set_visible(false);
        });
    }
    {
        let home_window = home_window.clone();
        window.connect_close_request(move |window| {
            window.set_visible(false);
            home_window.present();
            glib::Propagation::Stop
        });
    }
    {
        let app = app.clone();
        home_window.connect_close_request(move |_| {
            app.quit();
            glib::Propagation::Proceed
        });
    }
    {
        let application = app.clone();
        let action = gio::SimpleAction::new("quit", None);
        action.connect_activate(move |_, _| application.quit());
        app.add_action(&action);
        app.set_accels_for_action("app.quit", &[&primary_accelerator("q")]);
    }
    {
        let ui = ui.clone();
        let save_action = gio::SimpleAction::new("save", None);
        save_action.connect_activate(move |_, _| {
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
        app.add_action(&save_action);
        app.set_accels_for_action("app.save", &[&primary_accelerator("s")]);
    }
    {
        let ui = ui.clone();
        let action = gio::SimpleAction::new("save-all", None);
        action.connect_activate(move |_, _| ui.dispatch(AppCommand::SaveAllDocuments));
        app.add_action(&action);
        app.set_accels_for_action("app.save-all", &[&primary_accelerator("<Shift>s")]);
    }
    {
        let ui = ui.clone();
        let view = ui.document_tabs.clone();
        view.connect_selected_page_notify(move |view| {
            if ui.updating.get() {
                return;
            }
            let id = view.selected_page().and_then(|page| {
                ui.document_pages
                    .borrow()
                    .iter()
                    .find(|(_, item)| item == &page)
                    .map(|(id, _)| id.clone())
            });
            if let Some(id) = id {
                ui.dispatch(AppCommand::PreviewDocument(id));
            }
        });
    }
    {
        let ui = ui.clone();
        let view = ui.document_tabs.clone();
        view.connect_close_page(move |view, page| {
            if ui.updating.get() {
                return glib::Propagation::Proceed;
            }
            let id = ui
                .document_pages
                .borrow()
                .iter()
                .find(|(_, item)| item == page)
                .map(|(id, _)| id.clone());
            // The core removes the native page only after saving or confirming discard.
            view.close_page_finish(page, false);
            if let Some(id) = id {
                ui.request_close_document(id);
            }
            glib::Propagation::Stop
        });
    }
    {
        let ui = ui.clone();
        let view = ui.document_tabs.clone();
        view.connect_page_reordered(move |_, page, index| {
            if ui.updating.get() {
                return;
            }
            let id = ui
                .document_pages
                .borrow()
                .iter()
                .find(|(_, item)| item == page)
                .map(|(id, _)| id.clone());
            if let Some(id) = id {
                ui.dispatch(AppCommand::MoveDocumentTab {
                    id,
                    index: index as usize,
                });
            }
        });
    }
    if let Some(error) = persistence_error {
        ui.status.set_label(&error);
    }
    {
        let ui = ui.clone();
        let application = app.clone();
        let action = gio::SimpleAction::new("open", None);
        action.connect_activate(move |_, _| {
            let window = application.active_window();
            let dialog = gtk::FileChooserNative::new(
                Some("Open Workspace"),
                window.as_ref(),
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
        app.add_action(&action);
        app.set_accels_for_action("app.open", &[&primary_accelerator("o")]);
    }
    {
        let ui = ui.clone();
        ui.search.clone().connect_search_changed(move |search| {
            if !ui.updating.get() {
                ui.dispatch(AppCommand::QueueSearchQuery(search.text().to_string()));
            }
        });
    }
    ui.search_controls.connect(&ui);
    for (name, key, replace) in [
        ("find-in-files", "<Shift>f", false),
        ("replace-in-files", "<Shift>h", true),
    ] {
        let action = gio::SimpleAction::new(name, None);
        ui.window.add_action(&action);
        let ui = ui.clone();
        let text_tab = text_tab.clone();
        action.connect_activate(move |_, _| {
            text_tab.set_active(true);
            if replace {
                ui.search_controls.replace_expander.set_expanded(true);
                ui.search_controls.replacement.grab_focus();
            } else {
                ui.search.grab_focus();
            }
        });
        app.set_accels_for_action(&format!("win.{name}"), &[&primary_accelerator(key)]);
    }
    for (mode, sidebar) in ui.sidebars.iter().enumerate() {
        {
            let ui = ui.clone();
            sidebar
                .file_selection
                .clone()
                .connect_selected_notify(move |selection| {
                    if ui.updating.get() || ui.sidebar_tab.get() != SidebarTab::ALL[mode] {
                        return;
                    }
                    let row = ui
                        .sidebar()
                        .sidebar_rows
                        .borrow()
                        .get(selection.selected() as usize)
                        .cloned();
                    ui.sync_sidebar_selection();
                    if let Some(row) = row {
                        if let SidebarRow::Directory(path) = row {
                            ui.toggle_directory(&path);
                            return;
                        }
                        let Some(id) = row.id().cloned() else { return };
                        let location = row.location().cloned();
                        ui.dispatch(AppCommand::PreviewDocument(id.clone()));
                        if ui
                            .snapshot
                            .borrow()
                            .active_document
                            .as_ref()
                            .map(|doc| &doc.id)
                            == Some(&id)
                            && let Some(location) = &location
                        {
                            ui.select_match(location);
                        }
                    }
                });
        }
        {
            let ui = ui.clone();
            sidebar.files.clone().connect_activate(move |_, index| {
                if ui.sidebar_tab.get() != SidebarTab::ALL[mode] {
                    return;
                }
                let selected = ui
                    .sidebar()
                    .sidebar_rows
                    .borrow()
                    .get(index as usize)
                    .cloned();
                if let Some(row) = selected
                    && let Some(id) = row.id().cloned()
                {
                    ui.dispatch(AppCommand::OpenDocument(id.clone()));
                    if ui
                        .snapshot
                        .borrow()
                        .active_document
                        .as_ref()
                        .map(|doc| &doc.id)
                        == Some(&id)
                        && let Some(location) = row.location()
                    {
                        ui.select_match(location);
                    }
                }
            });
        }
    }
    {
        let ui = ui.clone();
        ui.buffer.clone().connect_changed(move |buffer| {
            if ui.updating.get() {
                return;
            }
            let file_name = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .map(|doc| doc.id.as_str().to_owned());
            if let Some(file_name) = file_name.as_deref() {
                syntax::apply(buffer, file_name);
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
        reload.connect_clicked(move |_| {
            let id = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .map(|doc| doc.id.clone());
            if let Some(id) = id {
                ui.dispatch(AppCommand::ReloadDocument(id));
            }
        });
    }
    {
        let ui = ui.clone();
        overwrite.connect_clicked(move |_| {
            let id = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .map(|doc| doc.id.clone());
            if let Some(id) = id {
                ui.dispatch(AppCommand::OverwriteDocument(id));
            }
        });
    }
    {
        let ui = ui.clone();
        let mut last = std::time::Instant::now();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            if ui.core.borrow().wants_fast_poll()
                || last.elapsed() >= std::time::Duration::from_millis(250)
            {
                ui.poll_core();
                last = std::time::Instant::now();
            }
            glib::ControlFlow::Continue
        });
    }
    home_window.present();
    #[cfg(feature = "devtools")]
    {
        let core = Rc::downgrade(&ui.core);
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let Some(core) = core.upgrade() else {
                return glib::ControlFlow::Break;
            };
            core.borrow().diagnostic_observer().ui_heartbeat();
            glib::ControlFlow::Continue
        });
    }
    #[cfg(debug_assertions)]
    smoke::run(smoke::Widgets {
        ui: &ui,
        app,
        sidebar_header: &sidebar_header,
        content_header: &content_header,
        toggle_sidebar: &toggle_sidebar,
        icons: &icons,
        sidebar_icon,
        files_scroll: &files_scroll,
        file_tab: &file_tab,
        text_tab: &text_tab,
        editor_scroll: &editor_scroll,
        overwrite: &overwrite,
        save: &save,
        decrease_text_size: &decrease_text_size,
        increase_text_size: &increase_text_size,
        split: &split,
        sidebar_frame: &sidebar_frame,
        sidebar_resize: &sidebar_resize,
    });
    ui
}
