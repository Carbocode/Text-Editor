mod document_tab_scroll;
mod editor_chrome;
mod material_icons;
mod sidebar_interactions;
mod syntax;

use adw::prelude::*;
use gtk::{gio, glib};
use native_workspace_application::{
    AppCommand, AppSnapshot, NativeApplication as Application, PathFilter, PathFilterResult,
    SearchLocation, SearchResult, file_icon_name, folder_icon_name,
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
    // Keep Files and Search models, selections and scroll positions alive across tab switches.
    sidebars: [SidebarView; 2],
    sidebar_pages: gtk::Stack,
    search: gtk::SearchEntry,
    file_search: gtk::SearchEntry,
    search_mode: Rc<Cell<bool>>,
    sidebar_tab: Rc<Cell<u8>>,
    /// The name filter the Files tree shows, i.e. of `file_filter`. Typed
    /// text reaches it only after `path_filter` has listed the matches.
    file_query: Rc<RefCell<String>>,
    file_filter: Rc<RefCell<PathFilterResult>>,
    path_filter: Rc<RefCell<PathFilter>>,
    watching_file_filter: Rc<Cell<bool>>,
    file_search_row: gtk::Box,
    text_search_row: gtk::Box,
    search_spinner: gtk::Spinner,
    search_truncated: gtk::Label,
    watching_search: Rc<Cell<bool>>,
    conflict_banner: gtk::Box,
    document_tabs: adw::TabView,
    document_tab_scroll: Rc<document_tab_scroll::TouchpadScroll>,
    document_pages: Rc<RefCell<Vec<(DocumentId, adw::TabPage)>>>,
    editor_panel: gtk::Box,
    editor_stack: gtk::Stack,
    empty_editor_page: gtk::Box,
    editor: gtk::TextView,
    editor_chrome: editor_chrome::EditorChrome,
    buffer: gtk::TextBuffer,
    heading: gtk::Label,
    status: gtk::Label,
    counter: gtk::Label,
    home_window: adw::ApplicationWindow,
    home_error: gtk::Label,
    resume: gtk::Button,
    recent_list: gtk::Box,
    recent_empty: gtk::Label,
    updating: Rc<Cell<bool>>,
    background_move_error: Rc<RefCell<Option<String>>>,
    pending_move_target: Rc<RefCell<Option<String>>>,
    drop_trace: Rc<RefCell<Option<native_workspace_application::SidebarDropTrace>>>,
    collapsed_directories: Rc<RefCell<std::collections::HashSet<String>>>,
}

#[derive(Clone)]
struct SidebarView {
    interactions: Rc<sidebar_interactions::Interactions>,
    files: gtk::ListView,
    file_model: gtk::StringList,
    file_selection: gtk::SingleSelection,
    sidebar_rows: Rc<RefCell<Vec<SidebarRow>>>,
    all_entries: Rc<RefCell<Vec<SidebarEntry>>>,
    visible_entries: Rc<RefCell<Vec<SidebarEntry>>>,
    files_placeholder: gtk::Label,
    overlay: gtk::Overlay,
    scroll: gtk::ScrolledWindow,
    dirty: Rc<Cell<bool>>,
    pending_rows: Rc<RefCell<Vec<PendingRow>>>,
    open_documents: Rc<RefCell<std::collections::HashSet<String>>>,
}

struct PendingRow {
    path: String,
    row: glib::WeakRef<gtk::Box>,
    spinner: glib::WeakRef<gtk::Spinner>,
    open_indicator: glib::WeakRef<gtk::Image>,
    is_file: bool,
}

fn update_pending_row(row: &gtk::Box, spinner: &gtk::Spinner, path: &str, target: Option<&str>) {
    let pending =
        target.is_some_and(|target| path == target || path.starts_with(&format!("{target}/")));
    row.set_opacity(if pending { 0.55 } else { 1.0 });
    if pending {
        row.add_css_class("pending-move");
    } else {
        row.remove_css_class("pending-move");
    }
    spinner.set_visible(pending);
    spinner.set_spinning(pending);
    row.set_tooltip_text(pending.then_some("Moving… Waiting for confirmation"));
}

#[derive(Clone)]
enum SidebarRow {
    Directory(String),
    File(DocumentId),
    Match(SearchResult),
}

#[derive(Clone)]
struct SidebarEntry {
    row: SidebarRow,
    path: String,
    title: String,
    depth: usize,
    suffix: Option<&'static str>,
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
    fn create_entry(&self, name: &str, directory: bool) {
        let id = match DocumentId::parse(name.to_owned()) {
            Ok(id) => id,
            Err(_) => {
                self.status.set_label("Invalid relative path");
                return;
            }
        };
        let result = self.core.borrow_mut().dispatch(AppCommand::CreateEntry {
            id: id.clone(),
            directory,
        });
        match result {
            Ok(snapshot) => {
                self.render(snapshot);
                if !directory {
                    self.dispatch(AppCommand::OpenDocument(id));
                }
            }
            Err(error) => {
                self.status.set_label(&error.to_string());
                self.home_error.set_label(&error.to_string());
            }
        }
    }

    fn dispatch(&self, command: AppCommand) {
        self.background_move_error.replace(None);
        let opening_workspace = matches!(command, AppCommand::OpenWorkspace(_));
        if opening_workspace {
            self.home_error.set_label("");
        }
        let result = self.core.borrow_mut().dispatch(command);
        match result {
            Ok(snapshot) => {
                self.render(snapshot);
                if opening_workspace {
                    self.window.present();
                    self.home_window.set_visible(false);
                }
            }
            Err(error) => {
                let snapshot = self.core.borrow().snapshot();
                self.render(snapshot);
                self.status.set_label(&error.to_string());
                self.home_error.set_label(&error.to_string());
            }
        }
        let background_error = self.core.borrow_mut().take_background_error();
        if let Some(error) = background_error {
            self.background_move_error.replace(Some(error.clone()));
            let snapshot = self.core.borrow().snapshot();
            self.render(snapshot);
            self.status.set_label(&error);
        }
    }

    /// Matching every path on each keystroke froze the window on large
    /// workspaces; a worker lists the matches and `poll_file_filter` shows them.
    fn queue_file_filter(&self, query: &str) {
        let (documents, directories) = {
            let snapshot = self.snapshot.borrow();
            (snapshot.documents.clone(), snapshot.directories.clone())
        };
        let ready = self
            .path_filter
            .borrow_mut()
            .queue(query, documents, directories);
        match ready {
            Some(result) => self.apply_file_filter(result),
            None => self.watch_file_filter(),
        }
    }

