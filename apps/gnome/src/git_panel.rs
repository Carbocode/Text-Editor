//! Native Git controls and graph rendering; Git policy and graph geometry are shared.
use crate::ui::Ui;
use adw::prelude::*;
use native_workspace_application::{
    AppCommand, AppSnapshot, GitAction, GitChange, GitRefKind, GraphCommit, PatchKind,
    git_action_confirmation,
};
use native_workspace_domain::path;
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
    selected: Rc<RefCell<Option<String>>>,
}

const LANE: f64 = 12.0;
const ROW: i32 = 24;
const MAX_LANES: usize = 14;
const COLORS: [(f64, f64, f64); 8] = [
    (0.24, 0.56, 0.98),
    (0.96, 0.55, 0.20),
    (0.62, 0.42, 0.94),
    (0.25, 0.72, 0.47),
    (0.93, 0.36, 0.56),
    (0.18, 0.70, 0.76),
    (0.85, 0.70, 0.16),
    (0.55, 0.60, 0.68),
];

/// Status letters and colors are presentation; classification comes from Git.
fn status(code: &str) -> (&str, &'static str) {
    match code {
        "?" => ("U", "git-added"),
        "A" => ("A", "git-added"),
        "D" => ("D", "git-deleted"),
        "R" | "C" => (code, "git-renamed"),
        _ => (code, "git-modified"),
    }
}
fn change_status(change: &GitChange, staged: bool) -> (&str, &'static str) {
    if change.conflicted {
        ("!", "git-deleted")
    } else {
        status(if staged {
            &change.index
        } else {
            &change.worktree
        })
    }
}

/// Icon, file name, dimmed folder and a colored status letter.
fn file_label(file: &str, letter: &str, class: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let icon = gtk::Image::from_paintable(Some(&crate::sidebar::icon_for(file)));
    icon.set_pixel_size(14);
    row.append(&icon);
    let name = gtk::Label::new(Some(path::name(file)));
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&name);
    let folder = gtk::Label::new(Some(path::parent(file)));
    folder.add_css_class("dim-label");
    folder.add_css_class("caption");
    folder.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    folder.set_hexpand(true);
    folder.set_xalign(0.0);
    row.append(&folder);
    let code = gtk::Label::new(Some(letter));
    code.add_css_class("git-status");
    code.add_css_class(class);
    row.append(&code);
    row.set_tooltip_text(Some(file));
    row
}

