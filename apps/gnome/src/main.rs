mod material_icons;

use adw::prelude::*;
use gtk::{gio, glib};
use native_workspace_application::{
    AppCommand, AppSnapshot, Application, SearchLocation, SearchResult, file_icon_name,
    folder_icon_name,
};
use native_workspace_domain::DocumentId;
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
    files: gtk::ListBox,
    sidebar_rows: Rc<RefCell<Vec<SidebarRow>>>,
    search: gtk::SearchEntry,
    search_spinner: gtk::Spinner,
    files_placeholder: gtk::Label,
    conflict_banner: gtk::Box,
    editor: gtk::TextView,
    buffer: gtk::TextBuffer,
    heading: gtk::Label,
    status: gtk::Label,
    counter: gtk::Label,
    save: gtk::Button,
    recent_button: gtk::MenuButton,
    recent_list: gtk::Box,
    updating: Rc<Cell<bool>>,
    collapsed_directories: Rc<RefCell<std::collections::HashSet<String>>>,
    force_sidebar: Rc<Cell<bool>>,
}

#[derive(Clone)]
enum SidebarRow {
    Directory(String),
    File(DocumentId),
    Match(SearchResult),
}

impl SidebarRow {
    fn id(&self) -> Option<&DocumentId> {
        match self {
            Self::Directory(_) => None,
            Self::File(id) => Some(id),
            Self::Match(result) => Some(&result.id),
        }
    }

