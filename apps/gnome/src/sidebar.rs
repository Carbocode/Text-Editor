//! GTK row materialization and icons; ordering comes from application snapshots.
use crate::ui::{PendingRow, SidebarEntry, SidebarRow, SidebarView, update_pending_row};
use crate::{material_icons, sidebar_interactions};
use adw::prelude::*;
use gtk::{gio, glib};
use native_workspace_application::{AppSnapshot, SidebarRowKind, file_icon_name, folder_icon_name};
use native_workspace_domain::path;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) fn sidebar_entry(
    snapshot: &AppSnapshot,
    row: native_workspace_application::SidebarRow,
) -> Option<SidebarEntry> {
    let (kind, title, suffix) = match row.kind {
        SidebarRowKind::Directory { .. } => (
            SidebarRow::Directory(row.path.clone()),
            path::name(&row.path).into(),
            None,
        ),
        SidebarRowKind::File => {
            let document = snapshot.document_summary(&row.path)?;
            (
                SidebarRow::File(document.id.clone()),
                document.id.name().into(),
                if document.external_change {
                    Some("⚠")
                } else if document.dirty {
                    Some("●")
                } else {
                    None
                },
            )
        }
        SidebarRowKind::Match { result } => {
            let result = snapshot.search_results[result].clone();
            let title = format!(
                "↳ {}:{}  {}",
                result.location.line, result.location.column, result.snippet
            );
            (SidebarRow::Match(result), title, None)
        }
    };
    Some(SidebarEntry {
        git_status: snapshot.sidebar_git_status(&row.path).into(),
        row: kind,
        path: row.path,
        title,
        depth: row.depth,
        suffix,
    })
}

pub(super) fn tree_entries(
    snapshot: &AppSnapshot,
    parent: &str,
    collapsed: &std::collections::HashSet<String>,
    entries: &mut Vec<SidebarEntry>,
) {
    // Only used when expanding one folder; shared ordering defines its rows.
    for child in snapshot.sidebar_children(parent) {
        let open = child.directory && !collapsed.contains(&child.path);
        let row = native_workspace_application::SidebarRow {
            depth: path::depth(&child.path),
            path: child.path.clone(),
            kind: if child.directory {
                SidebarRowKind::Directory { expanded: open }
            } else {
                SidebarRowKind::File
            },
        };
        if let Some(entry) = sidebar_entry(snapshot, row) {
            entries.push(entry);
        }
        if open {
            tree_entries(snapshot, &child.path, collapsed, entries);
        }
    }
}

pub(super) fn sidebar_row(
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
pub(super) fn sidebar_resize_handle(split: &adw::OverlaySplitView) -> gtk::Box {
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

pub(super) fn sidebar_entry_widget(entry: &SidebarEntry, collapsed: bool) -> gtk::Box {
    let widget = match &entry.row {
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
    };
    if !entry.git_status.is_empty() {
        widget.add_css_class(&format!("git-{}", entry.git_status.to_lowercase()));
        // The title expands, so the last child is a fixed trailing column.
        let badge = gtk::Label::new(Some(&entry.git_status));
        badge.add_css_class("git-badge");
        badge.set_width_chars(2);
        badge.set_halign(gtk::Align::End);
        badge.set_tooltip_text(Some(&format!("Git status: {}", entry.git_status)));
        widget.append(&badge);
    }
    widget
}

pub(super) fn icon_for(name: &str) -> gtk::gdk::Texture {
    texture_for_icon(file_icon_name(name))
}

pub(super) fn folder_icon_for(name: &str, expanded: bool) -> gtk::gdk::Texture {
    texture_for_icon(folder_icon_name(name, expanded))
}

// Keep the native leading indicator slot present even for clean documents, so
// toggling the dirty marker never changes tab measurements or truncates it.
pub(super) fn document_tab_indicator_icon(dirty: bool) -> gio::Icon {
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

pub(super) fn document_tab_icon(path: &str) -> gio::Icon {
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

pub(super) fn texture_for_icon(name: &str) -> gtk::gdk::Texture {
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

pub(super) fn build_sidebar(
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