fn draw_graph(row: &GraphCommit, cr: &gtk::cairo::Context, height: f64) {
    let x = |lane: usize| lane as f64 * LANE + LANE / 2.0 + 2.0;
    cr.set_line_width(1.6);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    for edge in row.edges.iter().filter(|e| e.from.max(e.to) < MAX_LANES) {
        let (r, g, b) = COLORS[edge.color % COLORS.len()];
        cr.set_source_rgb(r, g, b);
        let (ax, ay) = (x(edge.from), f64::from(edge.start) * height / 2.0);
        let (bx, by) = (x(edge.to), f64::from(edge.end) * height / 2.0);
        cr.move_to(ax, ay);
        let mid = (ay + by) / 2.0;
        cr.curve_to(ax, mid, bx, mid, bx, by);
        let _ = cr.stroke();
    }
    if row.lane >= MAX_LANES {
        return;
    }
    let (r, g, b) = COLORS[row.color % COLORS.len()];
    let (cx, cy) = (x(row.lane), height / 2.0);
    let radius = if row.head { 4.5 } else { 3.5 };
    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
    if row.merge {
        cr.set_source_rgb(0.5, 0.5, 0.5);
        cr.set_operator(gtk::cairo::Operator::Clear);
        let _ = cr.fill_preserve();
        cr.set_operator(gtk::cairo::Operator::Over);
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(1.8);
        let _ = cr.stroke();
    } else {
        cr.set_source_rgb(r, g, b);
        let _ = cr.fill();
    }
    if row.head {
        cr.set_source_rgba(r, g, b, 0.55);
        cr.set_line_width(1.2);
        cr.arc(cx, cy, 6.5, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();
    }
}

/// Lanes continuing below a commit stay visible through its inline detail.
fn draw_continuation(row: &GraphCommit, cr: &gtk::cairo::Context, height: f64) {
    cr.set_line_width(1.6);
    let mut drawn = HashSet::new();
    for edge in &row.edges {
        if edge.end == 2 && edge.to < MAX_LANES && drawn.insert(edge.to) {
            let (r, g, b) = COLORS[edge.color % COLORS.len()];
            cr.set_source_rgb(r, g, b);
            let x = edge.to as f64 * LANE + LANE / 2.0 + 2.0;
            cr.move_to(x, 0.0);
            cr.line_to(x, height);
            let _ = cr.stroke();
        }
    }
}

fn badge(name: &str, kind: GitRefKind, current: bool) -> gtk::Label {
    let short = if name.chars().count() > 24 {
        let chars: Vec<_> = name.chars().collect();
        format!(
            "{}…{}",
            chars[..11].iter().collect::<String>(),
            chars[chars.len() - 11..].iter().collect::<String>()
        )
    } else {
        name.into()
    };
    let label = gtk::Label::new(Some(&match (kind, current) {
        (GitRefKind::Tag, _) => format!("🏷 {short}"),
        (GitRefKind::Remote, _) => format!("☁ {short}"),
        (_, true) => format!("✓ {short}"),
        _ => short,
    }));
    label.add_css_class("git-ref");
    label.add_css_class(match kind {
        GitRefKind::Head => "git-ref-head",
        GitRefKind::Branch if current => "git-ref-current",
        GitRefKind::Branch => "git-ref-branch",
        GitRefKind::Remote => "git-ref-remote",
        GitRefKind::Tag => "git-ref-tag",
    });
    label.set_tooltip_text(Some(name));
    label
}

enum Item {
    Run(&'static str, String),
    /// A query whose result opens in the review window.
    Inspect(&'static str, String),
    Confirm(&'static str, String),
    Form(&'static str, String),
}

/// Flat popover menu; each entry runs one typed Git request.
fn popover_menu(ui: &Ui, items: Vec<(String, Item)>) -> gtk::Popover {
    let options = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    let items: Vec<_> = items
        .into_iter()
        .map(|(title, item)| (title, Rc::new(item)))
        .collect();
    for (title, item) in items {
        let choice = gtk::Button::with_label(&title);
        choice.add_css_class("flat");
        if let Some(label) = choice.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
        }
        let next = ui.clone();
        let close = popover.clone();
        choice.connect_clicked(move |_| {
            close.popdown();
            match &*item {
                Item::Run(id, value) => action(&next, id, value.clone(), String::new(), false),
                Item::Inspect(id, value) => {
                    let value = value.clone();
                    next.git
                        .review(&next, |ui| action(ui, id, value, String::new(), false));
                }
                Item::Confirm(id, value) => confirm(&next, id, value.clone()),
                Item::Form(id, value) => {
                    let spec = next
                        .snapshot
                        .borrow()
                        .git
                        .actions
                        .iter()
                        .find(|a| a.id == *id)
                        .cloned();
                    if let Some(spec) = spec {
                        form_with(&next, &spec, value);
                    }
                }
            }
        });
        options.append(&choice);
    }
    popover.set_child(Some(&options));
    popover
}

/// Builds the menu on secondary click and detaches it when closed, so rebuilt
/// rows never keep popovers alive.
fn context_menu(
    widget: &impl IsA<gtk::Widget>,
    ui: &Ui,
    items: impl Fn() -> Vec<(String, Item)> + 'static,
) {
    let click = gtk::GestureClick::builder().button(3).build();
    let ui = ui.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let popover = popover_menu(&ui, items());
        popover.set_parent(&widget);
        popover.set_has_arrow(false);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            gtk::glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
    });
    widget.add_controller(click);
}

fn commit_items(id: &str) -> Vec<(String, Item)> {
    let v = || id.to_owned();
    vec![
        ("Show Changes".into(), Item::Inspect("show_commit", v())),
        ("Checkout (Detached)".into(), Item::Run("checkout", v())),
        ("Create Branch Here…".into(), Item::Form("branch", v())),
        ("Create Tag Here…".into(), Item::Form("tag", v())),
        ("Merge into Current Branch".into(), Item::Run("merge", v())),
        (
            "Rebase Current Branch onto This…".into(),
            Item::Confirm("rebase", v()),
        ),
        ("Cherry-pick".into(), Item::Run("cherry_pick", v())),
        ("Revert Commit".into(), Item::Run("revert", v())),
    ]
}

fn ref_items(name: &str, kind: GitRefKind, current: bool) -> Vec<(String, Item)> {
    let v = || name.to_owned();
    match kind {
        GitRefKind::Branch if current => vec![("Rename…".into(), Item::Form("rename_branch", v()))],
        GitRefKind::Branch => vec![
            (format!("Checkout {name}"), Item::Run("checkout", v())),
            ("Merge into Current Branch".into(), Item::Run("merge", v())),
            (
                "Rebase Current Branch onto This…".into(),
                Item::Confirm("rebase", v()),
            ),
            ("Rename…".into(), Item::Form("rename_branch", v())),
            ("Delete…".into(), Item::Form("delete_branch", v())),
        ],
        GitRefKind::Remote => vec![
            ("Merge into Current Branch".into(), Item::Run("merge", v())),
            (
                "Rebase Current Branch onto This…".into(),
                Item::Confirm("rebase", v()),
            ),
        ],
        GitRefKind::Tag => vec![("Delete Tag…".into(), Item::Confirm("tag_delete", v()))],
        GitRefKind::Head => vec![("Create Branch Here…".into(), Item::Form("branch", v()))],
    }
}

/// Numbered, colored diff lines; other text (blame, errors) stays plain.
fn fill_review(buffer: &gtk::TextBuffer, text: &str, snapshot: &AppSnapshot) {
    let lines = &snapshot.git.patch_lines;
    if snapshot.error.is_some() || lines.is_empty() {
        buffer.set_text(text);
        return;
    }
    buffer.set_text("");
    let number = |n: Option<usize>| n.map_or(String::new(), |n| n.to_string());
    for line in lines {
        let mut end = buffer.end_iter();
        buffer.insert_with_tags_by_name(
            &mut end,
            &format!("{:>5} {:>5} ", number(line.old), number(line.new)),
            &["gutter"],
        );
        let tag = match line.kind {
            PatchKind::Added => "added",
            PatchKind::Removed => "removed",
            PatchKind::Hunk => "hunk",
            PatchKind::Meta => "meta",
            PatchKind::Context => "",
        };
        let body = if line.text.ends_with('\n') {
            line.text.clone()
        } else {
            format!("{}\n", line.text)
        };
        let mut end = buffer.end_iter();
        if tag.is_empty() {
            buffer.insert(&mut end, &body);
        } else {
            buffer.insert_with_tags_by_name(&mut end, &body, &[tag]);
        }
    }
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
    confirm_with(ui, id, value, String::new());
}
fn confirm_with(ui: &Ui, id: &'static str, value: String, extra: String) {
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
            action(&next, id, value.clone(), extra.clone(), false);
        }
    });
    dialog.present(Some(&ui.window));
}
fn form(ui: &Ui, spec: &GitAction) {
    form_with(ui, spec, "");
}
/// `prefill` fills the field naming the selected commit or reference.
fn form_with(ui: &Ui, spec: &GitAction, prefill: &str) {
    if spec.first.is_empty() && spec.second.is_empty() && spec.option.is_empty() {
        if spec.destructive {
            confirm(ui, spec.id, String::new());
        } else {
            action(ui, spec.id, String::new(), String::new(), false);
        }
        return;
    }
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
    // Branch/tag creation start from the selected commit; others name it first.
    if matches!(spec.id, "branch" | "tag") {
        second.set_text(prefill);
    } else {
        first.set_text(prefill);
    }
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
            selected: Rc::default(),
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
        let snapshot = ui.snapshot.borrow();
        let mut group = "";
        for spec in &snapshot.git.actions {
            if spec.group != group {
                group = spec.group;
                let heading = gtk::Label::new(Some(group));
                heading.set_xalign(0.0);
                heading.add_css_class("caption-heading");
                heading.add_css_class("dim-label");
                heading.set_margin_top(6);
                choices.append(&heading);
            }
            let button = gtk::Button::with_label(spec.title);
            button.add_css_class("flat");
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.0);
            }
            let next = ui.clone();
            let spec = spec.clone();
            button.connect_clicked(move |_| form(&next, &spec));
            choices.append(&button);
        }
        drop(snapshot);
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
        let table = text.buffer().tag_table();
        for (name, background, foreground) in [
            ("added", Some("rgba(46,160,67,0.18)"), None),
            ("removed", Some("rgba(248,81,73,0.18)"), None),
            ("hunk", Some("rgba(56,139,253,0.12)"), Some("#388bfd")),
            ("meta", None, Some("#8b949e")),
            ("gutter", None, Some("#8b949e")),
        ] {
            let tag = gtk::TextTag::new(Some(name));
            if let Some(color) = background {
                tag.set_paragraph_background(Some(color));
            }
            if let Some(color) = foreground {
                tag.set_foreground(Some(color));
            }
            table.add(&tag);
        }
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
                fill_review(&view.buffer, &text, snapshot);
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
            type Include = fn(&GitChange) -> bool;
            let sections: [(&str, bool, Include); 3] = [
                ("Merge Changes", false, |c| c.conflicted),
                ("Staged Changes", true, |c| c.staged && !c.conflicted),
                ("Changes", false, |c| c.unstaged && !c.conflicted),
            ];
            for (title, staged, include) in sections {
                let changes: Vec<_> = repo.changes.iter().filter(|c| include(c)).collect();
                if changes.is_empty() && title != "Changes" {
                    continue;
                }
                let heading = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                heading.add_css_class("git-section");
                let label = gtk::Label::new(Some(&title.to_uppercase()));
                label.set_xalign(0.0);
                label.set_hexpand(true);
                label.add_css_class("caption-heading");
                label.add_css_class("dim-label");
                heading.append(&label);
                if title != "Merge Changes" {
                    let all = gtk::Button::from_icon_name(if staged {
                        "list-remove-symbolic"
                    } else {
                        "list-add-symbolic"
                    });
                    all.add_css_class("flat");
                    all.set_sensitive(!changes.is_empty());
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
                }
                let count = gtk::Label::new(Some(&changes.len().to_string()));
                count.add_css_class("git-count");
                heading.append(&count);
                self.rows.append(&heading);
                for change in changes {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                    let (letter, class) = change_status(change, staged);
                    let button = gtk::Button::new();
                    button.set_child(Some(&file_label(&change.path, letter, class)));
                    button.set_widget_name("gitReview");
                    button.add_css_class("flat");
                    button.add_css_class("git-file");
                    button.set_hexpand(true);
                    let path = change.path.clone();
                    let next = ui.clone();
                    let panel = self.clone();
                    let conflicted = change.conflicted;
                    button.connect_clicked(move |_| {
                        let path = path.clone();
                        panel.review(&next, move |ui| {
                            if conflicted {
                                action(ui, "resolve", path, String::new(), false)
                            } else {
                                ui.dispatch(AppCommand::ShowGitDiff { path, staged })
                            }
                        })
                    });
                    let path = change.path.clone();
                    context_menu(&button, ui, move || {
                        let p = || path.clone();
                        let mut items = vec![
                            ("File History".into(), Item::Inspect("file_history", p())),
                            ("Blame".into(), Item::Inspect("blame", p())),
                            ("Add to .gitignore".into(), Item::Run("ignore", p())),
                        ];
                        if !staged && !conflicted {
                            items.push(("Discard Changes…".into(), Item::Confirm("discard", p())));
                        }
                        items
                    });
                    row.append(&button);
                    if !staged && !conflicted {
                        let discard = gtk::Button::from_icon_name("edit-undo-symbolic");
                        discard.add_css_class("flat");
                        discard.set_tooltip_text(Some("Discard Changes…"));
                        let path = change.path.clone();
                        let next = ui.clone();
                        discard.connect_clicked(move |_| confirm(&next, "discard", path.clone()));
                        row.append(&discard);
                    }
                    if !conflicted {
                        let stage = gtk::Button::from_icon_name(if staged {
                            "list-remove-symbolic"
                        } else {
                            "list-add-symbolic"
                        });
                        stage.add_css_class("flat");
                        stage.set_tooltip_text(Some(if staged { "Unstage" } else { "Stage" }));
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
                    }
                    self.rows.append(&row);
                }
            }
            if repo.changes.is_empty() {
                let clean = gtk::Label::new(Some("No changes"));
                clean.add_css_class("dim-label");
                clean.set_xalign(0.0);
                self.rows.append(&clean);
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
        let gutter = (snapshot.git.graph_width.clamp(1, MAX_LANES) as f64 * LANE + 4.0) as i32;
        let detail = snapshot
            .git
            .inspection
            .as_ref()
            .and_then(|i| i.commit.as_ref());
        for row in &snapshot.git.history {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            let drawing = gtk::DrawingArea::new();
            drawing.set_content_width(gutter);
            drawing.set_content_height(ROW);
            let data = row.clone();
            drawing.set_draw_func(move |_, cr, _, height| draw_graph(&data, cr, f64::from(height)));
            content.append(&drawing);
            for r in &row.commit.refs {
                let label = badge(&r.name, r.kind, r.current);
                let (name, kind, current) = (r.name.clone(), r.kind, r.current);
                context_menu(&label, ui, move || ref_items(&name, kind, current));
                content.append(&label);
            }
            let subject = gtk::Label::new(Some(&row.commit.subject));
            subject.set_xalign(0.0);
            subject.set_hexpand(true);
            subject.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if row.head {
                subject.add_css_class("heading");
            }
            content.append(&subject);
            let date = gtk::Label::new(Some(row.commit.date.get(..10).unwrap_or_default()));
            date.add_css_class("dim-label");
            date.add_css_class("caption");
            content.append(&date);
            content.set_tooltip_text(Some(&format!(
                "{}\n{} · {}\n{}",
                row.commit.subject, row.commit.author, row.commit.date, row.commit.id
            )));
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.set_child(Some(&content));
            button.add_css_class("git-graph-row");
            button.set_hexpand(true);
            let selected = self.selected.borrow().as_deref() == Some(row.commit.id.as_str());
            if selected {
                button.add_css_class("git-selected");
            }
            let id = row.commit.id.clone();
            context_menu(&button, ui, move || commit_items(&id));
            self.graph.append(&button);
            let expanded = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            expanded.set_visible(selected);
            if selected {
                let lanes = gtk::DrawingArea::new();
                lanes.set_content_width(gutter);
                let data = row.clone();
                lanes.set_draw_func(move |_, cr, _, height| {
                    draw_continuation(&data, cr, f64::from(height))
                });
                expanded.append(&lanes);
                let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
                card.add_css_class("git-detail");
                card.set_hexpand(true);
                match detail.filter(|d| d.id == row.commit.id) {
                    Some(detail) => {
                        let message = gtk::Label::new(Some(&detail.message));
                        message.set_wrap(true);
                        message.set_xalign(0.0);
                        message.set_selectable(true);
                        card.append(&message);
                        let meta = gtk::Label::new(Some(&format!(
                            "{} <{}> · {} · {}",
                            detail.author,
                            detail.email,
                            detail
                                .date
                                .get(..16)
                                .unwrap_or(&detail.date)
                                .replace('T', " "),
                            &detail.id[..detail.id.len().min(10)]
                        )));
                        meta.add_css_class("dim-label");
                        meta.add_css_class("caption");
                        meta.set_xalign(0.0);
                        meta.set_wrap(true);
                        meta.set_selectable(true);
                        card.append(&meta);
                        for file in &detail.files {
                            let (letter, class) = status(&file.status);
                            let open = gtk::Button::new();
                            open.set_child(Some(&file_label(&file.path, letter, class)));
                            open.add_css_class("flat");
                            open.add_css_class("git-file");
                            let (id, path) = (detail.id.clone(), file.path.clone());
                            let next = ui.clone();
                            let panel = self.clone();
                            open.connect_clicked(move |_| {
                                let (id, path) = (id.clone(), path.clone());
                                panel.review(&next, move |ui| {
                                    action(ui, "show_commit_file", id, path, false)
                                })
                            });
                            card.append(&open);
                        }
                        let full = gtk::Button::with_label("Full Diff");
                        full.add_css_class("flat");
                        full.set_halign(gtk::Align::Start);
                        let id = detail.id.clone();
                        let next = ui.clone();
                        let panel = self.clone();
                        full.connect_clicked(move |_| {
                            let id = id.clone();
                            panel.review(&next, move |ui| {
                                action(ui, "show_commit", id, String::new(), false)
                            })
                        });
                        card.append(&full);
                    }
                    None => card.append(&gtk::Spinner::builder().spinning(true).build()),
                }
                expanded.append(&card);
            }
            self.graph.append(&expanded);
            let id = row.commit.id.clone();
            let next = ui.clone();
            let panel = self.clone();
            button.connect_clicked(move |button| {
                let open = panel.selected.borrow().as_deref() == Some(id.as_str());
                if open {
                    // Collapsing is local; Git state is unchanged.
                    panel.selected.replace(None);
                    button.remove_css_class("git-selected");
                    expanded.set_visible(false);
                } else {
                    panel.selected.replace(Some(id.clone()));
                    action(&next, "show_commit", id.clone(), String::new(), false);
                }
            });
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