    fn location(&self) -> Option<&SearchLocation> {
        match self {
            Self::Directory(_) | Self::File(_) => None,
            Self::Match(result) => result.location.as_ref(),
        }
    }
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
        let previous_selection = self
            .sidebar_rows
            .borrow()
            .get(
                self.files
                    .selected_row()
                    .map_or(usize::MAX, |row| row.index() as usize),
            )
            .cloned();
        let results_changed = {
            let previous = self.snapshot.borrow();
            previous.search_results != snapshot.search_results
                || previous.search_query != snapshot.search_query
                || previous.documents != snapshot.documents
                || previous.directories != snapshot.directories
                || self.force_sidebar.replace(false)
        };
        {
            let previous = self.snapshot.borrow();
            if previous.workspace_root != snapshot.workspace_root {
                self.collapsed_directories
                    .replace(snapshot.directories.iter().cloned().collect());
            } else if previous.directories != snapshot.directories {
                let known: std::collections::HashSet<_> = previous.directories.iter().collect();
                self.collapsed_directories.borrow_mut().extend(
                    snapshot
                        .directories
                        .iter()
                        .filter(|path| !known.contains(path))
                        .cloned(),
                );
            }
        }
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
        self.search_spinner
            .set_spinning(snapshot.search_in_progress);
        self.search_spinner.set_visible(snapshot.search_in_progress);
        self.files_placeholder
            .set_label(if snapshot.workspace_name.is_none() {
                "Open a workspace"
            } else {
                "No matching files"
            });
        self.conflict_banner.set_visible(
            snapshot
                .active_document
                .as_ref()
                .is_some_and(|document| document.external_change),
        );
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
            while let Some(child) = self.files.first_child() {
                self.files.remove(&child);
            }
            let mut rows = Vec::new();
            let conflicted: std::collections::HashSet<_> = snapshot
                .documents
                .iter()
                .filter(|entry| entry.external_change)
                .map(|entry| &entry.id)
                .collect();
            let groups: std::collections::HashMap<_, _> = snapshot
                .search_results
                .chunk_by(|left, right| left.id == right.id)
                .map(|group| (group[0].id.as_str().to_owned(), group))
                .collect();
            let mut paths: Vec<(String, bool)> = Vec::new();
            if snapshot.search_query.0.is_empty() {
                paths.extend(
                    snapshot
                        .directories
                        .iter()
                        .cloned()
                        .map(|path| (path, true)),
                );
            } else {
                for id in groups.keys() {
                    let mut parent = std::path::Path::new(id).parent();
                    while let Some(path) = parent.and_then(|path| path.to_str()) {
                        if path.is_empty() {
                            break;
                        }
                        paths.push((path.to_owned(), true));
                        parent = std::path::Path::new(path).parent();
                    }
                }
            }
            paths.extend(groups.keys().cloned().map(|path| (path, false)));
            let directories: std::collections::HashSet<_> = paths
                .iter()
                .filter(|(_, is_directory)| *is_directory)
                .map(|(path, _)| path.clone())
                .collect();
            paths.sort_by(|left, right| compare_sidebar_paths(left, right, &directories));
            paths.dedup();
            for (path, is_directory) in paths {
                let depth = path.matches('/').count();
                let hidden = snapshot.search_query.0.is_empty()
                    && path
                        .split('/')
                        .scan(String::new(), |prefix, part| {
                            let parent = prefix.clone();
                            if !prefix.is_empty() {
                                prefix.push('/');
                            }
                            prefix.push_str(part);
                            Some(parent)
                        })
                        .any(|parent| {
                            !parent.is_empty()
                                && self.collapsed_directories.borrow().contains(&parent)
                        });
                if hidden {
                    continue;
                }
                let name = path.rsplit('/').next().unwrap_or(&path);
                if is_directory {
                    let collapsed = snapshot.search_query.0.is_empty()
                        && self.collapsed_directories.borrow().contains(&path);
                    let marker = if collapsed { "▸" } else { "▾" };
                    self.files.append(&sidebar_row(
                        name,
                        depth,
                        Some(marker),
                        Some(&folder_icon_for(&path, !collapsed)),
                        None,
                    ));
                    rows.push(SidebarRow::Directory(path));
                    continue;
                }
                let Some(group) = groups.get(&path) else {
                    continue;
                };
                let document = &group[0];
                let suffix = if conflicted.contains(&document.id) {
                    Some("⚠")
                } else if document.dirty {
                    Some("●")
                } else {
                    None
                };
                self.files.append(&sidebar_row(
                    name,
                    depth,
                    None,
                    Some(&icon_for(document.id.as_str())),
                    suffix,
                ));
                rows.push(SidebarRow::File(document.id.clone()));
                for result in group.iter().filter(|result| result.location.is_some()) {
                    let location = result.location.as_ref().unwrap();
                    let title = match result.snippet.as_deref() {
                        Some(snippet) => {
                            format!("↳ {}:{}  {}", location.line, location.column, snippet)
                        }
                        None => format!("↳ {}:{}", location.line, location.column),
                    };
                    self.files
                        .append(&sidebar_row(&title, depth + 1, None, None, None));
                    rows.push(SidebarRow::Match(result.clone()));
                }
            }
            self.sidebar_rows.replace(rows);
        }
        if let Some(active) = snapshot.active_document.as_ref() {
            let rows = self.sidebar_rows.borrow();
            let selected_index = previous_selection.as_ref().and_then(|previous| {
                rows.iter().position(|row| {
                    row.id() == Some(&active.id)
                        && row.id() == previous.id()
                        && row.location() == previous.location()
                })
            });
            let index = selected_index.or_else(|| {
                if snapshot.search_query.0.trim().is_empty() {
                    rows.iter()
                        .position(|row| matches!(row, SidebarRow::File(id) if id == &active.id))
                } else {
                    None
                }
            });
            if let Some(index) = index {
                self.files
                    .select_row(self.files.row_at_index(index as i32).as_ref());
            } else {
                self.files.unselect_all();
            }
        } else {
            self.files.unselect_all();
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

    fn select_match(&self, location: &SearchLocation) {
        let start = self
            .buffer
            .iter_at_offset(location.start_char.min(i32::MAX as u64) as i32);
        let mut end = self
            .buffer
            .iter_at_offset(location.end_char.min(i32::MAX as u64) as i32);
        self.buffer.select_range(&start, &end);
        self.editor.grab_focus();
        self.editor.scroll_to_iter(&mut end, 0.2, false, 0.0, 0.0);
    }
}

