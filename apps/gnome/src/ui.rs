//! GTK presentation controller. Editor behavior belongs to Session.
use crate::document_tabs::style_preview_tab;
use crate::sidebar::{
    document_tab_icon, document_tab_indicator_icon, folder_icon_for, sidebar_entry, tree_entries,
};
use crate::{document_tab_scroll, editor_chrome, sidebar_interactions, syntax};
use adw::prelude::*;
use gtk::glib;
use native_workspace_application::{
    AppCommand, AppError, AppSnapshot, SearchLocation, SearchResult, Session, close_confirmation,
    delete_confirmation,
};
use native_workspace_domain::{DocumentId, path};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarTab {
    Files,
    Search,
    Git,
    Bookmarks,
    Warnings,
    Errors,
}
impl SidebarTab {
    pub(super) const ALL: [Self; 6] = [
        Self::Files,
        Self::Search,
        Self::Git,
        Self::Bookmarks,
        Self::Warnings,
        Self::Errors,
    ];
    pub(super) fn has_tree(self) -> bool {
        matches!(self, Self::Files | Self::Search)
    }
}

#[derive(Clone)]
pub(super) struct Ui {
    pub(super) window: adw::ApplicationWindow,
    pub(super) core: Rc<RefCell<Session>>,
    pub(super) snapshot: Rc<RefCell<AppSnapshot>>,
    // Keep Files and Search models, selections and scroll positions alive across tab switches.
    pub(super) sidebars: [SidebarView; 2],
    pub(super) sidebar_pages: gtk::Stack,
    pub(super) search: gtk::SearchEntry,
    pub(super) file_search: gtk::SearchEntry,
    pub(super) sidebar_tab: Rc<Cell<SidebarTab>>,
    /// The name filter the Files tree shows, i.e. of `file_filter`. Typed
    /// text reaches it only after `path_filter` has listed the matches.
    pub(super) file_query: Rc<RefCell<String>>,
    pub(super) file_search_row: gtk::Box,
    pub(super) text_search_row: gtk::Box,
    pub(super) search_spinner: gtk::Spinner,
    pub(super) search_truncated: gtk::Label,
    pub(super) conflict_banner: gtk::Box,
    pub(super) document_tabs: adw::TabView,
    pub(super) document_tab_bar: adw::TabBar,
    pub(super) document_tab_scroll: Rc<document_tab_scroll::TouchpadScroll>,
    pub(super) document_pages: Rc<RefCell<Vec<(DocumentId, adw::TabPage)>>>,
    pub(super) editor_panel: gtk::Box,
    pub(super) editor_stack: gtk::Stack,
    pub(super) empty_editor_page: gtk::Box,
    pub(super) editor: gtk::TextView,
    pub(super) editor_chrome: editor_chrome::EditorChrome,
    pub(super) buffer: gtk::TextBuffer,
    pub(super) heading: gtk::Label,
    pub(super) status: gtk::Label,
    pub(super) home_window: adw::ApplicationWindow,
    pub(super) home_error: gtk::Label,
    pub(super) resume: gtk::Button,
    pub(super) recent_list: gtk::Box,
    pub(super) recent_empty: gtk::Label,
    pub(super) updating: Rc<Cell<bool>>,
    pub(super) pending_move_target: Rc<RefCell<Option<String>>>,
    pub(super) drop_trace: Rc<RefCell<Option<native_workspace_application::SidebarDropTrace>>>,
    pub(super) collapsed_directories: Rc<RefCell<std::collections::HashSet<String>>>,
}

#[derive(Clone)]
pub(super) struct SidebarView {
    pub(super) interactions: Rc<sidebar_interactions::Interactions>,
    pub(super) files: gtk::ListView,
    pub(super) file_model: gtk::StringList,
    pub(super) file_selection: gtk::SingleSelection,
    pub(super) sidebar_rows: Rc<RefCell<Vec<SidebarRow>>>,
    pub(super) all_entries: Rc<RefCell<Vec<SidebarEntry>>>,
    pub(super) visible_entries: Rc<RefCell<Vec<SidebarEntry>>>,
    pub(super) files_placeholder: gtk::Label,
    pub(super) overlay: gtk::Overlay,
    pub(super) scroll: gtk::ScrolledWindow,
    pub(super) dirty: Rc<Cell<bool>>,
    pub(super) pending_rows: Rc<RefCell<Vec<PendingRow>>>,
    pub(super) open_documents: Rc<RefCell<std::collections::HashSet<String>>>,
}

