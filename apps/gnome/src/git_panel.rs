//! Native Git controls and graph rendering; Git policy and graph geometry are shared.
use crate::ui::Ui;
use adw::prelude::*;
use native_workspace_application::{AppCommand, AppSnapshot, GitAction, git_action_confirmation};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

struct Review {
    window: gtk::Window,
    buffer: gtk::TextBuffer,
    lines: gtk::Box,
    stage: gtk::Button,
    selected: Rc<RefCell<HashSet<usize>>>,
    shown: String,
    split: gtk::Paned,
    merge: gtk::Box,
    comparison: gtk::Paned,
    before: gtk::TextBuffer,
    after: gtk::TextBuffer,
    side_by_side: gtk::CheckButton,
}
#[derive(Clone)]
pub(super) struct GitPanel {
    pub root: gtk::Box,
    branch: gtk::ComboBoxText,
    repository: gtk::ComboBoxText,
    summary: gtk::Label,
    refresh: gtk::Button,
    actions: gtk::MenuButton,
    extras: gtk::Box,
    rows: gtk::Box,
    graph: gtk::Box,
    controls: gtk::Box,
    message: gtk::TextView,
    amend: gtk::CheckButton,
    signoff: gtk::CheckButton,
    commit: gtk::Button,
    operation: gtk::Box,
    error: gtk::Label,
    updating: Rc<Cell<bool>>,
    review: Rc<RefCell<Option<Review>>>,
}

fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(child)
        .build()
}
fn action(ui: &Ui, id: &str, value: String, extra: String, flag: bool) {
    let _ = ui.core.borrow_mut().git_request(id, value, extra, flag);
    ui.render_current();
}
fn confirm(ui: &Ui, id: &'static str, value: String) {
    let prompt = git_action_confirmation();
    let dialog = adw::AlertDialog::builder()
        .heading(&prompt.title)
        .body(prompt.body)
        .build();
    dialog.add_responses(&[("cancel", prompt.cancel), ("apply", prompt.confirm)]);
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    let next = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "apply" {
            action(&next, id, value.clone(), String::new(), false);
        }
    });
    dialog.present(Some(&ui.window));
}
fn form(ui: &Ui, spec: &GitAction) {
    let window = gtk::Window::builder()
        .title(spec.title)
        .transient_for(&ui.window)
        .modal(true)
        .default_width(440)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    let first = gtk::Entry::builder()
        .placeholder_text(spec.first)
        .visible(!spec.first.is_empty())
        .build();
    let second = gtk::Entry::builder()
        .placeholder_text(spec.second)
        .visible(!spec.second.is_empty())
        .build();
    let option = gtk::CheckButton::with_label(spec.option);
    option.set_visible(!spec.option.is_empty());
    content.append(&first);
    content.append(&second);
    content.append(&option);
    if spec.destructive {
        let label = gtk::Label::new(Some(git_action_confirmation().body));
        label.set_wrap(true);
        content.append(&label);
    }
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label(spec.title);
    apply.add_css_class(if spec.destructive {
        "destructive-action"
    } else {
        "suggested-action"
    });
    buttons.append(&cancel);
    buttons.append(&apply);
    content.append(&buttons);
    window.set_child(Some(&content));
    let close = window.clone();
    cancel.connect_clicked(move |_| close.close());
    let close = window.clone();
    let ui = ui.clone();
    let id = spec.id;
    apply.connect_clicked(move |_| {
        action(
            &ui,
            id,
            first.text().into(),
            second.text().into(),
            option.is_active(),
        );
        close.close();
    });
    window.present();
}
impl GitPanel {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_margin_start(8);
        root.set_margin_end(8);
        let paned = gtk::Paned::new(gtk::Orientation::Vertical);
        paned.set_widget_name("gitSplit");
        paned.set_vexpand(true);
        paned.set_wide_handle(true);
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(true);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.add_tick_callback(|split, _| {
            if split.height() > 0 {
                split.set_position(split.height() / 2);
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        });
        root.append(&paned);
        let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
        top.set_size_request(-1, 160);
        let repository = gtk::ComboBoxText::new();
        repository.set_visible(false);
        top.append(&repository);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let branch = gtk::ComboBoxText::new();
        branch.set_hexpand(true);
        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh Git changes"));
        let actions = gtk::MenuButton::new();
        actions.set_icon_name("view-more-symbolic");
        actions.set_tooltip_text(Some("Git Actions"));
        line.append(&branch);
        line.append(&refresh);
        line.append(&actions);
        top.append(&line);
        let summary = gtk::Label::new(None);
        summary.set_xalign(0.0);
        summary.add_css_class("dim-label");
        top.append(&summary);
        let controls = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let message = gtk::TextView::new();
        message.set_widget_name("gitCommitMessage");
        message.set_wrap_mode(gtk::WrapMode::WordChar);
        message.set_size_request(-1, 56);
        controls.append(&scroll(&message));
        let choices = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let amend = gtk::CheckButton::with_label("Amend");
        let signoff = gtk::CheckButton::with_label("Sign off");
        let commit = gtk::Button::with_label("Commit");
        commit.add_css_class("suggested-action");
        choices.append(&amend);
        choices.append(&signoff);
        choices.append(&commit);
        controls.append(&choices);
        top.append(&controls);
        let operation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        top.append(&operation);
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        top.append(&scroll(&rows));
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.add_css_class("error");
        top.append(&error);
        paned.set_start_child(Some(&top));
        let bottom = gtk::Box::new(gtk::Orientation::Vertical, 4);
        bottom.set_size_request(-1, 160);
        bottom.set_widget_name("gitGraph");
        let heading = gtk::Label::new(Some("Source Control Graph"));
        heading.set_xalign(0.0);
        heading.add_css_class("heading");
        bottom.append(&heading);
        let graph = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bottom.append(&scroll(&graph));
        paned.set_end_child(Some(&bottom));
        Self {
            root,
            branch,
            repository,
            summary,
            refresh,
            actions,
            extras: gtk::Box::new(gtk::Orientation::Vertical, 3),
            rows,
            graph,
            controls,
            message,
            amend,
            signoff,
            commit,
            operation,
            error,
            updating: Rc::default(),
            review: Rc::default(),
        }
    }
    pub fn connect(&self, ui: &Ui) {
        let next = ui.clone();
        self.refresh
            .connect_clicked(move |_| next.dispatch(AppCommand::RefreshGit));
        let next = ui.clone();
        let panel = self.clone();
        self.repository.connect_changed(move |combo| {
            if !panel.updating.get()
                && let Some(id) = combo.active_id()
            {
                action(&next, "select_repository", id.into(), String::new(), false);
            }
        });
        let next = ui.clone();
        let panel = self.clone();
        self.branch.connect_changed(move |combo| {
            if !panel.updating.get()
                && let Some(id) = combo.active_id()
            {
                action(&next, "checkout", id.into(), String::new(), false);
            }
        });
        let next = ui.clone();
        let panel = self.clone();
        self.commit.connect_clicked(move |_| {
            let buffer = panel.message.buffer();
            action(
                &next,
                "commit",
                buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .into(),
                if panel.signoff.is_active() {
                    "signoff".into()
                } else {
                    String::new()
                },
                panel.amend.is_active(),
            );
        });
        let choices = gtk::Box::new(gtk::Orientation::Vertical, 2);
        choices.append(&self.extras);
        for spec in &ui.snapshot.borrow().git.actions {
            let button = gtk::Button::with_label(spec.title);
            button.add_css_class("flat");
            let next = ui.clone();
            let spec = spec.clone();
            button.connect_clicked(move |_| form(&next, &spec));
            choices.append(&button);
        }
        let popover = gtk::Popover::new();
        let list = scroll(&choices);
        list.set_min_content_height(350);
        popover.set_child(Some(&list));
        self.actions.set_popover(Some(&popover));
    }
    fn review(&self, ui: &Ui, command: impl FnOnce(&Ui)) {
        let text = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .build();
        text.set_widget_name("gitDiffText");
        text.buffer().set_text("Loading…");
        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        split.set_start_child(Some(&scroll(&text)));
        split.set_position(650);
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let check_scroll = scroll(&lines);
        check_scroll.set_min_content_width(260);
        split.set_end_child(Some(&check_scroll));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let stage = gtk::Button::with_label("Stage Selected Lines");
        content.append(&stage);
        let side_by_side = gtk::CheckButton::with_label("Side by Side");
        side_by_side.set_active(true);
        content.append(&side_by_side);
        let comparison = gtk::Paned::new(gtk::Orientation::Horizontal);
        comparison.set_vexpand(true);
        let before = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .build();
        let after = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .build();
        comparison.set_start_child(Some(&scroll(&before)));
        comparison.set_end_child(Some(&scroll(&after)));
        content.append(&comparison);
        let compare = comparison.clone();
        let unified = split.clone();
        side_by_side.connect_toggled(move |toggle| {
            compare.set_visible(toggle.is_active());
            unified.set_visible(!toggle.is_active());
        });
        content.append(&split);
        let merge = gtk::Box::new(gtk::Orientation::Vertical, 6);
        merge.set_vexpand(true);
        merge.set_visible(false);
        content.append(&merge);
        let window = gtk::Window::builder()
            .title("Git Changes")
            .transient_for(&ui.window)
            .modal(true)
            .default_width(1000)
            .default_height(650)
            .child(&content)
            .build();
        window.set_widget_name("gitDiffDialog");
        let selected = Rc::new(RefCell::new(HashSet::new()));
        let indices = selected.clone();
        let next = ui.clone();
        let close = window.clone();
        stage.connect_clicked(move |_| {
            let _ = next
                .core
                .borrow_mut()
                .git_stage_lines(indices.borrow().iter().copied().collect());
            next.render_current();
            close.close();
        });
        self.review.replace(Some(Review {
            window: window.clone(),
            buffer: text.buffer(),
            lines,
            stage,
            selected,
            shown: String::new(),
            split,
            merge,
            comparison,
            before: before.buffer(),
            after: after.buffer(),
            side_by_side,
        }));
        let state = self.review.clone();
        window.connect_close_request(move |_| {
            state.replace(None);
            gtk::glib::Propagation::Proceed
        });
        command(ui);
        window.present();
    }
    pub fn render(&self, ui: &Ui, snapshot: &AppSnapshot) {
        let busy = snapshot.activity.busy;
        self.refresh.set_sensitive(!busy);
        self.actions.set_sensitive(!busy);
        self.branch.set_sensitive(!busy);
        self.controls.set_sensitive(!busy);
        self.rows.set_sensitive(!busy);
        self.graph.set_sensitive(!busy);
        self.error
            .set_label(snapshot.error.as_deref().unwrap_or_default());
        self.error.set_visible(snapshot.error.is_some());
        if !busy && let Some(view) = self.review.borrow_mut().as_mut() {
            let text = snapshot
                .error
                .clone()
                .or_else(|| snapshot.git.inspection.as_ref().map(|d| d.text.clone()))
                .or_else(|| snapshot.git.diff.as_ref().map(|d| d.text.clone()))
                .unwrap_or_default();
            if view.shown != text {
                let conflict = snapshot
                    .git
                    .inspection
                    .as_ref()
                    .and_then(|i| i.conflict.as_ref());
                let comparison = snapshot
                    .git
                    .diff
                    .as_ref()
                    .and_then(|d| d.before.as_ref().zip(d.after.as_ref()));
                view.side_by_side
                    .set_visible(comparison.is_some() && conflict.is_none());
                if let Some((before, after)) = comparison {
                    view.before.set_text(before);
                    view.after.set_text(after);
                } else {
                    view.side_by_side.set_active(false);
                }
                view.comparison.set_visible(
                    conflict.is_none() && comparison.is_some() && view.side_by_side.is_active(),
                );
                view.split.set_visible(
                    conflict.is_none() && (!view.side_by_side.is_active() || comparison.is_none()),
                );
                view.merge.set_visible(conflict.is_some());
                while let Some(child) = view.merge.first_child() {
                    view.merge.remove(&child);
                }
                if let Some(conflict) = conflict {
                    let sources = gtk::Paned::new(gtk::Orientation::Horizontal);
                    sources.set_size_request(-1, 180);
                    for (index, source) in [&conflict.current, &conflict.incoming]
                        .into_iter()
                        .enumerate()
                    {
                        let text = gtk::TextView::builder()
                            .editable(false)
                            .monospace(true)
                            .build();
                        text.buffer().set_text(source);
                        let scroll = scroll(&text);
                        if index == 0 {
                            sources.set_start_child(Some(&scroll));
                        } else {
                            sources.set_end_child(Some(&scroll));
                        }
                    }
                    view.merge.append(&sources);
                    let draft = gtk::TextView::builder().monospace(true).build();
                    draft.buffer().set_text(&conflict.expected);
                    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                    for (title, choice) in [
                        ("Accept Current", &conflict.accept_current),
                        ("Accept Incoming", &conflict.accept_incoming),
                        ("Accept Both", &conflict.accept_both),
                    ] {
                        let button = gtk::Button::with_label(title);
                        let buffer = draft.buffer();
                        let value = choice.clone();
                        button.connect_clicked(move |_| buffer.set_text(&value));
                        controls.append(&button);
                    }
                    let save = gtk::Button::with_label("Save and Stage Result");
                    let buffer = draft.buffer();
                    let next = ui.clone();
                    let close = view.window.clone();
                    save.connect_clicked(move |_| {
                        let _ = next.core.borrow_mut().git_resolve(
                            buffer
                                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                                .into(),
                        );
                        next.render_current();
                        close.close();
                    });
                    controls.append(&save);
                    view.merge.append(&controls);
                    view.merge.append(&scroll(&draft));
                }
                view.buffer.set_text(&text);
                view.shown = text;
                view.selected.borrow_mut().clear();
                view.window.set_title(Some(
                    snapshot
                        .git
                        .inspection
                        .as_ref()
                        .map(|d| d.title.as_str())
                        .or_else(|| snapshot.git.diff.as_ref().map(|d| d.path.as_str()))
                        .unwrap_or("Git Changes"),
                ));
                while let Some(child) = view.lines.first_child() {
                    view.lines.remove(&child);
                }
                view.stage.set_visible(snapshot.git.diff.is_some());
                view.stage
                    .set_label(if snapshot.git.diff.as_ref().is_some_and(|d| d.staged) {
                        "Unstage Selected Lines"
                    } else {
                        "Stage Selected Lines"
                    });
                for line in snapshot.git.patch_lines.iter().filter(|l| l.changed) {
                    let check = gtk::CheckButton::with_label(line.text.trim_end_matches('\n'));
                    let index = line.index;
                    let selected = view.selected.clone();
                    check.connect_toggled(move |check| {
                        if check.is_active() {
                            selected.borrow_mut().insert(index);
                        } else {
                            selected.borrow_mut().remove(&index);
                        }
                    });
                    view.lines.append(&check);
                }
            }
        }
        if Arc::ptr_eq(&ui.snapshot.borrow().git, &snapshot.git) {
            return;
        }
        self.updating.set(true);
        self.branch.remove_all();
        self.repository.remove_all();
        self.repository.set_visible(snapshot.git.roots.len() > 1);
        for root in &snapshot.git.roots {
            let value = root.to_string_lossy();
            self.repository.append(Some(&value), &value);
        }
        if let Some(root) = &snapshot.git.selected_root {
            self.repository.set_active_id(root.to_str());
        }
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        while let Some(child) = self.graph.first_child() {
            self.graph.remove(&child);
        }
        while let Some(child) = self.operation.first_child() {
            self.operation.remove(&child);
        }
        self.controls.set_visible(snapshot.git.repository.is_some());
        if let Some(repo) = &snapshot.git.repository {
            for b in &repo.branches {
                self.branch.append(Some(&b.name), &b.name);
            }
            if repo.branches.is_empty() {
                self.branch.append(Some(&repo.branch), &repo.branch);
            }
            self.branch.set_active_id(Some(&repo.branch));
            self.summary
                .set_label(&format!("↑ {}  ↓ {}", repo.ahead, repo.behind));
            if let Some(kind) = &repo.operation {
                self.operation
                    .append(&gtk::Label::new(Some(&format!("{kind} in progress"))));
                let continue_button = gtk::Button::with_label("Continue");
                let next = ui.clone();
                continue_button.connect_clicked(move |_| {
                    action(&next, "continue", String::new(), String::new(), false)
                });
                self.operation.append(&continue_button);
                let abort = gtk::Button::with_label("Abort");
                let next = ui.clone();
                abort.connect_clicked(move |_| confirm(&next, "abort", String::new()));
                self.operation.append(&abort);
            }
            for staged in [true, false] {
                let heading = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                let label =
                    gtk::Label::new(Some(if staged { "Staged Changes" } else { "Changes" }));
                label.set_xalign(0.0);
                label.set_hexpand(true);
                label.add_css_class("heading");
                heading.append(&label);
                let all = gtk::Button::from_icon_name(if staged {
                    "list-remove-symbolic"
                } else {
                    "list-add-symbolic"
                });
                all.set_tooltip_text(Some(if staged { "Unstage All" } else { "Stage All" }));
                let next = ui.clone();
                all.connect_clicked(move |_| {
                    action(
                        &next,
                        if staged { "unstage" } else { "stage" },
                        String::new(),
                        String::new(),
                        false,
                    )
                });
                heading.append(&all);
                self.rows.append(&heading);
                for change in repo
                    .changes
                    .iter()
                    .filter(|c| if staged { c.staged } else { c.unstaged })
                {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                    let button = gtk::Button::with_label(&change.path);
                    button.set_widget_name("gitReview");
                    button.add_css_class("flat");
                    button.set_hexpand(true);
                    let path = change.path.clone();
                    let next = ui.clone();
                    let panel = self.clone();
                    button.connect_clicked(move |_| {
                        panel.review(&next, |ui| {
                            ui.dispatch(AppCommand::ShowGitDiff {
                                path: path.clone(),
                                staged,
                            })
                        })
                    });
                    row.append(&button);
                    if change.conflicted {
                        let resolve = gtk::Button::with_label("Resolve");
                        let path = change.path.clone();
                        let next = ui.clone();
                        let panel = self.clone();
                        resolve.connect_clicked(move |_| {
                            panel.review(&next, |ui| {
                                action(ui, "resolve", path.clone(), String::new(), false)
                            })
                        });
                        row.append(&resolve);
                    }
                    let status = gtk::Label::new(Some(if change.conflicted {
                        "!"
                    } else if staged {
                        &change.index
                    } else {
                        &change.worktree
                    }));
                    row.append(&status);
                    let stage = gtk::Button::from_icon_name(if staged {
                        "list-remove-symbolic"
                    } else {
                        "list-add-symbolic"
                    });
                    let path = change.path.clone();
                    let next = ui.clone();
                    stage.connect_clicked(move |_| {
                        action(
                            &next,
                            if staged { "unstage" } else { "stage" },
                            path.clone(),
                            String::new(),
                            false,
                        )
                    });
                    row.append(&stage);
                    if !staged {
                        let discard = gtk::Button::from_icon_name("edit-undo-symbolic");
                        discard.set_tooltip_text(Some("Discard Changes…"));
                        let path = change.path.clone();
                        let next = ui.clone();
                        discard.connect_clicked(move |_| confirm(&next, "discard", path.clone()));
                        row.append(&discard);
                    }
                    self.rows.append(&row);
                }
            }
            if repo.changes.is_empty() {
                self.rows
                    .append(&gtk::Label::new(Some("Working tree clean")));
            }
        } else {
            self.summary
                .set_label("Initialize or clone a repository from Git Actions.");
        }
        self.updating.set(false);
        while let Some(child) = self.extras.first_child() {
            self.extras.remove(&child);
        }
        if let Some(repo) = &snapshot.git.repository {
            for stash in &repo.stashes {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 3);
                let title = gtk::Label::new(Some(&format!("[{}] {}", stash.index, stash.subject)));
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title.set_max_width_chars(22);
                row.append(&title);
                for (label, id, flag) in [
                    ("Apply", "stash_apply", false),
                    ("Pop", "stash_apply", true),
                    ("Delete…", "stash_drop", false),
                ] {
                    let button = gtk::Button::with_label(label);
                    let value = stash.index.to_string();
                    let next = ui.clone();
                    button.connect_clicked(move |_| {
                        if id == "stash_drop" {
                            confirm(&next, id, value.clone())
                        } else {
                            action(&next, id, value.clone(), String::new(), flag)
                        }
                    });
                    row.append(&button);
                }
                self.extras.append(&row);
            }
            for worktree in &repo.worktrees {
                let button =
                    gtk::Button::with_label(&format!("Open Worktree: {}", worktree.branch));
                let path = std::path::PathBuf::from(&worktree.path);
                let next = ui.clone();
                button.connect_clicked(move |_| {
                    next.dispatch(AppCommand::OpenWorkspace(path.clone()))
                });
                self.extras.append(&button);
            }
            for remote in &repo.remotes {
                let button = gtk::Button::with_label(&format!("Fetch {}", remote.name));
                button.set_tooltip_text(Some(&remote.url));
                let name = remote.name.clone();
                let next = ui.clone();
                button.connect_clicked(move |_| {
                    action(&next, "fetch", name.clone(), String::new(), false)
                });
                self.extras.append(&button);
            }
        }
        for row in &snapshot.git.history {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let drawing = gtk::DrawingArea::new();
            drawing.set_content_width((row.columns.max(2) * 14) as i32);
            drawing.set_content_height(48);
            let data = row.clone();
            drawing.set_draw_func(move |_, cr, _, height| {
                let colors = [
                    (0.33, 0.60, 0.93),
                    (0.87, 0.58, 0.27),
                    (0.67, 0.47, 0.87),
                    (0.33, 0.67, 0.47),
                    (0.87, 0.47, 0.60),
                    (0.33, 0.67, 0.67),
                ];
                cr.set_line_width(2.0);
                for edge in &data.edges {
                    let (r, g, b) = colors[edge.color % colors.len()];
                    cr.set_source_rgb(r, g, b);
                    let ax = edge.from as f64 * 14.0 + 7.0;
                    let ay = edge.start as f64 * height as f64 / 2.0;
                    let bx = edge.to as f64 * 14.0 + 7.0;
                    let by = edge.end as f64 * height as f64 / 2.0;
                    cr.move_to(ax, ay);
                    cr.curve_to(ax, (ay + by) / 2.0, bx, (ay + by) / 2.0, bx, by);
                    let _ = cr.stroke();
                }
                let (r, g, b) = colors[data.lane % colors.len()];
                cr.set_source_rgb(r, g, b);
                cr.arc(
                    data.lane as f64 * 14.0 + 7.0,
                    height as f64 / 2.0,
                    4.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                let _ = cr.fill();
            });
            content.append(&drawing);
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
            labels.set_hexpand(true);
            for (value, class) in [
                (&row.commit.refs, "accent"),
                (&row.commit.subject, ""),
                (
                    &format!(
                        "{} · {} · {}",
                        &row.commit.id[..7],
                        row.commit.author,
                        &row.commit.date[..10]
                    ),
                    "dim-label",
                ),
            ] {
                let label = gtk::Label::new(Some(value));
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                if !class.is_empty() {
                    label.add_css_class(class);
                }
                labels.append(&label);
            }
            content.append(&labels);
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.set_child(Some(&content));
            button.add_css_class("git-graph-row");
            let id = row.commit.id.clone();
            let next = ui.clone();
            let panel = self.clone();
            button.connect_clicked(move |_| {
                panel.review(&next, |ui| {
                    action(ui, "show_commit", id.clone(), String::new(), false)
                })
            });
            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            button.set_hexpand(true);
            row_box.append(&button);
            let menu = gtk::MenuButton::new();
            menu.set_icon_name("view-more-symbolic");
            let options = gtk::Box::new(gtk::Orientation::Vertical, 2);
            for (title, op) in [
                ("Checkout Commit", "checkout"),
                ("Cherry-pick", "cherry_pick"),
                ("Revert Commit", "revert"),
            ] {
                let choice = gtk::Button::with_label(title);
                let id = row.commit.id.clone();
                let next = ui.clone();
                choice
                    .connect_clicked(move |_| action(&next, op, id.clone(), String::new(), false));
                options.append(&choice);
            }
            let popover = gtk::Popover::new();
            popover.set_child(Some(&options));
            menu.set_popover(Some(&popover));
            row_box.append(&menu);
            self.graph.append(&row_box);
        }
        if snapshot.git.history.len() >= snapshot.git.history_limit {
            let more = gtk::Button::with_label("Load More Commits");
            let next = ui.clone();
            more.connect_clicked(move |_| {
                let limit = next.snapshot.borrow().git.history_limit + 200;
                next.dispatch(AppCommand::LoadGitHistory(limit));
            });
            self.graph.append(&more);
        }
    }
}