fn compare_sidebar_paths(
    left: &(String, bool),
    right: &(String, bool),
    directories: &std::collections::HashSet<String>,
) -> std::cmp::Ordering {
    let mut left_prefix = String::new();
    let mut right_prefix = String::new();
    let mut left_parts = left.0.split('/');
    let mut right_parts = right.0.split('/');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(a), Some(b)) => {
                if !left_prefix.is_empty() {
                    left_prefix.push('/');
                    right_prefix.push('/');
                }
                left_prefix.push_str(a);
                right_prefix.push_str(b);
                if a != b {
                    return (!directories.contains(&left_prefix))
                        .cmp(&(!directories.contains(&right_prefix)))
                        .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
                        .then_with(|| a.cmp(b));
                }
            }
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, None) => return std::cmp::Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod sidebar_order_tests {
    use super::compare_sidebar_paths;
    use std::collections::HashSet;

    #[test]
    fn directories_precede_files_at_each_level() {
        let mut entries = vec![
            ("a.txt".to_owned(), false),
            ("z/child.txt".to_owned(), false),
            ("z/x".to_owned(), true),
            ("z".to_owned(), true),
            ("z/x/nested.txt".to_owned(), false),
            ("z/b.txt".to_owned(), false),
        ];
        let directories: HashSet<_> = entries
            .iter()
            .filter(|(_, is_directory)| *is_directory)
            .map(|(path, _)| path.clone())
            .collect();
        entries.sort_by(|left, right| compare_sidebar_paths(left, right, &directories));
        let paths: Vec<_> = entries.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "z",
                "z/x",
                "z/x/nested.txt",
                "z/b.txt",
                "z/child.txt",
                "a.txt"
            ]
        );
    }
}

fn sidebar_row(
    title: &str,
    depth: usize,
    disclosure: Option<&str>,
    icon: Option<&gtk::gdk::Texture>,
    suffix: Option<&str>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_margin_start(8 + (depth as i32 * 16));
    content.set_margin_end(8);
    content.set_margin_top(3);
    content.set_margin_bottom(3);
    if let Some(disclosure) = disclosure {
        let marker = gtk::Label::new(Some(disclosure));
        marker.set_size_request(12, -1);
        content.append(&marker);
    }
    if let Some(icon) = icon {
        let image = gtk::Image::from_paintable(Some(icon));
        image.set_pixel_size(18);
        content.append(&image);
    }
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&label);
    if let Some(suffix) = suffix {
        content.append(&gtk::Label::new(Some(suffix)));
    }
    row.set_child(Some(&content));
    row
}

fn icon_for(name: &str) -> gtk::gdk::Texture {
    texture_for_icon(file_icon_name(name))
}

fn folder_icon_for(name: &str, expanded: bool) -> gtk::gdk::Texture {
    texture_for_icon(folder_icon_name(name, expanded))
}

