//! Search sidebar widgets. Matching and edit preparation are core commands.
use crate::ui::{SidebarRow, Ui};
use adw::prelude::*;
use native_workspace_application::{AppCommand, AppSnapshot, SearchOptions};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Clone)]
pub(super) struct SearchControls {
    pub root: gtk::Box,
    pub(super) case: gtk::ToggleButton,
    pub(super) word: gtk::ToggleButton,
    pub(super) regex: gtk::ToggleButton,
    include: gtk::Entry,
    exclude: gtk::Entry,
    ignore: gtk::CheckButton,
    open: gtk::CheckButton,
    pub(super) replacement: gtk::Entry,
    pub(super) replace_expander: gtk::Expander,
    pub(super) preserve: gtk::ToggleButton,
    pub(super) all: gtk::Button,
    file: gtk::Button,
    occurrence: gtk::Button,
    refresh: gtk::Button,
    cancel: gtk::Button,
    clear: gtk::Button,
    status: gtk::Label,
    pub(super) preview: Rc<RefCell<Option<Preview>>>,
}

pub(super) struct Preview {
    pub(super) window: gtk::Window,
    pub(super) apply: gtk::Button,
    error: gtk::Label,
    heading: gtk::Label,
    buffer: gtk::TextBuffer,
    shown: Arc<native_workspace_application::ReplacementPreview>,
}

fn toggle(label: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::with_label(label);
    button.set_tooltip_text(Some(tooltip));
    button
}