pub(super) struct PendingRow {
    pub(super) path: String,
    pub(super) row: glib::WeakRef<gtk::Box>,
    pub(super) spinner: glib::WeakRef<gtk::Spinner>,
    pub(super) open_indicator: glib::WeakRef<gtk::Image>,
    pub(super) is_file: bool,
}

pub(super) fn update_pending_row(
    row: &gtk::Box,
    spinner: &gtk::Spinner,
    path: &str,
    target: Option<&str>,
) {
    let pending = target.is_some_and(|target| path::is_within(path, target));
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
pub(super) enum SidebarRow {
    Directory(String),
    File(DocumentId),
    Match(SearchResult),
}

#[derive(Clone)]
pub(super) struct SidebarEntry {
    pub(super) row: SidebarRow,
    pub(super) path: String,
    pub(super) title: String,
    pub(super) depth: usize,
    pub(super) suffix: Option<&'static str>,
}

impl SidebarRow {
    pub(super) fn id(&self) -> Option<&DocumentId> {
        match self {
            Self::Directory(_) => None,
            Self::File(id) => Some(id),
            Self::Match(result) => Some(&result.id),
        }
    }

    pub(super) fn location(&self) -> Option<&SearchLocation> {
        match self {
            Self::Directory(_) | Self::File(_) => None,
            Self::Match(result) => Some(&result.location),
        }
    }
}

impl Ui {
    pub(super) fn create_entry(&self, name: &str, directory: bool) {
        let Ok(id) = DocumentId::parse(name) else {
            self.core.borrow_mut().report(AppError::InvalidEntryName);
            self.render_current();
            return;
        };
        let success = self
            .core
            .borrow_mut()
            .dispatch(AppCommand::CreateEntry {
                id: id.clone(),
                directory,
            })
            .is_ok();
        self.render_current();
        if success && !directory {
            self.dispatch(AppCommand::OpenDocument(id));
        }
    }

    pub(super) fn dispatch(&self, command: AppCommand) {
        let _ = self.core.borrow_mut().dispatch(command);
        self.render_current();
    }

    pub(super) fn render_current(&self) {
        let snapshot = self.core.borrow().snapshot();
        self.render(snapshot);
    }

    pub(super) fn queue_file_filter(&self, query: &str) {
        self.core.borrow_mut().filter_files(query);
        self.render_current();
    }

    pub(super) fn poll_core(&self) {
        self.core.borrow_mut().poll();
        if self.core.borrow().revision() != self.snapshot.borrow().revision {
            self.render_current();
        }
    }

    pub(super) fn request_close_document(&self, id: DocumentId) {
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
        let prompt = close_confirmation(id.as_str());
        let dialog = adw::AlertDialog::builder()
            .heading(&prompt.title)
            .body(prompt.body)
            .build();
        dialog.add_responses(&[
            ("cancel", prompt.cancel),
            ("discard", prompt.discard),
            ("save", prompt.confirm),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let root = self.snapshot.borrow().workspace.clone();
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if ui.snapshot.borrow().workspace != root {
                return;
            }
            if response == "discard" {
                ui.dispatch(AppCommand::DiscardAndCloseDocument(id.clone()));
            }
            if response == "save" {
                ui.dispatch(AppCommand::SaveAndCloseDocument(id.clone()));
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn confirm_delete(&self, path: String) {
        if self.core.borrow().snapshot().activity.busy {
            return;
        }
        let prompt = delete_confirmation(&path);
        let dialog = adw::AlertDialog::builder()
            .heading(&prompt.title)
            .body(prompt.body)
            .build();
        dialog.add_responses(&[("cancel", prompt.cancel), ("delete", prompt.confirm)]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let requested_root = self.snapshot.borrow().workspace.clone();
        let ui = self.clone();
        dialog.connect_response(Some("delete"), move |_, _| {
            if ui.snapshot.borrow().workspace != requested_root {
                return;
            }
            if let Ok(id) = DocumentId::parse(path.clone()) {
                ui.dispatch(AppCommand::DeleteEntry(id));
                ui.sync_pending_move();
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn sync_pending_move(&self) {
        self.window
            .set_sensitive(!self.core.borrow().snapshot().activity.deleting);
        self.editor.set_editable(
            self.snapshot.borrow().active_document.is_some()
                && !self.core.borrow().snapshot().activity.deleting,
        );
        let target = self
            .core
            .borrow()
            .snapshot()
            .activity
            .relocation
            .filter(|_| self.core.borrow().snapshot().activity.relocation_slow)
            .map(|relocation| relocation.target.to_string());
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

    pub(super) fn render_document_tabs(&self, snapshot: &AppSnapshot) {
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
        style_preview_tab(
            &self.document_tab_bar.clone().upcast(),
            &pages,
            snapshot.preview_document.as_ref(),
        );
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

    pub(super) fn render(&self, snapshot: AppSnapshot) {
        self.sync_pending_move();
        let diagnostic_started = self
            .core
            .borrow()
            .diagnostic_observer()
            .enabled()
            .then(std::time::Instant::now);
        let diagnostic_operation = snapshot.diagnostic_operation;
        let diagnostic_revision = snapshot.revision;
        if self.file_query.borrow().as_str() != snapshot.file_filter.query.trim() {
            self.file_query
                .replace(snapshot.file_filter.query.trim().to_owned());
            self.sidebars[0].dirty.set(true);
        }
        if self.snapshot.borrow().file_filter != snapshot.file_filter {
            self.sidebars[0].dirty.set(true);
        }
        self.updating.set(true);
        self.home_error
            .set_label(snapshot.error.as_deref().unwrap_or_default());
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
            if previous.workspace != snapshot.workspace
                || previous.documents != snapshot.documents
                || previous.directories != snapshot.directories
            {
                for sidebar in &self.sidebars {
                    sidebar.dirty.set(true);
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
            if previous.workspace != snapshot.workspace {
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
                || previous.preview_document != snapshot.preview_document
                || previous.active_document.as_ref().map(|doc| &doc.id)
                    != snapshot.active_document.as_ref().map(|doc| &doc.id)
                || previous.documents != snapshot.documents
        };
        if tabs_changed {
            self.render_document_tabs(&snapshot);
        }
        let recent_changed = self.snapshot.borrow().recent_workspaces != snapshot.recent_workspaces;
        let workspace_changed = self.snapshot.borrow().workspace != snapshot.workspace;
        if workspace_changed {
            // Every folder starts collapsed again; rebind all rows.
            for sidebar in &self.sidebars {
                sidebar.visible_entries.borrow_mut().clear();
                sidebar
                    .file_model
                    .splice(0, sidebar.file_model.n_items(), &[]);
            }
        }
        if workspace_changed && snapshot.workspace.is_some() {
            self.window.present();
            self.home_window.set_visible(false);
        }
        self.window.set_title(Some(
            snapshot.workspace_name().unwrap_or("Native Workspace"),
        ));
        self.heading
            .set_label(snapshot.workspace_name().unwrap_or("Open a workspace"));
        if self.search.text().as_str() != snapshot.search_query {
            self.search.set_text(&snapshot.search_query);
        }
        self.search_spinner
            .set_spinning(snapshot.search_in_progress);
        self.search_spinner.set_visible(
            self.sidebar_tab.get() == SidebarTab::Search && snapshot.search_in_progress,
        );
        self.search_truncated
            .set_visible(self.sidebar_tab.get() == SidebarTab::Search && snapshot.search_truncated);
        self.conflict_banner.set_visible(
            snapshot
                .active_document
                .as_ref()
                .is_some_and(|document| document.external_change),
        );
        self.editor.set_editable(
            snapshot.active_document.is_some() && !self.core.borrow().snapshot().activity.deleting,
        );
        self.status
            .set_label(match snapshot.active_document.as_ref() {
                Some(doc) if doc.dirty => "Modified",
                Some(_) => "Saved",
                None => "Select a document",
            });
        if self.core.borrow().snapshot().activity.deleting {
            self.status.set_label("Deleting…");
        } else if let Some(error) = snapshot.error.as_deref() {
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
        self.resume.set_visible(snapshot.workspace.is_some());
        self.recent_empty
            .set_visible(snapshot.recent_workspaces.is_empty());
        self.render_sidebar(&snapshot);
        let content = snapshot
            .active_document
            .as_ref()
            .map(|doc| doc.content.as_ref())
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
        let switched_document = previous.editor_generation != snapshot.editor_generation;
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
            self.core.borrow().diagnostic_observer().snapshot_applied(
                diagnostic_operation,
                diagnostic_revision,
                started.elapsed().as_nanos() as u64,
            );
        }
    }

    pub(super) fn sidebar(&self) -> &SidebarView {
        &self.sidebars[usize::from(self.sidebar_tab.get() == SidebarTab::Search)]
    }

    pub(super) fn switch_sidebar(&self, tab: SidebarTab) {
        self.sidebar_tab.set(tab);
        self.file_search_row.set_visible(tab == SidebarTab::Files);
        self.text_search_row.set_visible(tab == SidebarTab::Search);
        self.search_spinner
            .set_visible(tab == SidebarTab::Search && self.snapshot.borrow().search_in_progress);
        self.search_truncated
            .set_visible(tab == SidebarTab::Search && self.snapshot.borrow().search_truncated);
        if !tab.has_tree() {
            self.sidebar_pages.set_visible_child_name("empty");
            return;
        }
        self.updating.set(true);
        self.render_sidebar(&self.snapshot.borrow());
        self.updating.set(false);
        self.sidebar_pages
            .set_visible_child_name(if tab == SidebarTab::Files {
                "files"
            } else {
                "search"
            });
    }

    pub(super) fn render_sidebar(&self, snapshot: &AppSnapshot) {
        let previous_selection = self
            .sidebar()
            .sidebar_rows
            .borrow()
            .get(self.sidebar().file_selection.selected() as usize)
            .cloned();
        let results_changed = self.sidebar().dirty.replace(false);
        self.sidebar()
            .files_placeholder
            .set_label(if snapshot.workspace_name().is_none() {
                "Open a workspace"
            } else if self.sidebar_tab.get() == SidebarTab::Search {
                if snapshot.search_query.is_empty() {
                    "Search file contents"
                } else {
                    "No text matches"
                }
            } else if !self.file_query.borrow().is_empty() {
                "No matching names"
            } else {
                "No files"
            });
        if results_changed {
            let rows = if self.sidebar_tab.get() == SidebarTab::Search {
                snapshot.search_rows()
            } else if snapshot.file_filter.is_active() {
                snapshot.file_filter_rows()
            } else {
                snapshot.file_tree_rows(|p| !self.collapsed_directories.borrow().contains(p))
            };
            let entries = rows
                .into_iter()
                .filter_map(|row| sidebar_entry(snapshot, row))
                .collect();
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
                if self.sidebar_tab.get() != SidebarTab::Search
                    && self.file_query.borrow().trim().is_empty()
                {
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

    pub(super) fn sync_sidebar_selection(&self) {
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

    pub(super) fn rebuild_visible_sidebar(&self) {
        let searching = self.sidebar_tab.get() == SidebarTab::Search
            || !self.file_query.borrow().trim().is_empty();
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
    pub(super) fn release_focus_from_rows(&self, removed: &[SidebarEntry]) {
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

    pub(super) fn toggle_directory(&self, path: &str) {
        if self.sidebar_tab.get() == SidebarTab::Search
            || !self.file_query.borrow().trim().is_empty()
        {
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

    pub(super) fn select_match(&self, location: &SearchLocation) {
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