fn texture_for_icon(name: &str) -> gtk::gdk::Texture {
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<String, gtk::gdk::Texture>> = RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with(|cache| {
        if let Some(texture) = cache.borrow().get(name) {
            return texture.clone();
        }
        let bytes = material_icons::png_bytes(name)
            .or_else(|| material_icons::png_bytes("file"))
            .expect("bundled default file icon");
        let texture = gtk::gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes))
            .expect("bundled PNG icon");
        cache.borrow_mut().insert(name.to_owned(), texture.clone());
        texture
    })
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
    let files = gtk::ListBox::new();
    files.add_css_class("compact-files");
    files.set_size_request(220, -1);
    let files_placeholder = gtk::Label::new(Some("Open a workspace"));
    files.set_placeholder(Some(&files_placeholder));
    let search = gtk::SearchEntry::new();
    let search_spinner = gtk::Spinner::new();
    search_spinner.set_visible(false);
    search.set_placeholder_text(Some("Search files"));
    search.set_margin_top(8);
    search.set_margin_bottom(8);
    search.set_margin_start(8);
    search.set_margin_end(8);
    let sidebar_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    search.set_hexpand(true);
    search_row.append(&search);
    search_row.append(&search_spinner);
    sidebar_content.append(&search_row);
    let files_scroll = gtk::ScrolledWindow::new();
    files_scroll.set_vexpand(true);
    files_scroll.set_child(Some(&files));
    sidebar_content.append(&files_scroll);
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
    let reload = gtk::Button::with_label("Reload (discard edits)");
    let overwrite = gtk::Button::with_label("Overwrite disk");
    conflict_banner.append(&reload);
    conflict_banner.append(&overwrite);
    conflict_banner.set_visible(false);
    let content_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content_body.append(&conflict_banner);
    editor_scroll.set_vexpand(true);
    content_body.append(&editor_scroll);
    content_view.set_content(Some(&content_body));
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
        sidebar_rows: Rc::new(RefCell::new(Vec::new())),
        search,
        search_spinner,
        files_placeholder,
        conflict_banner,
        editor,
        buffer,
        heading,
        status,
        counter,
        save,
        recent_button,
        recent_list,
        updating: Rc::new(Cell::new(false)),
        collapsed_directories: Rc::new(RefCell::new(std::collections::HashSet::new())),
        force_sidebar: Rc::new(Cell::new(false)),
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
                ui.dispatch(AppCommand::QueueSearchQuery(search.text().to_string()));
            }
        });
    }
    {
        let ui = ui.clone();
        ui.files
            .clone()
            .connect_row_selected(move |_, selected_row| {
                if ui.updating.get() {
                    return;
                }
                let row = ui
                    .sidebar_rows
                    .borrow()
                    .get(selected_row.map_or(usize::MAX, |row| row.index() as usize))
                    .cloned();
                if let Some(row) = row {
                    if let SidebarRow::Directory(path) = row {
                        let mut collapsed = ui.collapsed_directories.borrow_mut();
                        if !collapsed.insert(path.clone()) {
                            collapsed.remove(&path);
                        }
                        drop(collapsed);
                        ui.force_sidebar.set(true);
                        let snapshot = ui.snapshot.borrow().clone();
                        ui.render(snapshot);
                        return;
                    }
                    let Some(id) = row.id().cloned() else { return };
                    let location = row.location().cloned();
                    ui.dispatch(AppCommand::OpenDocument(id.clone()));
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
        ui.files.clone().connect_row_activated(move |_, row| {
            let selected = ui.sidebar_rows.borrow().get(row.index() as usize).cloned();
            if let Some(row) = selected
                && ui
                    .snapshot
                    .borrow()
                    .active_document
                    .as_ref()
                    .map(|doc| &doc.id)
                    == row.id()
                && let Some(location) = row.location()
            {
                ui.select_match(location);
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
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let has_workspace = ui.snapshot.borrow().workspace_root.is_some();
            if has_workspace {
                let result = ui
                    .core
                    .borrow_mut()
                    .dispatch_if_changed(AppCommand::PollWorkspaceChanges);
                match result {
                    Ok(Some(snapshot)) => ui.render(snapshot),
                    Ok(None) => {}
                    Err(error) => ui.status.set_label(&error.to_string()),
                }
                let result = ui
                    .core
                    .borrow_mut()
                    .dispatch_if_changed(AppCommand::PollSearchResults);
                match result {
                    Ok(Some(snapshot)) => ui.render(snapshot),
                    Ok(None) => {}
                    Err(error) => ui.status.set_label(&error.to_string()),
                }
            }
            glib::ControlFlow::Continue
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
        let workspace_path = PathBuf::from(path);
        ui.dispatch(AppCommand::OpenWorkspace(workspace_path.clone()));
        if ui
            .snapshot
            .borrow()
            .directories
            .iter()
            .any(|path| path == "src")
        {
            assert!(ui.collapsed_directories.borrow().contains("src"));
            assert_eq!(ui.sidebar_rows.borrow().len(), 2);
            assert!(matches!(
                ui.sidebar_rows.borrow()[0],
                SidebarRow::Directory(_)
            ));
            ui.files.select_row(ui.files.row_at_index(0).as_ref());
            let nested_index = ui
                .sidebar_rows
                .borrow()
                .iter()
                .position(|row| matches!(row, SidebarRow::File(id) if id.as_str() == "src/main.rs"))
                .expect("expanded directory file");
            let nested_row = ui.files.row_at_index(nested_index as i32).unwrap();
            assert_eq!(nested_row.child().unwrap().margin_start(), 24);
        }
        assert_eq!(
            window.title().as_deref(),
            ui.snapshot.borrow().workspace_name.as_deref()
        );
        let first =
            ui.sidebar_rows
                .borrow()
                .iter()
                .enumerate()
                .find_map(|(index, row)| match row {
                    SidebarRow::File(id) if id.as_str() == "README.md" => Some((index, id.clone())),
                    _ => None,
                });
        if let Some((index, id)) = first {
            ui.files
                .select_row(ui.files.row_at_index(index as i32).as_ref());
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
            ui.buffer.set_text(&format!(
                "GTK Phase 1 smoke content\n{}💡 Another Phase 1 match",
                "filler\n".repeat(80)
            ));
            ui.save.emit_clicked();
            assert_eq!(ui.status.text(), "Saved");
            ui.dispatch(AppCommand::SetSearchQuery("phase 1".into()));
            assert_eq!(ui.snapshot.borrow().search_results.len(), 2);
            assert_eq!(ui.sidebar_rows.borrow().len(), 3);
            fn first_sidebar_row(widget: &gtk::Widget) -> Option<gtk::Widget> {
                if widget.css_name() == "row" {
                    return Some(widget.clone());
                }
                let mut child = widget.first_child();
                while let Some(current) = child {
                    if let Some(row) = first_sidebar_row(&current) {
                        return Some(row);
                    }
                    child = current.next_sibling();
                }
                None
            }
            let first_row = first_sidebar_row(ui.files.upcast_ref()).expect("sidebar file row");
            let (_, natural_height, _, _) = first_row.measure(gtk::Orientation::Vertical, -1);
            assert!(
                natural_height <= 32,
                "sidebar row is {natural_height}px high"
            );
            assert!(matches!(ui.sidebar_rows.borrow()[0], SidebarRow::File(_)));
            assert_eq!(
                ui.snapshot.borrow().search_results[0].snippet.as_deref(),
                Some("GTK Phase 1 smoke content")
            );
            ui.files.select_row(ui.files.row_at_index(2).as_ref());
            let (start, end) = ui
                .buffer
                .selection_bounds()
                .expect("selected search occurrence");
            assert_eq!(ui.buffer.text(&start, &end, false), "Phase 1");
            let vertical = editor_scroll.vadjustment();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                glib::MainContext::default().iteration(false);
                if vertical.value() > 0.0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(
                vertical.value() > 0.0,
                "vertical scroll: upper={}, page={}",
                vertical.upper(),
                vertical.page_size()
            );
            ui.dispatch(AppCommand::SetSearchQuery("unlikely-no-match".into()));
            assert!(ui.snapshot.borrow().search_results.is_empty());
            assert_eq!(ui.search.text(), "unlikely-no-match");
            ui.dispatch(AppCommand::SetSearchQuery(String::new()));
            assert_eq!(
                ui.snapshot.borrow().search_results.len(),
                ui.snapshot.borrow().documents.len()
            );
            let document_path = workspace_path.join(id.as_str());
            std::fs::write(&document_path, "external clean edit").unwrap();
            ui.dispatch(AppCommand::RefreshWorkspace);
            assert_eq!(
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
                "external clean edit"
            );
            assert!(!ui.conflict_banner.is_visible());
            ui.buffer.set_text("local edit");
            std::fs::write(&document_path, "external conflict").unwrap();
            ui.dispatch(AppCommand::RefreshWorkspace);
            assert!(ui.conflict_banner.is_visible());
            ui.save.emit_clicked();
            assert_eq!(
                std::fs::read_to_string(&document_path).unwrap(),
                "external conflict"
            );
            overwrite.emit_clicked();
            assert_eq!(
                std::fs::read_to_string(&document_path).unwrap(),
                "local edit"
            );
            assert!(!ui.conflict_banner.is_visible());
            println!("GTK workspace edit/save smoke test passed");
        }
        app.quit();
    }
}