impl SearchControls {
    pub fn new(search: &gtk::SearchEntry, spinner: &gtk::Spinner) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_bottom(6);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        search.set_hexpand(true);
        row.append(search);
        row.append(spinner);
        root.append(&row);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        let case = toggle("Aa", "Match Case");
        let word = toggle("ab", "Match Whole Word");
        let regex = toggle(".*", "Use Regular Expression");
        for button in [&case, &word, &regex] {
            options.append(button);
        }
        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh Search"));
        let cancel = gtk::Button::from_icon_name("process-stop-symbolic");
        cancel.set_tooltip_text(Some("Cancel Search"));
        let clear = gtk::Button::from_icon_name("edit-clear-symbolic");
        clear.set_tooltip_text(Some("Clear Search"));
        for button in [&refresh, &cancel, &clear] {
            options.append(button);
        }
        root.append(&options);
        let replacement = gtk::Entry::new();
        replacement.set_placeholder_text(Some("Replace"));
        replacement.set_hexpand(true);
        let preserve = toggle("AB", "Preserve Case");
        let replace_box = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        replace_row.append(&replacement);
        replace_row.append(&preserve);
        replace_box.append(&replace_row);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        let occurrence = gtk::Button::with_label("Match…");
        occurrence.set_tooltip_text(Some("Replace selected match"));
        let file = gtk::Button::with_label("File…");
        file.set_tooltip_text(Some("Replace all in selected file"));
        let all = gtk::Button::with_label("All…");
        all.set_tooltip_text(Some("Replace All"));
        for button in [&occurrence, &file, &all] {
            actions.append(button);
        }
        replace_box.append(&actions);
        let replace = gtk::Expander::builder()
            .label("Replace")
            .child(&replace_box)
            .build();
        root.append(&replace);
        let details = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let include = gtk::Entry::new();
        include.set_placeholder_text(Some("files to include: *.rs, src/**"));
        let exclude = gtk::Entry::new();
        exclude.set_placeholder_text(Some("files to exclude: target, **/*.lock"));
        let open = gtk::CheckButton::with_label("Search only in Open Editors");
        let ignore = gtk::CheckButton::with_label("Use Ignore Files");
        ignore.set_active(true);
        details.append(&include);
        details.append(&open);
        details.append(&exclude);
        details.append(&ignore);
        root.append(
            &gtk::Expander::builder()
                .label("Search details")
                .child(&details)
                .build(),
        );
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.add_css_class("caption");
        root.append(&status);
        Self {
            root,
            case,
            word,
            regex,
            include,
            exclude,
            ignore,
            open,
            replacement,
            replace_expander: replace,
            preserve,
            all,
            file,
            occurrence,
            refresh,
            cancel,
            clear,
            status,
            preview: Rc::default(),
        }
    }

    pub fn options(&self) -> SearchOptions {
        SearchOptions {
            match_case: self.case.is_active(),
            whole_word: self.word.is_active(),
            regex: self.regex.is_active(),
            include: self.include.text().into(),
            exclude: self.exclude.text().into(),
            use_ignore_files: self.ignore.is_active(),
            only_open_editors: self.open.is_active(),
        }
    }

    pub fn connect(&self, ui: &Ui) {
        for toggle in [&self.case, &self.word, &self.regex] {
            let ui = ui.clone();
            toggle.connect_toggled(move |_| {
                if !ui.updating.get() {
                    ui.dispatch(AppCommand::ConfigureSearch(ui.search_controls.options()));
                }
            });
        }
        for toggle in [&self.ignore, &self.open] {
            let ui = ui.clone();
            toggle.connect_toggled(move |_| {
                if !ui.updating.get() {
                    ui.dispatch(AppCommand::ConfigureSearch(ui.search_controls.options()));
                }
            });
        }
        for field in [&self.include, &self.exclude] {
            let ui = ui.clone();
            field.connect_changed(move |_| {
                if !ui.updating.get() {
                    ui.dispatch(AppCommand::ConfigureSearch(ui.search_controls.options()));
                }
            });
        }
        for (button, command) in [
            (&self.refresh, AppCommand::RefreshSearch),
            (&self.cancel, AppCommand::CancelSearch),
            (&self.clear, AppCommand::QueueSearchQuery(String::new())),
        ] {
            let ui = ui.clone();
            button.connect_clicked(move |_| ui.dispatch(command.clone()));
        }
        for (button, mode) in [(&self.all, 0), (&self.file, 1), (&self.occurrence, 2)] {
            let ui = ui.clone();
            button.connect_clicked(move |_| {
                let row = ui
                    .sidebar()
                    .sidebar_rows
                    .borrow()
                    .get(ui.sidebar().file_selection.selected() as usize)
                    .cloned();
                let target = match (mode, row) {
                    (0, _) => Some(native_workspace_application::ReplacementTarget::All),
                    (1, Some(SidebarRow::File(id))) => Some(
                        native_workspace_application::ReplacementTarget::Document(id),
                    ),
                    (1, Some(SidebarRow::Match(result))) => Some(
                        native_workspace_application::ReplacementTarget::Document(result.id),
                    ),
                    (2, Some(SidebarRow::Match(result))) => {
                        Some(native_workspace_application::ReplacementTarget::Match {
                            id: result.id,
                            location: result.location,
                        })
                    }
                    _ => None,
                };
                if let Some(target) = target {
                    ui.dispatch(AppCommand::PrepareReplacement {
                        replacement: ui.search_controls.replacement.text().into(),
                        preserve_case: ui.search_controls.preserve.is_active(),
                        target,
                    });
                }
            });
        }
    }

    pub fn render(&self, ui: &Ui, snapshot: &AppSnapshot) {
        let options = &snapshot.search_options;
        self.case.set_active(options.match_case);
        self.word.set_active(options.whole_word);
        self.regex.set_active(options.regex);
        self.ignore.set_active(options.use_ignore_files);
        self.open.set_active(options.only_open_editors);
        if self.include.text().as_str() != options.include {
            self.include.set_text(&options.include);
        }
        if self.exclude.text().as_str() != options.exclude {
            self.exclude.set_text(&options.exclude);
        }
        self.cancel.set_visible(snapshot.search_in_progress);
        let ready = !snapshot.search_in_progress
            && !snapshot.search_truncated
            && !snapshot.search_results.is_empty()
            && !snapshot.activity.busy;
        for button in [&self.all, &self.file, &self.occurrence] {
            button.set_sensitive(ready);
        }
        let files = snapshot
            .search_results
            .iter()
            .map(|result| &result.id)
            .collect::<std::collections::HashSet<_>>()
            .len();
        self.status
            .set_label(snapshot.search_error.as_deref().unwrap_or(&format!(
                "{} results in {files} files",
                snapshot.search_results.len()
            )));
        if snapshot.search_error.is_some() {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
        let Some(preview) = &snapshot.replacement_preview else {
            if let Some(dialog) = self.preview.borrow_mut().take() {
                dialog.window.destroy();
            }
            return;
        };
        if self.preview.borrow().is_none() {
            let window = gtk::Window::builder()
                .title("Replace Preview")
                .transient_for(&ui.window)
                .modal(true)
                .default_width(640)
                .default_height(480)
                .build();
            let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
            content.set_margin_start(16);
            content.set_margin_end(16);
            content.set_margin_top(16);
            content.set_margin_bottom(16);
            let prompt = native_workspace_application::replacement_confirmation(
                preview.occurrences,
                preview.file_count,
            );
            let heading = gtk::Label::new(Some(&prompt.title));
            content.append(&heading);
            let note = gtk::Label::new(Some(prompt.body));
            note.set_wrap(true);
            content.append(&note);
            let text = gtk::TextView::builder()
                .editable(false)
                .monospace(true)
                .build();
            text.buffer().set_text(&preview.text);
            let scroll = gtk::ScrolledWindow::builder()
                .vexpand(true)
                .child(&text)
                .build();
            content.append(&scroll);
            let error = gtk::Label::new(None);
            error.set_wrap(true);
            error.add_css_class("error");
            content.append(&error);
            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let cancel = gtk::Button::with_label(prompt.cancel);
            let apply = gtk::Button::with_label(prompt.confirm);
            apply.add_css_class("suggested-action");
            actions.append(&cancel);
            actions.append(&apply);
            content.append(&actions);
            window.set_child(Some(&content));
            let cancel_ui = ui.clone();
            cancel.connect_clicked(move |_| cancel_ui.dispatch(AppCommand::DiscardReplacement));
            let apply_ui = ui.clone();
            apply.connect_clicked(move |_| apply_ui.dispatch(AppCommand::ApplyReplacement));
            let close_ui = ui.clone();
            window.connect_close_request(move |_| {
                close_ui.dispatch(AppCommand::DiscardReplacement);
                gtk::glib::Propagation::Stop
            });
            window.present();
            self.preview.replace(Some(Preview {
                window,
                apply,
                error,
                heading,
                buffer: text.buffer(),
                shown: preview.clone(),
            }));
        }
        if let Some(dialog) = self.preview.borrow_mut().as_mut() {
            if !Arc::ptr_eq(&dialog.shown, preview) {
                let prompt = native_workspace_application::replacement_confirmation(
                    preview.occurrences,
                    preview.file_count,
                );
                dialog.heading.set_label(&prompt.title);
                dialog.buffer.set_text(&preview.text);
                dialog.shown = preview.clone();
            }
            dialog.apply.set_sensitive(!snapshot.activity.busy);
            dialog
                .error
                .set_label(snapshot.error.as_deref().unwrap_or_default());
        }
    }
}