    /// Matches stream in while a search runs; show them without waiting for
    /// the slow workspace poll.
    fn watch_search(&self) {
        if !self.snapshot.borrow().search_in_progress || self.watching_search.replace(true) {
            return;
        }
        let ui = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let result = ui
                .core
                .borrow_mut()
                .dispatch_if_changed(AppCommand::PollSearchResults);
            match result {
                Ok(Some(snapshot)) => ui.render(snapshot),
                Ok(None) => {}
                Err(error) => ui.status.set_label(&error.to_string()),
            }
            if ui.snapshot.borrow().search_in_progress {
                glib::ControlFlow::Continue
            } else {
                ui.watching_search.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    fn watch_file_filter(&self) {
        if self.watching_file_filter.replace(true) {
            return;
        }
        let ui = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            ui.poll_file_filter();
            if ui.path_filter.borrow().in_progress() {
                glib::ControlFlow::Continue
            } else {
                ui.watching_file_filter.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    fn poll_file_filter(&self) {
        let result = self.path_filter.borrow_mut().poll();
        if let Some(result) = result {
            self.apply_file_filter(result);
        }
    }

    fn apply_file_filter(&self, result: PathFilterResult) {
        self.file_query.replace(result.query.trim().to_owned());
        self.file_filter.replace(result);
        self.sidebars[0].dirty.set(true);
        if self.sidebar_tab.get() == 0 {
            self.updating.set(true);
            self.render_sidebar(&self.snapshot.borrow());
            self.updating.set(false);
        }
    }

    /// Collects watcher, search and move-worker results.
    fn poll_core(&self) {
        let has_workspace = self.snapshot.borrow().workspace_root.is_some();
        if has_workspace {
            let result = self
                .core
                .borrow_mut()
                .dispatch_if_changed(AppCommand::PollWorkspaceChanges);
            match result {
                Ok(Some(snapshot)) => self.render(snapshot),
                Ok(None) => {}
                Err(error) => self.status.set_label(&error.to_string()),
            }
            let result = self
                .core
                .borrow_mut()
                .dispatch_if_changed(AppCommand::PollSearchResults);
            match result {
                Ok(Some(snapshot)) => self.render(snapshot),
                Ok(None) => {}
                Err(error) => self.status.set_label(&error.to_string()),
            }
        }
        self.sync_pending_move();
        let background_error = self.core.borrow_mut().take_background_error();
        if let Some(error) = background_error {
            self.background_move_error.replace(Some(error.clone()));
            let snapshot = self.core.borrow().snapshot();
            self.render(snapshot);
            self.status.set_label(&error);
        }
    }

    fn request_close_document(&self, id: DocumentId) {
        if !self
            .snapshot
            .borrow()
            .documents
            .iter()
            .any(|doc| doc.id == id && doc.dirty)
        {
            self.dispatch(AppCommand::CloseDocument(id));
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Save changes to “{}”?", id.as_str()))
            .body("Your changes will be lost if you close without saving.")
            .build();
        dialog.add_responses(&[
            ("cancel", "Cancel"),
            ("discard", "Discard Changes"),
            ("save", "Save"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let root = self.snapshot.borrow().workspace_root.clone();
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if ui.snapshot.borrow().workspace_root != root {
                return;
            }
            if response == "discard" {
                ui.dispatch(AppCommand::DiscardAndCloseDocument(id.clone()));
            }
            if response == "save" {
                ui.dispatch(AppCommand::SaveDocument(id.clone()));
                if !ui
                    .snapshot
                    .borrow()
                    .documents
                    .iter()
                    .any(|d| d.id == id && d.dirty)
                {
                    ui.dispatch(AppCommand::CloseDocument(id.clone()));
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    fn confirm_delete(&self, path: String) {
        if self.core.borrow().has_pending_move() {
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Delete “{path}”?"))
            .body("This permanently deletes the entry and all its contents. Modified documents must be saved first. This cannot be undone.")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete Permanently")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let requested_root = self.snapshot.borrow().workspace_root.clone();
        let ui = self.clone();
        dialog.connect_response(Some("delete"), move |_, _| {
            if ui.snapshot.borrow().workspace_root != requested_root {
                return;
            }
            if let Ok(id) = DocumentId::parse(path.clone()) {
                ui.dispatch(AppCommand::DeleteEntry(id));
                ui.sync_pending_move();
                ui.watch_pending_move();
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Collect move and deletion completions promptly without joining the worker.
    fn watch_pending_move(&self) {
        let ui = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            ui.poll_core();
            if ui.core.borrow().has_pending_move() {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    fn sync_pending_move(&self) {
        self.window
            .set_sensitive(!self.core.borrow().deletion_pending());
        self.editor.set_editable(
            self.snapshot.borrow().active_document.is_some()
                && !self.core.borrow().deletion_pending(),
        );
        let target = self
            .core
            .borrow()
            .pending_move_target()
            .map(|id| id.as_str().to_owned());
        if *self.pending_move_target.borrow() == target {
            return;
        }
        self.pending_move_target.replace(target.clone());
        for sidebar in &self.sidebars {
            sidebar.pending_rows.borrow_mut().retain(|entry| {
                let Some(row) = entry.row.upgrade() else {
                    return false;
                };
                if let Some(spinner) = entry.spinner.upgrade() {
                    update_pending_row(&row, &spinner, &entry.path, target.as_deref());
                }
                true
            });
        }
    }

    fn render_document_tabs(&self, snapshot: &AppSnapshot) {
        let mut pages = self.document_pages.borrow_mut();
        // Retain native pages across renames and moves, including their order.
        if pages.len() == snapshot.open_documents.len() {
            let old_ids: std::collections::HashSet<_> =
                pages.iter().map(|(id, _)| id.clone()).collect();
            for ((old, _), new) in pages.iter_mut().zip(&snapshot.open_documents) {
                if !snapshot.open_documents.contains(old) && !old_ids.contains(new) {
                    *old = new.clone();
                }
            }
        }
        pages.retain(|(id, page)| {
            if snapshot.open_documents.contains(id) {
                true
            } else {
                self.document_tabs.close_page(page);
                false
            }
        });
        for (position, id) in snapshot.open_documents.iter().enumerate() {
            let page = if let Some((_, page)) = pages.iter().find(|(open, _)| open == id) {
                page.clone()
            } else {
                let child = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let page = self.document_tabs.append(&child);
                pages.push((id.clone(), page.clone()));
                page
            };
            if self.document_tabs.page_position(&page) != position as i32 {
                self.document_tabs.reorder_page(&page, position as i32);
            }
            let dirty = snapshot
                .documents
                .binary_search_by(|doc| doc.id.cmp(id))
                .ok()
                .is_some_and(|index| snapshot.documents[index].dirty);
            let title = id.as_str().rsplit('/').next().unwrap();
            let indicator = document_tab_indicator_icon(dirty);
            if page.indicator_icon().as_ref() != Some(&indicator) {
                page.set_indicator_icon(Some(&indicator));
                page.set_indicator_tooltip(if dirty { "Unsaved changes" } else { "" });
            }
            if page.title() != title {
                page.set_title(title);
            }
            let tooltip = glib::markup_escape_text(id.as_str());
            if page.tooltip().as_ref() != Some(&tooltip) {
                page.set_tooltip(&tooltip);
            }
            let icon = document_tab_icon(id.as_str());
            if page.icon().as_ref() != Some(&icon) {
                page.set_icon(Some(&icon));
            }
            if snapshot
                .active_document
                .as_ref()
                .is_some_and(|doc| &doc.id == id)
                && self.document_tabs.selected_page().as_ref() != Some(&page)
            {
                self.document_tabs.set_selected_page(&page);
            }
        }
        // Keep the lookup in visual order for the next relocation snapshot.
        pages.sort_by_key(|(_, page)| self.document_tabs.page_position(page));
        drop(pages);
        let target = self
            .document_tabs
            .selected_page()
            .map(|page| page.child().downcast::<gtk::Box>().expect("document page"))
            .unwrap_or_else(|| self.empty_editor_page.clone());
        if self.editor_panel.parent().as_ref() != Some(target.upcast_ref()) {
            if let Some(parent) = self.editor_panel.parent().and_downcast::<gtk::Box>() {
                parent.remove(&self.editor_panel);
            }
            target.append(&self.editor_panel);
        }
        self.editor_stack
            .set_visible_child_name(if snapshot.open_documents.is_empty() {
                "empty"
            } else {
                "documents"
            });
    }

    fn render(&self, snapshot: AppSnapshot) {
        self.sync_pending_move();
        let diagnostic_started = self
            .core
            .borrow()
            .devtools_enabled()
            .then(std::time::Instant::now);
        let diagnostic_operation = snapshot.diagnostic_operation;
        let diagnostic_revision = snapshot.revision;
        self.updating.set(true);
        if self.snapshot.borrow().open_documents != snapshot.open_documents {
            let open: std::collections::HashSet<_> = snapshot
                .open_documents
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect();
            for sidebar in &self.sidebars {
                for row in sidebar.pending_rows.borrow().iter() {
                    if let Some(indicator) = row.open_indicator.upgrade() {
                        indicator.set_visible(row.is_file && open.contains(&row.path));
                    }
                }
            }
            self.sidebars[0].open_documents.replace(open);
        }
        {
            let previous = self.snapshot.borrow();
            if previous.workspace_root != snapshot.workspace_root
                || previous.documents != snapshot.documents
                || previous.directories != snapshot.directories
            {
                for sidebar in &self.sidebars {
                    sidebar.dirty.set(true);
                }
                let query = self.file_search.text();
                if !query.trim().is_empty() {
                    // Refilter the new paths; the current matches stay until then.
                    self.path_filter.borrow_mut().queue(
                        &query,
                        snapshot.documents.clone(),
                        snapshot.directories.clone(),
                    );
                    self.watch_file_filter();
                }
            }
            if previous.search_results != snapshot.search_results
                || previous.search_query != snapshot.search_query
            {
                self.sidebars[1].dirty.set(true);
            }
        }
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
        let tabs_changed = {
            let previous = self.snapshot.borrow();
            previous.open_documents != snapshot.open_documents
                || previous.active_document.as_ref().map(|doc| &doc.id)
                    != snapshot.active_document.as_ref().map(|doc| &doc.id)
                || previous.documents != snapshot.documents
        };
        if tabs_changed {
            self.render_document_tabs(&snapshot);
        }
        let recent_changed = self.snapshot.borrow().recent_workspaces != snapshot.recent_workspaces;
        let workspace_changed = self.snapshot.borrow().workspace_root != snapshot.workspace_root;
        if workspace_changed {
            // Every folder starts collapsed again; rebind all rows.
            for sidebar in &self.sidebars {
                sidebar.visible_entries.borrow_mut().clear();
                sidebar
                    .file_model
                    .splice(0, sidebar.file_model.n_items(), &[]);
            }
        }
        if workspace_changed && snapshot.workspace_root.is_some() {
            self.window.present();
            self.home_window.set_visible(false);
        }
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
        self.search_spinner
            .set_visible(self.sidebar_tab.get() == 1 && snapshot.search_in_progress);
        self.search_truncated
            .set_visible(self.sidebar_tab.get() == 1 && snapshot.search_truncated);
        self.conflict_banner.set_visible(
            snapshot
                .active_document
                .as_ref()
                .is_some_and(|document| document.external_change),
        );
        self.counter
            .set_label(&format!("Counter: {}", snapshot.diagnostic_counter));
        self.editor.set_editable(
            snapshot.active_document.is_some() && !self.core.borrow().deletion_pending(),
        );
        self.status
            .set_label(match snapshot.active_document.as_ref() {
                Some(doc) if doc.dirty => "Modified",
                Some(_) => "Saved",
                None => "Select a document",
            });
        if self.core.borrow().deletion_pending() {
            self.status.set_label("Deleting…");
        } else if let Some(error) = self.background_move_error.borrow().as_deref() {
            self.status.set_label(error);
        }
        if recent_changed {
            while let Some(child) = self.recent_list.first_child() {
                self.recent_list.remove(&child);
            }
            for workspace in &snapshot.recent_workspaces {
                let button = gtk::Button::new();
                let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
                let name = gtk::Label::new(Some(&workspace.name));
                name.set_xalign(0.0);
                let path_label = gtk::Label::new(Some(&workspace.root.to_string_lossy()));
                path_label.set_xalign(0.0);
                path_label.add_css_class("dim-label");
                path_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                labels.append(&name);
                labels.append(&path_label);
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.append(&gtk::Image::from_icon_name("folder-symbolic"));
                labels.set_hexpand(true);
                row.append(&labels);
                row.append(&gtk::Image::from_icon_name("go-next-symbolic"));
                button.set_child(Some(&row));
                button.set_tooltip_text(Some(&workspace.root.to_string_lossy()));
                button.add_css_class("card");
                button.set_halign(gtk::Align::Fill);
                let ui = self.clone();
                let path = workspace.root.clone();
                button
                    .connect_clicked(move |_| ui.dispatch(AppCommand::OpenWorkspace(path.clone())));
                self.recent_list.append(&button);
            }
        }
        self.resume.set_visible(snapshot.workspace_root.is_some());
        self.recent_empty
            .set_visible(snapshot.recent_workspaces.is_empty());
        self.render_sidebar(&snapshot);
        let content = snapshot
            .active_document
            .as_ref()
            .map(|doc| doc.content.as_str())
            .unwrap_or("");
        let (start, end) = self.buffer.bounds();
        let content_changed = self.buffer.text(&start, &end, false).as_str() != content;
        let language_changed = self
            .snapshot
            .borrow()
            .active_document
            .as_ref()
            .map(|doc| doc.id.as_str().rsplit('/').next().unwrap())
            != snapshot
                .active_document
                .as_ref()
                .map(|doc| doc.id.as_str().rsplit('/').next().unwrap());
        let previous = self.snapshot.borrow();
        let previous_id = previous.active_document.as_ref().map(|doc| &doc.id);
        let next_id = snapshot.active_document.as_ref().map(|doc| &doc.id);
        let switched_document = previous_id != next_id
            && !(previous.open_documents.len() == snapshot.open_documents.len()
                && previous_id
                    .is_some_and(|id| !snapshot.documents.iter().any(|doc| &doc.id == id)));
        drop(previous);
        if switched_document {
            // Equal text does not invoke set_text, so explicitly end the old
            // document's undo history before reusing the native buffer.
            self.buffer.begin_irreversible_action();
        }
        if content_changed {
            self.buffer.set_text(content);
        }
        if switched_document {
            self.buffer.end_irreversible_action();
            self.buffer.place_cursor(&self.buffer.start_iter());
        }
        if content_changed || language_changed {
            syntax::apply(
                &self.buffer,
                snapshot
                    .active_document
                    .as_ref()
                    .map_or("", |doc| doc.id.as_str()),
            );
        }
        self.editor_chrome.set_file_name(
            snapshot
                .active_document
                .as_ref()
                .map_or("", |doc| doc.id.as_str()),
        );
        let previous = self.snapshot.replace(snapshot);
        // Freeing a large workspace listing takes milliseconds; do it off the UI thread.
        if previous.documents.len() > 10_000 {
            std::thread::spawn(move || drop(previous));
        }
        self.updating.set(false);
        if let Some(started) = diagnostic_started {
            self.core.borrow().diagnostic_snapshot_applied(
                diagnostic_operation,
                diagnostic_revision,
                started.elapsed().as_nanos() as u64,
            );
        }
    }

    fn sidebar(&self) -> &SidebarView {
        &self.sidebars[usize::from(self.search_mode.get())]
    }

    fn switch_sidebar(&self, tab: u8) {
        self.sidebar_tab.set(tab);
        self.file_search_row.set_visible(tab == 0);
        self.text_search_row.set_visible(tab == 1);
        self.search_spinner
            .set_visible(tab == 1 && self.snapshot.borrow().search_in_progress);
        self.search_truncated
            .set_visible(tab == 1 && self.snapshot.borrow().search_truncated);
        if tab >= 2 {
            self.sidebar_pages.set_visible_child_name("empty");
            return;
        }
        self.search_mode.set(tab == 1);
        self.updating.set(true);
        self.render_sidebar(&self.snapshot.borrow());
        self.updating.set(false);
        self.sidebar_pages
            .set_visible_child_name(if tab == 0 { "files" } else { "search" });
    }

    fn render_sidebar(&self, snapshot: &AppSnapshot) {
        let previous_selection = self
            .sidebar()
            .sidebar_rows
            .borrow()
            .get(self.sidebar().file_selection.selected() as usize)
            .cloned();
        let results_changed = self.sidebar().dirty.replace(false);
        self.sidebar()
            .files_placeholder
            .set_label(if snapshot.workspace_name.is_none() {
                "Open a workspace"
            } else if self.search_mode.get() {
                if snapshot.search_query.0.is_empty() {
                    "Search file contents"
                } else {
                    "No text matches"
                }
            } else if !self.file_query.borrow().is_empty() {
                "No matching names"
            } else {
                "No files"
            });
        if results_changed && !self.search_mode.get() && self.file_query.borrow().trim().is_empty()
        {
            // The plain file tree lists open folders only. Searches and name
            // filters still need every path and use the full ordering below.
            let mut entries = Vec::new();
            tree_entries(
                snapshot,
                "",
                &self.collapsed_directories.borrow(),
                &mut entries,
            );
            self.sidebar().all_entries.replace(entries);
            self.rebuild_visible_sidebar();
        } else if results_changed {
            let mut entries = Vec::new();
            let text_mode = self.search_mode.get();
            let groups: std::collections::HashMap<String, Vec<&SearchResult>> = snapshot
                .search_results
                .chunk_by(|left, right| left.id == right.id)
                .filter(|_| text_mode)
                .filter_map(|group| {
                    let matches: Vec<_> = group
                        .iter()
                        .filter(|result| result.location.is_some())
                        .collect();
                    (!matches.is_empty()).then(|| (group[0].id.as_str().to_owned(), matches))
                })
                .collect();
            let mut paths: Vec<(String, bool)> = Vec::new();
            if text_mode {
                paths.extend(groups.keys().cloned().map(|path| (path, false)));
            } else {
                // The worker already listed every folder holding a match.
                let filter = self.file_filter.borrow();
                paths.extend(filter.files.iter().cloned().map(|path| (path, false)));
                paths.extend(filter.directories.iter().cloned().map(|path| (path, true)));
            }
            if text_mode {
                let matched_paths = paths.clone();
                for (id, _) in matched_paths {
                    let mut parent = std::path::Path::new(&id).parent();
                    while let Some(path) = parent.and_then(|path| path.to_str()) {
                        if path.is_empty() {
                            break;
                        }
                        paths.push((path.to_owned(), true));
                        parent = std::path::Path::new(path).parent();
                    }
                }
            }
            let directories: std::collections::HashSet<_> = paths
                .iter()
                .filter(|(_, is_directory)| *is_directory)
                .map(|(path, _)| path.clone())
                .collect();
            let paths = order_sidebar_paths(paths, &directories);
            for (path, is_directory) in paths {
                let depth = path.matches('/').count();
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                if is_directory {
                    entries.push(SidebarEntry {
                        row: SidebarRow::Directory(path.clone()),
                        path,
                        title: name,
                        depth,
                        suffix: None,
                    });
                    continue;
                }
                let Some(document) = snapshot.document_summary(&path) else {
                    continue;
                };
                let suffix = if document.external_change {
                    Some("⚠")
                } else if document.dirty {
                    Some("●")
                } else {
                    None
                };
                entries.push(SidebarEntry {
                    row: SidebarRow::File(document.id.clone()),
                    path: path.clone(),
                    title: name,
                    depth,
                    suffix,
                });
                for result in groups.get(&path).into_iter().flatten() {
                    let location = result.location.as_ref().unwrap();
                    let title = match result.snippet.as_deref() {
                        Some(snippet) => {
                            format!("↳ {}:{}  {}", location.line, location.column, snippet)
                        }
                        None => format!("↳ {}:{}", location.line, location.column),
                    };
                    entries.push(SidebarEntry {
                        row: SidebarRow::Match((**result).clone()),
                        path: path.clone(),
                        title,
                        depth: depth + 1,
                        suffix: None,
                    });
                }
            }
            self.sidebar().all_entries.replace(entries);
            self.rebuild_visible_sidebar();
        }
        let selected_directory = previous_selection.as_ref().and_then(|previous| {
            if let SidebarRow::Directory(path) = previous {
                self.sidebar().sidebar_rows.borrow().iter().position(
                    |row| matches!(row, SidebarRow::Directory(candidate) if candidate == path),
                )
            } else {
                None
            }
        });
        if let Some(index) = selected_directory {
            self.sidebar().file_selection.set_selected(index as u32);
        } else if let Some(active) = snapshot.active_document.as_ref() {
            let rows = self.sidebar().sidebar_rows.borrow();
            let selected_index = previous_selection.as_ref().and_then(|previous| {
                rows.iter().position(|row| {
                    row.id() == Some(&active.id)
                        && row.id() == previous.id()
                        && row.location() == previous.location()
                })
            });
            let index = selected_index.or_else(|| {
                if !self.search_mode.get() && self.file_query.borrow().trim().is_empty() {
                    rows.iter()
                        .position(|row| matches!(row, SidebarRow::File(id) if id == &active.id))
                } else {
                    None
                }
            });
            if let Some(index) = index {
                self.sidebar().file_selection.set_selected(index as u32);
            } else {
                self.sidebar().file_selection.set_selected(u32::MAX);
            }
        } else {
            self.sidebar().file_selection.set_selected(u32::MAX);
        }
        self.sync_sidebar_selection();
    }

    fn sync_sidebar_selection(&self) {
        let selected = self
            .sidebar()
            .sidebar_rows
            .borrow()
            .get(self.sidebar().file_selection.selected() as usize)
            .and_then(|row| match row {
                SidebarRow::Directory(path) => Some(path.clone()),
                SidebarRow::File(id) => Some(id.as_str().to_owned()),
                SidebarRow::Match(_) => None,
            });
        self.sidebar().interactions.selected.replace(selected);
    }

    fn rebuild_visible_sidebar(&self) {
        let searching = self.search_mode.get() || !self.file_query.borrow().trim().is_empty();
        let collapsed = self.collapsed_directories.borrow();
        let mut hidden_prefix: Option<String> = None;
        let mut visible = Vec::new();
        for entry in self.sidebar().all_entries.borrow().iter() {
            if let Some(prefix) = hidden_prefix.as_ref() {
                if entry.path.starts_with(prefix) {
                    continue;
                }
                hidden_prefix = None;
            }
            if !searching
                && let SidebarRow::Directory(path) = &entry.row
                && collapsed.contains(path)
            {
                hidden_prefix = Some(format!("{path}/"));
            }
            visible.push(entry.clone());
        }
        // Rebinding a row rebuilds its widgets. Replace only the changed span,
        // so a move updates a few rows instead of every visible one.
        let (start, removed, added) = {
            let old = self.sidebar().visible_entries.borrow();
            let same = |left: &SidebarEntry, right: &SidebarEntry| {
                left.path == right.path
                    && left.title == right.title
                    && left.depth == right.depth
                    && left.suffix == right.suffix
                    && std::mem::discriminant(&left.row) == std::mem::discriminant(&right.row)
            };
            let start = old
                .iter()
                .zip(&visible)
                .take_while(|(left, right)| same(left, right))
                .count();
            let tail = old[start..]
                .iter()
                .rev()
                .zip(visible[start..].iter().rev())
                .take_while(|(left, right)| same(left, right))
                .count();
            (
                start,
                old.len() - start - tail,
                visible.len() - start - tail,
            )
        };
        if removed > 0 {
            let old = self.sidebar().visible_entries.borrow();
            self.release_focus_from_rows(&old[start..start + removed]);
        }
        let rows = visible.iter().map(|entry| entry.row.clone()).collect();
        let empty = visible.is_empty();
        self.sidebar().visible_entries.replace(visible);
        self.sidebar().sidebar_rows.replace(rows);
        self.sidebar()
            .file_model
            .splice(start as u32, removed as u32, &vec![""; added]);
        self.sidebar().files_placeholder.set_visible(empty);
    }

    /// Removing the row that holds keyboard focus makes GTK move focus to
    /// another row and scroll to it, which jumped the sidebar to the top
    /// after every drag. Release focus first; the scroll position stays.
    fn release_focus_from_rows(&self, removed: &[SidebarEntry]) {
        let files = &self.sidebar().files;
        let Some(root) = files.root() else { return };
        let Some(focus) = gtk::prelude::RootExt::focus(&root) else {
            return;
        };
        if !focus.is_ancestor(files) {
            return;
        }
        // Large spans come from collapsing big folders; skip the lookup.
        let holds_focus = removed.len() > 64
            || removed.iter().any(|entry| {
                self.sidebar()
                    .interactions
                    .row_widget(&entry.path)
                    .is_some_and(|row| row.is_ancestor(&focus) || focus.is_ancestor(&row))
            });
        if holds_focus {
            gtk::prelude::RootExt::set_focus(&root, None::<&gtk::Widget>);
        }
    }

    fn toggle_directory(&self, path: &str) {
        if self.search_mode.get() || !self.file_query.borrow().trim().is_empty() {
            return;
        }
        let Some(visible_index) = self
            .sidebar()
            .visible_entries
            .borrow()
            .iter()
            .position(|entry| matches!(&entry.row, SidebarRow::Directory(id) if id == path))
        else {
            return;
        };
        let Some(all_index) = self
            .sidebar()
            .all_entries
            .borrow()
            .iter()
            .position(|entry| matches!(&entry.row, SidebarRow::Directory(id) if id == path))
        else {
            return;
        };
        let prefix = format!("{path}/");
        let old_children = self.sidebar().visible_entries.borrow()[visible_index + 1..]
            .iter()
            .take_while(|entry| entry.path.starts_with(&prefix))
            .count();
        let mut collapsed = self.collapsed_directories.borrow_mut();
        let expanding = collapsed.remove(path);
        if !expanding {
            collapsed.insert(path.to_owned());
        }
        let mut replacement = Vec::new();
        if expanding {
            // Collapsed folders were listed without contents; load them now.
            tree_entries(&self.snapshot.borrow(), path, &collapsed, &mut replacement);
            let mut all = self.sidebar().all_entries.borrow_mut();
            let loaded = all[all_index + 1..]
                .iter()
                .take_while(|entry| entry.path.starts_with(&prefix))
                .count();
            all.splice(
                all_index + 1..all_index + 1 + loaded,
                replacement.iter().cloned(),
            );
        }
        drop(collapsed);
        let selected = self
            .sidebar()
            .sidebar_rows
            .borrow()
            .get(self.sidebar().file_selection.selected() as usize)
            .cloned();
        self.updating.set(true);
        if old_children > 0 {
            let visible = self.sidebar().visible_entries.borrow();
            self.release_focus_from_rows(
                &visible[visible_index + 1..visible_index + 1 + old_children],
            );
        }
        let range = visible_index + 1..visible_index + old_children + 1;
        self.sidebar().sidebar_rows.borrow_mut().splice(
            range.clone(),
            replacement.iter().map(|entry| entry.row.clone()),
        );
        let placeholders = vec![""; replacement.len()];
        self.sidebar()
            .visible_entries
            .borrow_mut()
            .splice(range, replacement);
        self.sidebar().file_model.splice(
            (visible_index + 1) as u32,
            old_children as u32,
            &placeholders,
        );
        if let Some(widget) = self.sidebar().interactions.row_widget(path)
            && let Some(marker) = widget
                .first_child()
                .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        {
            marker.set_label(if expanding { "▾" } else { "▸" });
            if let Some(image) = marker
                .next_sibling()
                .and_then(|widget| widget.downcast::<gtk::Image>().ok())
            {
                image.set_paintable(Some(&folder_icon_for(path, expanding)));
            }
        }
        if let Some(selected) = selected {
            let index = self.sidebar().sidebar_rows.borrow().iter().position(|row| {
                match (&selected, row) {
                    (SidebarRow::Directory(a), SidebarRow::Directory(b)) => a == b,
                    _ => selected.id() == row.id() && selected.location() == row.location(),
                }
            });
            if let Some(index) = index {
                self.sidebar().file_selection.set_selected(index as u32);
            }
        }
        self.sync_sidebar_selection();
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
        let start_offset = start.offset();
        let end_offset = end.offset();
        let mark = self.buffer.create_mark(None, &end, false);
        let buffer = self.buffer.clone();
        let editor = self.editor.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
            if let Some((selected_start, selected_end)) = buffer.selection_bounds()
                && selected_start.offset() == start_offset
                && selected_end.offset() == end_offset
            {
                editor.scroll_to_mark(&mark, 0.2, false, 0.0, 0.0);
            }
            buffer.delete_mark(&mark);
        });
    }
}

/// Rows of `parent`'s open subtree. Collapsed folders appear without their
/// contents, so an update costs the visible tree instead of the workspace.
fn tree_entries(
    snapshot: &AppSnapshot,
    parent: &str,
    collapsed: &std::collections::HashSet<String>,
    entries: &mut Vec<SidebarEntry>,
) {
    for child in snapshot.sidebar_children(parent) {
        let depth = child.path.matches('/').count();
        let title = child
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&child.path)
            .to_owned();
        if child.directory {
            entries.push(SidebarEntry {
                row: SidebarRow::Directory(child.path.clone()),
                path: child.path.clone(),
                title,
                depth,
                suffix: None,
            });
            if !collapsed.contains(&child.path) {
                tree_entries(snapshot, &child.path, collapsed, entries);
            }
        } else if let Some(document) = snapshot.document_summary(&child.path) {
            entries.push(SidebarEntry {
                row: SidebarRow::File(document.id.clone()),
                path: child.path,
                title,
                depth,
                suffix: if document.external_change {
                    Some("⚠")
                } else if document.dirty {
                    Some("●")
                } else {
                    None
                },
            });
        }
    }
}

/// Orders rows depth-first, with directories before files inside each folder.
/// Siblings are sorted once with precomputed keys: comparing whole paths
/// rebuilt every prefix per comparison and took seconds on node_modules trees.
fn order_sidebar_paths(
    mut paths: Vec<(String, bool)>,
    directories: &std::collections::HashSet<String>,
) -> Vec<(String, bool)> {
    use std::collections::{HashMap, HashSet};
    paths.sort_unstable();
    paths.dedup();
    let order = {
        // Parents are prefixes of their rows. Ancestors missing from the rows
        // still order their descendants without being emitted.
        let mut children: HashMap<&str, Vec<(&str, Option<usize>)>> = HashMap::new();
        let mut known: HashSet<&str> = paths.iter().map(|(path, _)| path.as_str()).collect();
        for (index, (path, _)) in paths.iter().enumerate() {
            let mut child = path.as_str();
            let mut row = Some(index);
            loop {
                let parent = child.rsplit_once('/').map_or("", |(parent, _)| parent);
                children.entry(parent).or_default().push((child, row));
                if parent.is_empty() || !known.insert(parent) {
                    break;
                }
                child = parent;
                row = None;
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_by_cached_key(|&(path, _)| {
                let name = path.rsplit('/').next().unwrap_or(path);
                (!directories.contains(path), name.to_lowercase(), name)
            });
        }
        let mut order = Vec::with_capacity(paths.len());
        let mut expanded = HashSet::new();
        let mut stack = vec![("", 0)];
        while let Some((parent, position)) = stack.pop() {
            let Some(&(path, row)) = children
                .get(parent)
                .and_then(|siblings| siblings.get(position))
            else {
                continue;
            };
            order.extend(row);
            stack.push((parent, position + 1));
            if expanded.insert(path) {
                stack.push((path, 0));
            }
        }
        order
    };
    let mut rows: Vec<_> = paths.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|row| rows[row].take())
        .collect()
}

fn sidebar_row(
    title: &str,
    depth: usize,
    disclosure: Option<&str>,
    icon: Option<&gtk::gdk::Texture>,
    suffix: Option<&str>,
) -> gtk::Box {
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
    label.set_width_chars(1);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&label);
    if let Some(suffix) = suffix {
        content.append(&gtk::Label::new(Some(suffix)));
    }
    content
}

/// AdwOverlaySplitView has no resize handle of its own. Dragging this strip
/// on the sidebar's trailing edge adjusts its preferred width.
fn sidebar_resize_handle(split: &adw::OverlaySplitView) -> gtk::Box {
    const MIN_WIDTH: f64 = 220.0;
    const MIN_CONTENT_WIDTH: f64 = 320.0;
    let handle = gtk::Box::new(gtk::Orientation::Vertical, 0);
    handle.set_widget_name("sidebar-resize-handle");
    handle.set_width_request(6);
    handle.set_halign(gtk::Align::End);
    handle.set_cursor_from_name(Some("col-resize"));
    let drag = gtk::GestureDrag::new();
    drag.set_name(Some("sidebar-resize"));
    // The handle moves with the sidebar, so track the pointer in the split
    // view's coordinates rather than by the gesture's own offset.
    let pointer = {
        let handle = handle.downgrade();
        let split = split.downgrade();
        move |x: f64, y: f64| {
            let handle = handle.upgrade()?;
            let split = split.upgrade()?;
            handle
                .compute_point(&split, &gtk::graphene::Point::new(x as f32, y as f32))
                .map(|point| point.x() as f64)
        }
    };
    let start = Rc::new(Cell::new((0.0, 0.0, 0.0, 0.0)));
    {
        let start = start.clone();
        let split = split.downgrade();
        let pointer = pointer.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(split) = split.upgrade() else {
                return;
            };
            let width = split.sidebar().map_or(0, |sidebar| sidebar.width()) as f64;
            start.set((width, pointer(x, y).unwrap_or_default(), x, y));
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
    }
    {
        let split = split.downgrade();
        drag.connect_drag_update(move |_, dx, dy| {
            let Some(split) = split.upgrade() else {
                return;
            };
            let (width, origin, x, y) = start.get();
            let Some(current) = pointer(x + dx, y + dy) else {
                return;
            };
            let direction = if split.direction() == gtk::TextDirection::Rtl {
                -1.0
            } else {
                1.0
            };
            let max_width = (split.width() as f64 - MIN_CONTENT_WIDTH).max(MIN_WIDTH);
            let width = (width + (current - origin) * direction).clamp(MIN_WIDTH, max_width);
            split.set_sidebar_width_unit(adw::LengthUnit::Px);
            split.set_min_sidebar_width(MIN_WIDTH);
            split.set_max_sidebar_width(width);
            split.set_sidebar_width_fraction((width / f64::from(split.width()).max(1.0)).min(1.0));
        });
    }
    handle.add_controller(drag);
    handle
}

fn sidebar_entry_widget(entry: &SidebarEntry, collapsed: bool) -> gtk::Box {
    match &entry.row {
        SidebarRow::Directory(path) => sidebar_row(
            &entry.title,
            entry.depth,
            Some(if collapsed { "▸" } else { "▾" }),
            Some(&folder_icon_for(path, !collapsed)),
            None,
        ),
        SidebarRow::File(id) => sidebar_row(
            &entry.title,
            entry.depth,
            None,
            Some(&icon_for(id.as_str())),
            entry.suffix,
        ),
        SidebarRow::Match(_) => sidebar_row(&entry.title, entry.depth, None, None, None),
    }
}

fn icon_for(name: &str) -> gtk::gdk::Texture {
    texture_for_icon(file_icon_name(name))
}

fn folder_icon_for(name: &str, expanded: bool) -> gtk::gdk::Texture {
    texture_for_icon(folder_icon_name(name, expanded))
}

// Keep the native leading indicator slot present even for clean documents, so
// toggling the dirty marker never changes tab measurements or truncates it.
fn document_tab_indicator_icon(dirty: bool) -> gio::Icon {
    thread_local! {
        static DIRTY: gio::Icon = gio::ThemedIcon::new("native-workspace-modified-symbolic").upcast();
        static CLEAN: gio::Icon = gio::BytesIcon::new(&glib::Bytes::from_static(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#
        )).upcast();
    }
    if dirty {
        DIRTY.with(Clone::clone)
    } else {
        CLEAN.with(Clone::clone)
    }
}

fn document_tab_icon(path: &str) -> gio::Icon {
    thread_local! {
        static ICONS: RefCell<std::collections::HashMap<String, gio::Icon>> = RefCell::new(std::collections::HashMap::new());
    }
    let name = file_icon_name(path);
    ICONS.with(|icons| {
        icons
            .borrow_mut()
            .entry(name.to_owned())
            .or_insert_with(|| {
                material_icons::png_bytes(name)
                    .map(|bytes| gio::BytesIcon::new(&glib::Bytes::from_static(bytes)).upcast())
                    .unwrap_or_else(|| gio::ThemedIcon::new("text-x-generic-symbolic").upcast())
            })
            .clone()
    })
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

fn build_sidebar(
    collapsed_directories: &Rc<RefCell<std::collections::HashSet<String>>>,
    file_query: &Rc<RefCell<String>>,
    text_mode: bool,
    pending_move_target: &Rc<RefCell<Option<String>>>,
    open_documents: &Rc<RefCell<std::collections::HashSet<String>>>,
) -> SidebarView {
    let pending_rows = Rc::new(RefCell::new(Vec::<PendingRow>::new()));
    let visible_entries: Rc<RefCell<Vec<SidebarEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let file_model = gtk::StringList::new(&[]);
    let file_selection = gtk::SingleSelection::new(Some(file_model.clone()));
    file_selection.set_autoselect(false);
    file_selection.set_can_unselect(true);
    let interactions = Rc::new(sidebar_interactions::Interactions::default());
    let factory = gtk::SignalListItemFactory::new();
    {
        let visible_entries = visible_entries.clone();
        let collapsed_directories = collapsed_directories.clone();
        let file_query = file_query.clone();
        let interactions = interactions.clone();
        let pending_move_target = pending_move_target.clone();
        let pending_rows = pending_rows.clone();
        let open_documents = open_documents.clone();
        factory.connect_bind(move |_, item| {
            let item = item
                .downcast_ref::<gtk::ListItem>()
                .expect("sidebar list item");
            if let Some(entry) = visible_entries.borrow().get(item.position() as usize) {
                let collapsed = matches!(&entry.row, SidebarRow::Directory(path)
                    if !text_mode && file_query.borrow().trim().is_empty()
                        && collapsed_directories.borrow().contains(path));
                let widget = sidebar_entry_widget(entry, collapsed);
                let is_file = matches!(&entry.row, SidebarRow::File(_));
                let open_indicator = gtk::Image::from_icon_name("view-reveal-symbolic");
                open_indicator.set_pixel_size(12);
                open_indicator.set_widget_name("sidebar-open-indicator");
                open_indicator.set_tooltip_text(Some("Open in editor"));
                open_indicator
                    .set_visible(is_file && open_documents.borrow().contains(&entry.path));
                widget.append(&open_indicator);
                let spinner = gtk::Spinner::new();
                spinner.set_size_request(14, 14);
                widget.append(&spinner);
                update_pending_row(
                    &widget,
                    &spinner,
                    &entry.path,
                    pending_move_target.borrow().as_deref(),
                );
                pending_rows
                    .borrow_mut()
                    .retain(|entry| entry.row.upgrade().is_some());
                pending_rows.borrow_mut().push(PendingRow {
                    path: entry.path.clone(),
                    row: widget.downgrade(),
                    spinner: spinner.downgrade(),
                    open_indicator: open_indicator.downgrade(),
                    is_file,
                });
                let row_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                widget.set_hexpand(true);
                row_area.set_hexpand(true);
                row_area.append(&widget);
                if !matches!(entry.row, SidebarRow::Match(_)) {
                    interactions.attach(
                        &widget,
                        &entry.path,
                        matches!(entry.row, SidebarRow::Directory(_)),
                    );
                }
                item.set_child(Some(&row_area));
            }
        });
    }
    factory.connect_unbind(|_, item| {
        item.downcast_ref::<gtk::ListItem>()
            .expect("sidebar list item")
            .set_child(gtk::Widget::NONE);
    });
    let files = gtk::ListView::new(Some(file_selection.clone()), Some(factory));
    files.add_css_class("compact-files");
    files.set_size_request(220, -1);
    let files_placeholder = gtk::Label::new(Some("Open a workspace"));
    files_placeholder.set_halign(gtk::Align::Center);
    files_placeholder.set_valign(gtk::Align::Center);
    files_placeholder.set_can_target(false);
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("workspace-files-scroll");
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&files));
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scroll));
    overlay.add_overlay(&files_placeholder);
    let root_drop = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root_drop.set_halign(gtk::Align::Fill);
    root_drop.set_valign(gtk::Align::End);
    root_drop.append(&gtk::Label::new(Some("Move to workspace root")));
    interactions.attach_root(&root_drop);
    overlay.add_overlay(&root_drop);
    SidebarView {
        interactions,
        files,
        file_model,
        file_selection,
        visible_entries,
        files_placeholder,
        overlay,
        scroll,
        sidebar_rows: Rc::new(RefCell::new(Vec::new())),
        all_entries: Rc::new(RefCell::new(Vec::new())),
        dirty: Rc::new(Cell::new(true)),
        pending_rows,
        open_documents: open_documents.clone(),
    }
}

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

fn build_window(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    let mut core = Application::new();
    core.enable_devtools("gtk");
    let persistence_error = if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_none()
        && std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none()
    {
        core.enable_persistence_default()
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    let devtools_available = core.devtools_supported();
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
    let current_snapshot = Rc::new(RefCell::new(Application::new().snapshot()));
    let search_mode = Rc::new(Cell::new(false));
    let sidebar_tab = Rc::new(Cell::new(0));
    let file_query = Rc::new(RefCell::new(String::new()));
    let file_filter = Rc::new(RefCell::new(PathFilterResult::default()));
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
    let text_search_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    text_search_row.set_margin_start(8);
    text_search_row.set_margin_end(8);
    text_search_row.set_margin_bottom(6);
    search.set_hexpand(true);
    text_search_row.append(&search);
    text_search_row.append(&search_spinner);
    text_search_row.set_visible(false);
    sidebar_content.append(&text_search_row);
    let search_truncated = gtk::Label::new(Some(
        "Showing the first 20,000 matches. Refine the search to see the rest.",
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
    let counter = gtk::Label::new(Some("Counter: 0"));
    let diagnostic = icon_button("Increment", "list-add-symbolic");
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
        search_mode,
        sidebar_tab,
        file_query,
        file_filter,
        path_filter: Rc::new(RefCell::new(PathFilter::new())),
        watching_file_filter: Rc::new(Cell::new(false)),
        file_search_row: search_row,
        text_search_row,
        search_spinner,
        search_truncated,
        watching_search: Rc::new(Cell::new(false)),
        conflict_banner,
        document_tabs,
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
        counter,
        home_window: home_window.clone(),
        home_error,
        resume: resume.clone(),
        recent_list,
        recent_empty,
        updating: Rc::new(Cell::new(false)),
        background_move_error: Rc::new(RefCell::new(None)),
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
    if let Some(action) = app
        .lookup_action("devtools")
        .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
    {
        let core = ui.core.clone();
        action.connect_activate(move |_, _| {
            let mut app = core.borrow_mut();
            app.start_devtools("gtk");
            app.open_devtools();
        });
    }
    {
        let ui = ui.clone();
        file_tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(0);
        });
    }
    {
        let ui = ui.clone();
        text_tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(1);
        });
    }
    for (index, tab) in empty_tabs.into_iter().enumerate() {
        let ui = ui.clone();
        tab.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            ui.switch_sidebar(index as u8 + 2);
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
    for sidebar in &ui.sidebars {
        let rename_ui = ui.clone();
        let delete_ui = ui.clone();
        let move_ui = ui.clone();
        let trace_ui = ui.clone();
        let validate_ui = ui.clone();
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
                        Ok(snapshot) => {
                            rename_ui.render(snapshot);
                            true
                        }
                        Err(error) => {
                            rename_ui.status.set_label(&error.to_string());
                            false
                        }
                    }
                }),
                move_entry: Rc::new(move |path, directory| {
                    move_ui.background_move_error.replace(None);
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
                    let result = move_ui.core.borrow_mut().dispatch_sidebar_move(
                        id,
                        directory,
                        true,
                        trace.clone(),
                    );
                    match result {
                        Ok(snapshot) => {
                            move_ui.render(snapshot);
                            move_ui.watch_pending_move();
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
                can_move: Rc::new(move |source, directory| {
                    if validate_ui.core.borrow().has_pending_move() {
                        return false;
                    }
                    let name = source.rsplit('/').next().unwrap_or(source);
                    let target = if directory.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{directory}/{name}")
                    };
                    let snapshot = validate_ui.snapshot.borrow();
                    // Runs on every pointer motion: look up only the
                    // destination's children instead of every workspace path.
                    target != source
                        && !target.starts_with(&format!("{source}/"))
                        && !snapshot
                            .sidebar_children(directory)
                            .iter()
                            .any(|child| child.path == target)
                }),
                expand: Rc::new(move |path| {
                    if !expand_ui.search_mode.get()
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
                    if !collapse_ui.search_mode.get()
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
        app.set_accels_for_action("app.quit", &["<Primary>q"]);
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
        app.set_accels_for_action("app.save", &["<Primary>s"]);
    }
    {
        let ui = ui.clone();
        let action = gio::SimpleAction::new("save-all", None);
        action.connect_activate(move |_, _| ui.dispatch(AppCommand::SaveAllDocuments));
        app.add_action(&action);
        app.set_accels_for_action("app.save-all", &["<Primary><Shift>s"]);
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
                ui.dispatch(AppCommand::OpenDocument(id));
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
        app.set_accels_for_action("app.open", &["<Primary>o"]);
    }
    {
        let ui = ui.clone();
        ui.search.clone().connect_search_changed(move |search| {
            if !ui.updating.get() {
                ui.dispatch(AppCommand::QueueSearchQuery(search.text().to_string()));
                ui.watch_search();
            }
        });
    }
    for (mode, sidebar) in ui.sidebars.iter().enumerate() {
        {
            let ui = ui.clone();
            sidebar
                .file_selection
                .clone()
                .connect_selected_notify(move |selection| {
                    if ui.updating.get() || ui.sidebar_tab.get() != mode as u8 {
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
            sidebar.files.clone().connect_activate(move |_, index| {
                if ui.sidebar_tab.get() != mode as u8 {
                    return;
                }
                let selected = ui
                    .sidebar()
                    .sidebar_rows
                    .borrow()
                    .get(index as usize)
                    .cloned();
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
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            ui.poll_core();
            glib::ControlFlow::Continue
        });
    }
    {
        let ui = ui.clone();
        diagnostic.connect_clicked(move |_| ui.dispatch(AppCommand::IncrementDiagnostic));
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
    if std::env::var_os("NATIVE_WORKSPACE_SMOKE").is_some() {
        assert!(icons.has_icon("document-open-symbolic"));
        assert!(icons.has_icon("document-save-symbolic"));
        assert!(icons.has_icon(sidebar_icon));
        for icon in [
            "user-home-symbolic",
            "open-menu-symbolic",
            "native-workspace-save-all-symbolic",
            "native-workspace-new-file-symbolic",
            "folder-new-symbolic",
            "system-run-symbolic",
            "native-workspace-quit-symbolic",
            "folder-symbolic",
            "edit-find-symbolic",
            "native-workspace-branch-symbolic",
            "native-workspace-bookmark-symbolic",
        ] {
            assert!(icons.has_icon(icon), "missing icon: {icon}");
        }
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
        if std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none() {
            app.quit();
        }
    }
    if let Some(path) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE") {
        let workspace_path = PathBuf::from(path);
        ui.dispatch(AppCommand::OpenWorkspace(workspace_path.clone()));
        assert!(ui.window.is_visible());
        assert!(!ui.home_window.is_visible());
        let pending_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let pending_spinner = gtk::Spinner::new();
        pending_row.append(&pending_spinner);
        update_pending_row(&pending_row, &pending_spinner, "dest/a.txt", Some("dest"));
        assert!(pending_spinner.is_visible() && pending_spinner.is_spinning());
        assert!(pending_row.opacity() < 1.0);
        update_pending_row(&pending_row, &pending_spinner, "dest/a.txt", None);
        assert!(!pending_spinner.is_visible() && !pending_spinner.is_spinning());
        assert_eq!(pending_row.opacity(), 1.0);
        assert!(ui.window != ui.home_window);
        assert_eq!(ui.home_window.default_width(), 640);
        assert_eq!(ui.window.default_width(), 900);
        app.activate_action("home", None);
        assert!(ui.home_window.is_visible());
        assert!(!ui.window.is_visible());
        resume.emit_clicked();
        assert!(ui.window.is_visible());
        assert!(!ui.home_window.is_visible());
        {
            let settle = || {
                let until = std::time::Instant::now() + std::time::Duration::from_millis(80);
                while std::time::Instant::now() < until {
                    glib::MainContext::default().iteration(false);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while sidebar_resize.width() < 6 && std::time::Instant::now() < deadline {
                settle();
            }
            assert!(
                sidebar_resize.width() >= 6,
                "resize handle: {}x{}, sidebar: {}x{}, mapped: {}, shown: {}",
                sidebar_resize.width(),
                sidebar_resize.height(),
                sidebar_frame.width(),
                sidebar_frame.height(),
                sidebar_resize.is_mapped(),
                split.shows_sidebar()
            );
            assert_eq!(sidebar_resize.height(), sidebar_frame.height());
            for y in [20.0, f64::from(sidebar_resize.height()) / 2.0] {
                let point = sidebar_resize
                    .compute_point(&sidebar_frame, &gtk::graphene::Point::new(3.0, y as f32))
                    .expect("resize handle position");
                assert_eq!(
                    sidebar_frame.pick(
                        f64::from(point.x()),
                        f64::from(point.y()),
                        gtk::PickFlags::DEFAULT
                    ),
                    Some(sidebar_resize.clone().upcast::<gtk::Widget>()),
                    "resize handle must receive pointer events at the header and body"
                );
            }
            let gesture = sidebar_resize
                .observe_controllers()
                .iter::<glib::Object>()
                .filter_map(Result::ok)
                .find_map(|controller| controller.downcast::<gtk::GestureDrag>().ok())
                .expect("sidebar resize gesture");
            let initial_width = sidebar_frame.width();
            gesture.emit_by_name::<()>("drag-begin", &[&3.0_f64, &20.0_f64]);
            gesture.emit_by_name::<()>("drag-update", &[&80.0_f64, &0.0_f64]);
            settle();
            assert!((sidebar_frame.width() - initial_width - 80).abs() <= 1);
            // After allocation, the handle has moved under the pointer. Keeping
            // the pointer still must not apply the same delta a second time.
            gesture.emit_by_name::<()>("drag-update", &[&0.0_f64, &0.0_f64]);
            settle();
            assert!((sidebar_frame.width() - initial_width - 80).abs() <= 1);
            gesture.emit_by_name::<()>("drag-update", &[&40.0_f64, &0.0_f64]);
            settle();
            assert!((sidebar_frame.width() - initial_width - 120).abs() <= 1);
            gesture.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
            gesture.emit_by_name::<()>("drag-begin", &[&3.0_f64, &20.0_f64]);
            gesture.emit_by_name::<()>("drag-update", &[&10_000.0_f64, &0.0_f64]);
            settle();
            assert!(sidebar_frame.width() <= split.width() - 320);
            gesture.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
            gesture.emit_by_name::<()>("drag-begin", &[&3.0_f64, &20.0_f64]);
            gesture.emit_by_name::<()>("drag-update", &[&-10_000.0_f64, &0.0_f64]);
            settle();
            assert!(sidebar_frame.width() >= 220);
            assert_eq!(split.min_sidebar_width(), 220.0);
            assert_eq!(split.max_sidebar_width(), 220.0);
            gesture.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
            ui.window.set_default_size(700, 600);
            settle();
            assert!(sidebar_frame.width() >= 220);
            assert_eq!(sidebar_resize.height(), sidebar_frame.height());
            ui.window.set_default_size(900, 600);
            toggle_sidebar.emit_clicked();
            settle();
            assert!(!split.shows_sidebar());
            toggle_sidebar.emit_clicked();
            settle();
            assert!(split.shows_sidebar());
            assert!(sidebar_resize.is_mapped());
            println!("GTK sidebar resize/full-height handle/bounds/toggle smoke test passed");
        }
        assert_eq!(files_scroll.hscrollbar_policy(), gtk::PolicyType::Never);
        if ui
            .snapshot
            .borrow()
            .directories
            .iter()
            .any(|path| path == "src")
        {
            assert!(ui.collapsed_directories.borrow().contains("src"));
            assert_eq!(ui.sidebar().sidebar_rows.borrow().len(), 2);
            assert!(matches!(
                ui.sidebar().sidebar_rows.borrow()[0],
                SidebarRow::Directory(_)
            ));
            let unchanged_root_file = ui.sidebar().file_model.item(1).expect("root file row");
            ui.sidebar().file_selection.set_selected(0);
            let nested_index = ui
                .sidebar()
                .sidebar_rows
                .borrow()
                .iter()
                .position(|row| matches!(row, SidebarRow::File(id) if id.as_str() == "src/main.rs"))
                .expect("expanded directory file");
            let nested = ui.sidebar().visible_entries.borrow()[nested_index].clone();
            assert_eq!(sidebar_entry_widget(&nested, false).margin_start(), 24);
            assert_eq!(
                unchanged_root_file.as_ptr(),
                ui.sidebar()
                    .file_model
                    .item(ui.sidebar().file_model.n_items() - 1)
                    .expect("root file after expansion")
                    .as_ptr(),
                "expanding a folder should preserve rows outside its subtree"
            );
            ui.toggle_directory("src");
            assert_eq!(ui.sidebar().sidebar_rows.borrow().len(), 2);
            assert_eq!(ui.sidebar().file_selection.selected(), 0);
            ui.toggle_directory("src");
            assert!(ui.sidebar().sidebar_rows.borrow().len() > 2);
        }
        // Switching tabs must retain the file model and avoid touching editor tags.
        let retained_row = ui.sidebars[0].file_model.item(0).expect("file sidebar row");
        let model_changes = Rc::new(Cell::new(0));
        let tag_changes = Rc::new(Cell::new(0));
        let changed = model_changes.clone();
        let model_handler = ui.sidebars[0]
            .file_model
            .connect_items_changed(move |_, _, _, _| {
                changed.set(changed.get() + 1);
            });
        let changed = tag_changes.clone();
        let tag_handler = ui.buffer.connect_apply_tag(move |_, _, _, _| {
            changed.set(changed.get() + 1);
        });
        for _ in 0..3 {
            text_tab.set_active(true);
            file_tab.set_active(true);
            ui.switch_sidebar(2);
            assert_eq!(
                ui.sidebar_pages.visible_child_name().as_deref(),
                Some("empty")
            );
            ui.switch_sidebar(0);
        }
        assert_eq!(
            model_changes.get(),
            0,
            "tab switches rebuilt the file model"
        );
        assert_eq!(tag_changes.get(), 0, "tab switches reapplied syntax tags");
        assert_eq!(retained_row, ui.sidebars[0].file_model.item(0).unwrap());
        ui.sidebars[0].file_model.disconnect(model_handler);
        ui.buffer.disconnect(tag_handler);
        assert_eq!(
            window.title().as_deref(),
            ui.snapshot.borrow().workspace_name.as_deref()
        );
        let first = ui
            .sidebar()
            .sidebar_rows
            .borrow()
            .iter()
            .enumerate()
            .find_map(|(index, row)| match row {
                SidebarRow::File(id) if id.as_str() == "README.md" => Some((index, id.clone())),
                _ => None,
            });
        if let Some((index, id)) = first {
            ui.sidebar().file_selection.set_selected(index as u32);
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
            save.emit_clicked();
            assert_eq!(ui.status.text(), "Saved");
            // Name filters arrive from a worker after the entry's debounce.
            let filter_names = |query: &str| {
                ui.file_search.set_text(query);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                while *ui.file_query.borrow() != query {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "name filter {query:?} never applied"
                    );
                    glib::MainContext::default().iteration(false);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            };
            filter_names("main");
            assert!(
                ui.sidebar()
                    .sidebar_rows
                    .borrow()
                    .iter()
                    .any(|row| matches!(row,
                SidebarRow::File(id) if id.as_str() == "src/main.rs"))
            );
            assert!(
                !ui.sidebar()
                    .sidebar_rows
                    .borrow()
                    .iter()
                    .any(|row| matches!(row, SidebarRow::Match(_)))
            );
            filter_names("src");
            assert!(
                ui.sidebar()
                    .sidebar_rows
                    .borrow()
                    .iter()
                    .any(|row| matches!(row,
                SidebarRow::Directory(path) if path == "src"))
            );
            let filtered_row = ui.sidebars[0]
                .file_model
                .item(0)
                .expect("filtered file row");
            let selected = ui.sidebars[0].file_selection.selected();
            let cursor = ui.buffer.cursor_position();
            let changed = tag_changes.clone();
            let tag_handler = ui.buffer.connect_apply_tag(move |_, _, _, _| {
                changed.set(changed.get() + 1);
            });
            text_tab.set_active(true);
            file_tab.set_active(true);
            assert_eq!(filtered_row, ui.sidebars[0].file_model.item(0).unwrap());
            assert_eq!(selected, ui.sidebars[0].file_selection.selected());
            assert_eq!(cursor, ui.buffer.cursor_position());
            assert_eq!(
                tag_changes.get(),
                0,
                "tab switches rehighlighted the loaded document"
            );
            ui.buffer.disconnect(tag_handler);
            filter_names("");
            text_tab.set_active(true);
            ui.dispatch(AppCommand::SetSearchQuery("README".into()));
            assert!(ui.sidebar().sidebar_rows.borrow().is_empty());
            ui.dispatch(AppCommand::SetSearchQuery("phase 1".into()));
            assert_eq!(ui.snapshot.borrow().search_results.len(), 2);
            assert_eq!(ui.sidebar().sidebar_rows.borrow().len(), 3);
            let first_row = sidebar_entry_widget(&ui.sidebar().visible_entries.borrow()[0], false);
            let (_, natural_height, _, _) = first_row.measure(gtk::Orientation::Vertical, -1);
            assert!(
                natural_height <= 32,
                "sidebar row is {natural_height}px high"
            );
            assert!(matches!(
                ui.sidebar().sidebar_rows.borrow()[0],
                SidebarRow::File(_)
            ));
            assert_eq!(
                ui.snapshot.borrow().search_results[0].snippet.as_deref(),
                Some("GTK Phase 1 smoke content")
            );
            let search_row = ui.sidebars[1].file_model.item(2).expect("search match row");
            file_tab.set_active(true);
            assert!(
                ui.sidebars[0]
                    .sidebar_rows
                    .borrow()
                    .iter()
                    .all(|row| !matches!(row, SidebarRow::Match(_)))
            );
            text_tab.set_active(true);
            assert_eq!(search_row, ui.sidebars[1].file_model.item(2).unwrap());
            ui.sidebar().file_selection.set_selected(2);
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
            file_tab.set_active(true);
            assert_eq!(
                ui.snapshot.borrow().search_results.len(),
                ui.snapshot.borrow().documents.len()
            );
            let document_path = workspace_path.join(id.as_str());
            text_tab.set_active(true);
            std::fs::write(&document_path, "external clean edit").unwrap();
            std::fs::write(workspace_path.join("new-while-hidden.txt"), "new file").unwrap();
            ui.dispatch(AppCommand::RefreshWorkspace);
            assert!(ui.sidebars[0].dirty.get());
            file_tab.set_active(true);
            assert!(!ui.sidebars[0].dirty.get());
            assert!(ui.sidebars[0].sidebar_rows.borrow().iter().any(
                |row| matches!(row, SidebarRow::File(id) if id.as_str() == "new-while-hidden.txt")
            ));
            assert_eq!(
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
                "external clean edit"
            );
            assert!(!ui.conflict_banner.is_visible());
            ui.buffer.set_text("local edit");
            app.activate_action("home", None);
            resume.emit_clicked();
            assert_eq!(
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
                "local edit"
            );
            assert!(ui.snapshot.borrow().active_document.as_ref().unwrap().dirty);
            std::fs::write(&document_path, "external conflict").unwrap();
            ui.dispatch(AppCommand::RefreshWorkspace);
            assert!(ui.conflict_banner.is_visible());
            app.activate_action("save", None);
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
            ui.create_entry("notes", true);
            ui.create_entry("notes/todo.md", false);
            assert!(workspace_path.join("notes/todo.md").is_file());
            ui.buffer.set_text("first\nsecond\n");
            horizontal.set_value(0.0);
            for _ in 0..100 {
                glib::MainContext::default().iteration(false);
            }
            assert!(ui.editor.hexpands() && ui.editor.vexpands());
            let last = ui.buffer.end_iter();
            let (last_y, line_height) = ui.editor.line_yrange(&last);
            let (clicked, _) = ui.editor.line_at_y(last_y + line_height / 2);
            assert_eq!(
                clicked.offset(),
                last.offset(),
                "final empty line is unreachable"
            );
            ui.buffer.place_cursor(&clicked);
            assert_eq!(ui.buffer.cursor_position(), 13);
            let (first_y, _) = ui.editor.line_yrange(&ui.buffer.start_iter());
            assert!(last_y > first_y);
            let overlay = ui
                .editor_chrome
                .row
                .first_child()
                .unwrap()
                .downcast::<gtk::Overlay>()
                .unwrap();
            let origin = ui
                .editor
                .compute_point(&overlay, &gtk::graphene::Point::new(0.0, 0.0))
                .unwrap();
            assert!(origin.x() >= 48.0, "active line cannot include the gutter");
            ui.buffer.set_text("");
            assert!(ui.editor.line_yrange(&ui.buffer.start_iter()).1 > 0);
            println!("GTK editor full-width/active-line smoke test passed");
            ui.buffer.set_text("unsaved rename");
            if ui.collapsed_directories.borrow().contains("notes") {
                ui.toggle_directory("notes");
            }
            // Let the virtualized row bind before activating its inline editor.
            for _ in 0..100 {
                glib::MainContext::default().iteration(false);
            }
            let file_row = ui.sidebars[0]
                .interactions
                .row_widget("notes/todo.md")
                .expect("file row for selected-click rename");
            let controllers = file_row.observe_controllers();
            let selected_click = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .filter_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
                .find(|gesture| gesture.name().as_deref() == Some("sidebar-entry-click"))
                .expect("entry click gesture");
            let file_entry = ui.sidebars[0].interactions.editor("notes/todo.md").unwrap();
            let file_stack = file_entry
                .parent()
                .unwrap()
                .downcast::<gtk::Stack>()
                .unwrap();
            ui.sidebars[0].file_selection.set_selected(u32::MAX);
            selected_click.emit_by_name::<()>("pressed", &[&1i32, &24f64, &0f64]);
            assert_eq!(file_stack.visible_child_name().as_deref(), Some("label"));
            let file_index = ui.sidebars[0]
                .sidebar_rows
                .borrow()
                .iter()
                .position(
                    |row| matches!(row, SidebarRow::File(id) if id.as_str() == "notes/todo.md"),
                )
                .unwrap();
            ui.sidebars[0]
                .file_selection
                .set_selected(file_index as u32);
            selected_click.emit_by_name::<()>("pressed", &[&1i32, &24f64, &0f64]);
            assert_eq!(file_stack.visible_child_name().as_deref(), Some("edit"));
            assert_eq!(file_entry.selection_bounds(), Some((0, 4)));
            for _ in 0..40 {
                glib::MainContext::default().iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            file_entry.set_text("discarded.md");
            ui.editor.grab_focus();
            for _ in 0..20 {
                glib::MainContext::default().iteration(false);
            }
            assert_eq!(file_stack.visible_child_name().as_deref(), Some("label"));
            assert!(workspace_path.join("notes/todo.md").is_file());
            assert!(!workspace_path.join("notes/discarded.md").exists());
            println!("GTK native focus loss cancels the rename draft");
            println!("GTK selected file click/rename/stem selection smoke test passed");
            let folder_index = ui.sidebars[0]
                .sidebar_rows
                .borrow()
                .iter()
                .position(|row| matches!(row, SidebarRow::Directory(path) if path == "notes"))
                .unwrap();
            ui.sidebars[0]
                .file_selection
                .set_selected(folder_index as u32);
            assert_eq!(
                ui.sidebars[0].file_selection.selected(),
                folder_index as u32
            );
            let folder_row = ui.sidebars[0].interactions.row_widget("notes").unwrap();
            let folder_controllers = folder_row.observe_controllers();
            let folder_click = (0..folder_controllers.n_items())
                .filter_map(|index| folder_controllers.item(index))
                .filter_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
                .find(|gesture| gesture.name().as_deref() == Some("sidebar-entry-click"))
                .unwrap();
            folder_click.emit_by_name::<()>("pressed", &[&1i32, &24f64, &0f64]);
            assert_eq!(
                ui.sidebars[0]
                    .interactions
                    .editor("notes")
                    .unwrap()
                    .parent()
                    .unwrap()
                    .downcast::<gtk::Stack>()
                    .unwrap()
                    .visible_child_name()
                    .as_deref(),
                Some("label")
            );
            ui.sidebars[0].interactions.begin_rename("notes");
            let entry = ui.sidebars[0]
                .interactions
                .editor("notes")
                .expect("inline rename entry");
            assert_eq!(
                entry
                    .parent()
                    .unwrap()
                    .downcast::<gtk::Stack>()
                    .unwrap()
                    .visible_child_name()
                    .as_deref(),
                Some("edit")
            );
            entry.set_text("renamed");
            entry.emit_activate();
            let active = ui.snapshot.borrow().active_document.clone().unwrap();
            assert_eq!(active.id.as_str(), "renamed/todo.md");
            assert_eq!(active.content, "unsaved rename");
            assert!(active.dirty);
            ui.dispatch(AppCommand::SaveDocument(active.id));
            assert_eq!(
                std::fs::read_to_string(workspace_path.join("renamed/todo.md")).unwrap(),
                "unsaved rename"
            );
            println!("GTK rename/buffer smoke test passed");
            ui.create_entry("destination", true);
            ui.buffer.set_text("unsaved move");
            let delayed = std::env::var_os("NATIVE_WORKSPACE_SMOKE_DROP_DELAY").is_some();
            #[cfg(debug_assertions)]
            if delayed {
                ui.core
                    .borrow_mut()
                    .delay_next_drop_for_smoke(std::time::Duration::from_secs(1));
            }
            let heartbeats = Rc::new(Cell::new(0));
            let heartbeat_counter = heartbeats.clone();
            let heartbeat =
                glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                    heartbeat_counter.set(heartbeat_counter.get() + 1);
                    glib::ControlFlow::Continue
                });
            let released_at = std::time::Instant::now();
            ui.sidebars[0].interactions.start_drag("renamed");
            assert!(ui.sidebars[0].interactions.hover("destination", true));
            assert!(
                ui.sidebars[0]
                    .interactions
                    .drop_entry("renamed", "destination")
            );
            assert!(ui.sidebars[0].interactions.commit_drop());
            let active = ui.snapshot.borrow().active_document.clone().unwrap();
            assert!(
                released_at.elapsed() < std::time::Duration::from_millis(250),
                "GTK release blocked the UI thread"
            );
            assert_eq!(active.id.as_str(), "destination/renamed/todo.md");
            assert_eq!(active.content, "unsaved move");
            assert!(active.dirty);
            ui.dispatch(AppCommand::MoveEntry {
                id: active.id,
                directory: String::new(),
            });
            ui.dispatch(AppCommand::SaveDocument(
                DocumentId::parse("todo.md").unwrap(),
            ));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while ui.core.borrow().has_pending_move() && std::time::Instant::now() < deadline {
                let result = ui
                    .core
                    .borrow_mut()
                    .dispatch_if_changed(AppCommand::PollSearchResults)
                    .unwrap();
                if let Some(snapshot) = result {
                    ui.render(snapshot);
                }
                let context = glib::MainContext::default();
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            heartbeat.remove();
            if delayed {
                assert!(released_at.elapsed() >= std::time::Duration::from_millis(950));
                assert!(
                    heartbeats.get() >= 30,
                    "GTK timers froze during pending move"
                );
                println!("GTK slow backend UI heartbeat ticks={}", heartbeats.get());
            }
            assert!(!ui.core.borrow().has_pending_move());
            assert_eq!(
                std::fs::read_to_string(workspace_path.join("todo.md")).unwrap(),
                "unsaved move"
            );
            println!("GTK drag-and-drop/buffer smoke test passed");
            println!("GTK workspace edit/save smoke test passed");
            for name in ["tab-review-a.txt", "tab-review-b.txt"] {
                ui.dispatch(AppCommand::CreateEntry {
                    id: DocumentId::parse(name).unwrap(),
                    directory: false,
                });
                ui.dispatch(AppCommand::OpenDocument(DocumentId::parse(name).unwrap()));
            }
            let active_page = ui
                .document_tabs
                .selected_page()
                .expect("selected native document page");
            assert_eq!(active_page.title(), "tab-review-b.txt");
            ui.document_tabs.set_selected_page(&active_page);
            assert_eq!(ui.document_tabs.selected_page(), Some(active_page.clone()));
            ui.document_tabs.reorder_page(&active_page, 0);
            assert_eq!(
                ui.snapshot.borrow().open_documents[0].as_str(),
                "tab-review-b.txt"
            );
            println!("GTK native document tabs selection/reordering smoke test passed");
            let clean_indicator = active_page
                .indicator_icon()
                .expect("reserved native indicator slot");
            ui.buffer.set_text("edited");
            assert_eq!(
                active_page.title(),
                "tab-review-b.txt",
                "Dirty marker changed the native tab title"
            );
            assert_ne!(active_page.indicator_icon(), Some(clean_indicator));
            ui.dispatch(AppCommand::SaveAllDocuments);
            ui.dispatch(AppCommand::OpenDocument(
                DocumentId::parse("tab-review-a.txt").unwrap(),
            ));
            ui.buffer.begin_user_action();
            ui.buffer.insert(&mut ui.buffer.end_iter(), "edited");
            ui.buffer.end_user_action();
            assert!(
                ui.buffer.can_undo(),
                "Fixture must contain an undoable edit"
            );
            ui.dispatch(AppCommand::OpenDocument(
                DocumentId::parse("tab-review-b.txt").unwrap(),
            ));
            assert!(
                !ui.buffer.can_undo(),
                "Undo history leaked between equal document buffers"
            );
            ui.buffer.begin_user_action();
            ui.buffer.insert(&mut ui.buffer.end_iter(), "!");
            ui.buffer.end_user_action();
            ui.dispatch(AppCommand::RenameEntry {
                id: DocumentId::parse("tab-review-b.txt").unwrap(),
                new_name: "tab-review-renamed.txt".into(),
            });
            assert!(
                ui.buffer.can_undo(),
                "Rename cleared the current document's undo history"
            );
            println!("GTK document tab undo isolation/rename smoke test passed");
            let settle = || {
                let until = std::time::Instant::now() + std::time::Duration::from_millis(80);
                while std::time::Instant::now() < until {
                    glib::MainContext::default().iteration(false);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            let font_before = ui.editor.pango_context().font_description().unwrap().size();
            let content_before =
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false);
            ui.buffer
                .select_range(&ui.buffer.iter_at_offset(1), &ui.buffer.iter_at_offset(3));
            let selection_before = ui
                .buffer
                .selection_bounds()
                .map(|(start, end)| (start.offset(), end.offset()));
            increase_text_size.emit_clicked();
            settle();
            assert!(
                ui.editor.pango_context().font_description().unwrap().size() > font_before,
                "Increase Text Size did not change the editor font"
            );
            assert_eq!(
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
                content_before
            );
            assert_eq!(
                ui.buffer
                    .selection_bounds()
                    .map(|(start, end)| (start.offset(), end.offset())),
                selection_before
            );
            assert!(ui.buffer.can_undo(), "Changing text size cleared undo");
            decrease_text_size.emit_clicked();
            settle();
            assert!(
                (ui.editor.pango_context().font_description().unwrap().size() - font_before).abs()
                    <= 1,
                "Decrease Text Size did not restore the editor font"
            );
            println!("GTK text size/content/selection/undo smoke test passed");
            let active_id = ui
                .snapshot
                .borrow()
                .active_document
                .as_ref()
                .unwrap()
                .id
                .clone();
            let readme = DocumentId::parse("README.md").unwrap();
            ui.dispatch(AppCommand::OpenDocument(readme.clone()));
            ui.dispatch(AppCommand::OpenDocument(active_id));
            let readme_indicator = ui.sidebars[0]
                .pending_rows
                .borrow()
                .iter()
                .filter(|row| row.path == "README.md")
                .find_map(|row| row.open_indicator.upgrade())
                .expect("visible README sidebar row");
            assert!(
                readme_indicator.is_visible(),
                "Inactive open file is missing its editor indicator"
            );
            ui.dispatch(AppCommand::CloseDocument(readme));
            assert!(
                !readme_indicator.is_visible(),
                "Closed file retained its editor indicator"
            );
            println!("GTK sidebar open/closed file indicator smoke test passed");
            if std::env::var_os("NATIVE_WORKSPACE_SMOKE_TAB_SCROLL").is_some() {
                for index in 0..20 {
                    let id = DocumentId::parse(format!("scroll-tab-{index:02}.rs")).unwrap();
                    ui.dispatch(AppCommand::CreateEntry {
                        id: id.clone(),
                        directory: false,
                    });
                    ui.dispatch(AppCommand::OpenDocument(id));
                }
                let settle = || {
                    let deadline =
                        std::time::Instant::now() + std::time::Duration::from_millis(400);
                    while std::time::Instant::now() < deadline {
                        glib::MainContext::default().iteration(false);
                    }
                };
                settle();
                let scroll = &ui.document_tab_scroll;
                let adjustment = scroll.adjustment().expect("overflowing native tab bar");
                let position = (adjustment.upper() - adjustment.page_size()) * 0.35;
                adjustment.set_value(position);
                for _ in 0..4 {
                    let snapshot = ui.core.borrow().snapshot();
                    ui.render(snapshot);
                    settle();
                    assert!(
                        (adjustment.value() - position).abs() < 1.0,
                        "Idle snapshots snapped the horizontal tab scroll back: {} vs {position}",
                        adjustment.value()
                    );
                }
                ui.buffer.begin_user_action();
                ui.buffer.insert(&mut ui.buffer.end_iter(), "// dirty");
                ui.buffer.end_user_action();
                settle();
                assert!(
                    (adjustment.value() - position).abs() < 1.0,
                    "Dirty marker update snapped the horizontal tab scroll back"
                );
                println!(
                    "GTK overflowing tabs retain manual scroll during idle snapshots and editing"
                );
                let selected = ui.document_tabs.selected_page();
                let lower = adjustment.lower();
                let upper = adjustment.upper() - adjustment.page_size();
                for (start, delta, boundary) in
                    [(upper - 5.0, 40.0, upper), (lower + 5.0, -40.0, lower)]
                {
                    adjustment.set_value(start);
                    scroll.begin();
                    for _ in 0..10 {
                        assert_eq!(
                            scroll.scroll(gtk::gdk::ScrollUnit::Surface, delta, 0.2),
                            glib::Propagation::Stop
                        );
                    }
                    scroll.decelerate(delta, 0.2);
                    settle();
                    assert!(
                        (adjustment.value() - boundary).abs() < 1.0,
                        "Touchpad scrolling rebounded from its boundary: {} vs {boundary}",
                        adjustment.value()
                    );
                    assert_eq!(ui.document_tabs.selected_page(), selected);
                    // Reversing immediately must leave the boundary without
                    // having to consume accumulated overshoot first.
                    scroll.scroll(gtk::gdk::ScrollUnit::Surface, -delta, 0.2);
                    assert!((adjustment.value() - (boundary - delta)).abs() < 1.0);
                }
                adjustment.set_value(position);
                scroll.begin();
                scroll.scroll(gtk::gdk::ScrollUnit::Surface, 0.1, 0.3);
                scroll.scroll(gtk::gdk::ScrollUnit::Surface, 12.0, 0.2);
                assert!(
                    (adjustment.value() - position - 12.1).abs() < 0.01,
                    "Small transverse finger movement changed the scroll axis or amplified its distance"
                );
                scroll.decelerate(2.0, 0.0);
                settle();
                assert!(adjustment.value() > position + 12.1 && adjustment.value() <= upper);
                scroll.begin();
                assert_eq!(
                    scroll.scroll(gtk::gdk::ScrollUnit::Wheel, 1.0, 0.0),
                    glib::Propagation::Proceed
                );
                println!(
                    "GTK touchpad boundary/reverse/axis lock/kinetic scrolling smoke test passed"
                );
            }
            if let Some(capture) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_SCREENSHOT") {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
                while std::time::Instant::now() < deadline {
                    glib::MainContext::default().iteration(false);
                }
                let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(
                    &snapshot,
                    ui.window.width() as f64,
                    ui.window.height() as f64,
                );
                let node = snapshot.to_node().expect("window render node");
                let texture = ui
                    .window
                    .renderer()
                    .expect("window renderer")
                    .render_texture(&node, None);
                texture
                    .save_to_png(PathBuf::from(capture))
                    .expect("native tabs screenshot");
            }
        }
        if std::env::var_os("NATIVE_WORKSPACE_SMOKE_RECORD_DROP").is_some() {
            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || app.quit());
        } else {
            app.quit();
        }
    }
}

#[cfg(test)]
mod sidebar_order_tests {
    use super::order_sidebar_paths;
    use std::collections::HashSet;

    #[test]
    fn directories_precede_files_at_each_level() {
        let entries = vec![
            ("a.txt".to_owned(), false),
            ("z/b.txt".to_owned(), false),
            ("B.txt".to_owned(), false),
            ("lib/deep/only.txt".to_owned(), false),
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
        let ordered = order_sidebar_paths(entries, &directories);
        let paths: Vec<_> = ordered.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "z",
                "z/x",
                "z/x/nested.txt",
                "z/b.txt",
                "z/child.txt",
                "a.txt",
                "B.txt",
                "lib/deep/only.txt"
            ]
        );
    }
}
