//! Native sidebar editing and drag-and-drop. Payloads are accepted only from this sidebar.
use gtk::{gdk, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

type FileDropValidator = Rc<dyn Fn(&[std::path::PathBuf], &str) -> bool>;
type FileDropHandler = Rc<dyn Fn(Vec<std::path::PathBuf>, String)>;

type MoveValidator = Rc<dyn Fn(&str, &str) -> bool>;

#[derive(Clone)]
pub struct Callbacks {
    pub begin_drop: Rc<dyn Fn() -> native_workspace_application::SidebarDropTrace>,
    pub rename: Rc<dyn Fn(String, String) -> bool>,
    pub delete: Rc<dyn Fn(String)>,
    pub move_entry: Rc<dyn Fn(String, String) -> bool>,
    pub can_move: MoveValidator,
    pub can_move_files: FileDropValidator,
    pub move_files: FileDropHandler,
    pub expand: Rc<dyn Fn(&str) -> bool>,
    pub collapse: Rc<dyn Fn(&str)>,
}

#[derive(Default)]
pub struct Interactions {
    pub callbacks: RefCell<Option<Callbacks>>,
    editors: RefCell<HashMap<String, glib::WeakRef<gtk::Entry>>>,
    editing: RefCell<Option<String>>,
    pub selected: RefCell<Option<String>>,
    folders: RefCell<HashSet<String>>,
    drop_widgets: RefCell<Vec<glib::WeakRef<gtk::Box>>>,
    dragged: RefCell<Option<String>>,
    active_drag: RefCell<Option<gdk::Drag>>,
    external_paths: RefCell<Vec<std::path::PathBuf>>,
    hovered: RefCell<Option<String>>,
    hover_widget: RefCell<Option<glib::WeakRef<gtk::Box>>>,
    expand_timer: RefCell<Option<glib::SourceId>>,
    leave_timer: RefCell<Option<glib::SourceId>>,
    temporary_folders: RefCell<HashSet<String>>,
    root_target: RefCell<Option<glib::WeakRef<gtk::Box>>>,
    pending_drop: RefCell<Option<PendingDrop>>,
    drop_sequence: std::cell::Cell<u64>,
}

/// A validated release whose model update waits for the native drag session to end.
struct PendingDrop {
    sequence: u64,
    source: String,
    directory: String,
    temporary_folders: HashSet<String>,
    trace: native_workspace_application::SidebarDropTrace,
}

/// Commits a drop even if the toolkit never reports the end of its drag session.
const DROP_COMMIT_FALLBACK: Duration = Duration::from_secs(1);

impl Interactions {
    pub fn editor(&self, path: &str) -> Option<gtk::Entry> {
        self.editors
            .borrow()
            .get(path)
            .and_then(glib::WeakRef::upgrade)
    }

    pub fn row_widget(&self, path: &str) -> Option<gtk::Box> {
        self.editor(path)?
            .parent()?
            .parent()?
            .downcast::<gtk::Box>()
            .ok()
    }

    pub fn begin_rename(&self, path: &str) {
        if let Some(entry) = self.editor(path) {
            self.editing.replace(Some(path.into()));
            if let Some(stack) = entry
                .parent()
                .and_then(|widget| widget.downcast::<gtk::Stack>().ok())
            {
                if let Some(label) = stack
                    .child_by_name("label")
                    .and_then(|widget| widget.downcast::<gtk::Label>().ok())
                {
                    entry.set_text(&label.text());
                }
                entry.remove_css_class("error");
                stack.set_visible_child_name("edit");
            }
            entry.grab_focus();
            let name = entry.text();
            let stem = if self.folders.borrow().contains(path) {
                name.len()
            } else {
                name.rfind('.')
                    .filter(|position| *position > 0)
                    .unwrap_or(name.len())
            };
            entry.select_region(0, name[..stem].chars().count() as i32);
        }
    }

    pub fn start_drag(&self, path: &str) {
        self.commit_drop();
        self.finish_drag();
        self.dragged.replace(Some(path.into()));
        if let Some(widget) = self
            .root_target
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        {
            widget.set_visible(true);
        }
    }

    pub fn finish_drag(&self) {
        self.external_paths.borrow_mut().clear();
        self.finish_drag_to(None);
    }

    fn owns_drop(&self, drop: &gdk::Drop) -> bool {
        self.dragged.borrow().is_some()
            && self
                .active_drag
                .borrow()
                .as_ref()
                .is_some_and(|drag| drop.drag().as_ref() == Some(drag))
    }

    fn hide_drag_preview(&self) {
        if let Some(drag) = self.active_drag.borrow().as_ref() {
            gtk::DragIcon::for_drag(drag).set_child(None::<&gtk::Widget>);
            if let Some(surface) = drag.drag_surface() {
                surface.hide();
            }
        }
    }

    fn finish_drag_to(&self, destination: Option<&str>) {
        self.reset_drag_feedback();
        self.restore_temporary_folders(destination);
        self.temporary_folders.borrow_mut().clear();
    }

    /// Ends hover and preview feedback without touching the list model.
    fn reset_drag_feedback(&self) {
        self.hide_drag_preview();
        self.active_drag.take();
        self.dragged.take();
        self.cancel_hover();
        for widget in self
            .drop_widgets
            .borrow()
            .iter()
            .filter_map(glib::WeakRef::upgrade)
        {
            widget.remove_css_class("sidebar-drop-valid");
            widget.remove_css_class("sidebar-drop-invalid");
        }
        if let Some(widget) = self
            .root_target
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        {
            widget.set_visible(false);
        }
    }

    fn cancel_hover(&self) {
        if let Some(timer) = self.leave_timer.take() {
            timer.remove();
        }
        if let Some(timer) = self.expand_timer.take() {
            timer.remove();
        }
        self.hovered.take();
        self.hover_widget.take();
    }

    fn restore_temporary_folders(&self, destination: Option<&str>) {
        let mut closing: Vec<_> = self
            .temporary_folders
            .borrow()
            .iter()
            .filter(|folder| {
                destination.is_none_or(|target| {
                    target != folder.as_str() && !target.starts_with(&format!("{folder}/"))
                })
            })
            .cloned()
            .collect();
        closing.sort_by_key(|path| std::cmp::Reverse(path.len()));
        let callbacks = self.callbacks.borrow().clone();
        for folder in closing {
            self.temporary_folders.borrow_mut().remove(&folder);
            if let Some(callbacks) = &callbacks {
                (callbacks.collapse)(&folder);
            }
        }
    }

    pub fn leave(self: &Rc<Self>, directory: &str) {
        if self.hovered.borrow().as_deref() != Some(directory) {
            return;
        }
        if let Some(timer) = self.leave_timer.take() {
            timer.remove();
        }
        let state = self.clone();
        let path = directory.to_owned();
        self.leave_timer.replace(Some(glib::timeout_add_local_once(
            Duration::from_millis(50),
            move || {
                state.leave_timer.take();
                if state.hovered.borrow().as_deref() == Some(&path) {
                    state.cancel_hover();
                }
            },
        )));
    }

    fn can_drop(&self, directory: &str) -> bool {
        if !self.external_paths.borrow().is_empty() {
            return self
                .callbacks
                .borrow()
                .as_ref()
                .is_some_and(|c| (c.can_move_files)(&self.external_paths.borrow(), directory));
        }
        let source = self.dragged.borrow().clone();
        let callbacks = self.callbacks.borrow().clone();
        match (source, callbacks) {
            (Some(source), Some(callbacks)) => (callbacks.can_move)(&source, directory),
            _ => false,
        }
    }

    pub fn hover(self: &Rc<Self>, directory: &str, folder: bool) -> bool {
        let valid = self.can_drop(directory);
        if let Some(timer) = self.leave_timer.take() {
            timer.remove();
        }
        if self.hovered.borrow().as_deref() != Some(directory) {
            self.cancel_hover();
            self.restore_temporary_folders(Some(directory));
            self.hovered.replace(Some(directory.into()));
            if valid && folder && !directory.is_empty() {
                let state = self.clone();
                let directory = directory.to_owned();
                let timer = glib::timeout_add_local_once(Duration::from_millis(450), move || {
                    state.expand_timer.take();
                    if state.hovered.borrow().as_deref() == Some(&directory)
                        && state.can_drop(&directory)
                    {
                        let callbacks = state.callbacks.borrow().clone();
                        if let Some(callbacks) = callbacks
                            && (callbacks.expand)(&directory)
                        {
                            state
                                .temporary_folders
                                .borrow_mut()
                                .insert(directory.clone());
                        }
                    }
                });
                self.expand_timer.replace(Some(timer));
            }
        }
        valid
    }

    /// Accepts a release inside the drop signal. The move, folder expansion and
    /// list rebuild run in `commit_drop` once GTK reports the drag as ended:
    /// replacing the list model here destroys the rows owning the active drag
    /// source and drop target, which wedges the X11/Wayland drag session.
    pub fn drop_entry(self: &Rc<Self>, source: &str, directory: &str) -> bool {
        if self.dragged.borrow().as_deref() != Some(source) || !self.can_drop(directory) {
            return false;
        }
        let trace = self
            .callbacks
            .borrow()
            .as_ref()
            .map(|callbacks| (callbacks.begin_drop)())
            .unwrap_or_default();
        let temporary_folders = self.temporary_folders.take();
        self.reset_drag_feedback();
        trace.preview_hidden();
        let sequence = self.drop_sequence.get() + 1;
        self.drop_sequence.set(sequence);
        self.pending_drop.replace(Some(PendingDrop {
            sequence,
            source: source.into(),
            directory: directory.into(),
            temporary_folders,
            trace,
        }));
        let state = self.clone();
        glib::timeout_add_local_once(DROP_COMMIT_FALLBACK, move || {
            let current = state
                .pending_drop
                .borrow()
                .as_ref()
                .is_some_and(|drop| drop.sequence == sequence);
            if current {
                state.commit_drop();
            }
        });
        true
    }

    /// Applies an accepted drop. Returns whether the move was dispatched.
    pub fn commit_drop(&self) -> bool {
        let Some(drop) = self.pending_drop.take() else {
            return false;
        };
        let callbacks = self.callbacks.borrow().clone();
        self.temporary_folders.replace(drop.temporary_folders);
        self.restore_temporary_folders(Some(&drop.directory));
        let mut temporary = self.temporary_folders.take();
        // A quick release must reveal the pending row even before hover expansion.
        if !drop.directory.is_empty()
            && let Some(callbacks) = &callbacks
            && (callbacks.expand)(&drop.directory)
        {
            temporary.insert(drop.directory.clone());
        }
        let accepted = callbacks
            .as_ref()
            .is_some_and(|callbacks| (callbacks.move_entry)(drop.source, drop.directory));
        if !accepted && let Some(callbacks) = callbacks {
            for folder in temporary {
                (callbacks.collapse)(&folder);
            }
        }
        drop.trace.handler_finished(accepted);
        accepted
    }

    pub fn attach(self: &Rc<Self>, widget: &gtk::Box, path: &str, folder: bool) {
        if folder {
            self.folders.borrow_mut().insert(path.into());
        } else {
            self.folders.borrow_mut().remove(path);
        }
        let mut child = widget.first_child();
        let label = loop {
            let Some(current) = child else { return };
            if current.hexpands()
                && let Ok(label) = current.clone().downcast::<gtk::Label>()
            {
                break label;
            }
            child = current.next_sibling();
        };
        let previous = label.prev_sibling();
        let next = label.next_sibling();
        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        let entry = gtk::Entry::new();
        entry.set_text(&label.text());
        entry.set_has_frame(true);
        entry.add_css_class("sidebar-rename");
        widget.remove(&label);
        stack.add_named(&label, Some("label"));
        stack.add_named(&entry, Some("edit"));
        widget.insert_child_after(&stack, previous.as_ref());
        // append before the optional modified/conflict marker
        if let Some(next) = next {
            widget.reorder_child_after(&next, Some(&stack));
        }
        self.editors
            .borrow_mut()
            .retain(|_, editor| editor.upgrade().is_some());
        self.editors
            .borrow_mut()
            .insert(path.into(), entry.downgrade());
        {
            let state = self.clone();
            let path = path.to_owned();
            let weak_stack = stack.downgrade();
            entry.connect_activate(move |entry| {
                let callbacks = state.callbacks.borrow().clone();
                if callbacks.is_some_and(|callbacks| {
                    (callbacks.rename)(path.clone(), entry.text().to_string())
                }) {
                    state.editing.take();
                    if let Some(stack) = weak_stack.upgrade() {
                        stack.set_visible_child_name("label");
                    }
                } else {
                    entry.add_css_class("error");
                }
            });
        }
        {
            let state = self.clone();
            let weak_stack = stack.downgrade();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    state.editing.take();
                    if let Some(stack) = weak_stack.upgrade() {
                        stack.set_visible_child_name("label");
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            entry.add_controller(key);
        }
        {
            let state = self.clone();
            let weak_stack = stack.downgrade();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(move |_| {
                state.editing.take();
                if let Some(stack) = weak_stack.upgrade() {
                    stack.set_visible_child_name("label");
                }
            });
            entry.add_controller(focus);
        }
        let gesture = gtk::GestureClick::new();
        gesture.set_button(3);
        {
            let state = self.clone();
            let weak_widget = widget.downgrade();
            let path = path.to_owned();
            gesture.connect_pressed(move |gesture, _, x, y| {
                let Some(widget) = weak_widget.upgrade() else {
                    return;
                };
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let menu = gtk::Popover::new();
                menu.set_parent(&widget);
                menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                let button = gtk::Button::with_label("Rename");
                let actions = gtk::Box::new(gtk::Orientation::Vertical, 4);
                actions.append(&button);
                let delete = gtk::Button::with_label("Delete…");
                delete.add_css_class("destructive-action");
                actions.append(&delete);
                menu.set_child(Some(&actions));
                let delete_state = state.clone();
                let delete_path = path.clone();
                let delete_menu = menu.downgrade();
                delete.connect_clicked(move |_| {
                    if let Some(menu) = delete_menu.upgrade() {
                        menu.popdown();
                    }
                    if let Some(callbacks) = delete_state.callbacks.borrow().as_ref() {
                        (callbacks.delete)(delete_path.clone());
                    }
                });
                let weak_menu = menu.downgrade();
                let state = state.clone();
                let path = path.clone();
                button.connect_clicked(move |_| {
                    if let Some(menu) = weak_menu.upgrade() {
                        menu.popdown();
                    }
                    state.begin_rename(&path);
                });
                menu.connect_closed(|menu| menu.unparent());
                menu.popup();
            });
        }
        widget.add_controller(gesture);
        let gesture = gtk::GestureClick::new();
        gesture.set_name(Some("sidebar-entry-click"));
        gesture.set_button(1);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let state = self.clone();
            let path = path.to_owned();
            let disclosure = folder
                .then(|| widget.first_child())
                .flatten()
                .map(|widget| widget.downgrade());
            gesture.connect_pressed(move |gesture, _, x, _| {
                if state.editing.borrow().is_some() || state.dragged.borrow().is_some() {
                    return;
                }
                let selected = state.selected.borrow().as_deref() == Some(&path);
                let on_disclosure = disclosure
                    .as_ref()
                    .and_then(glib::WeakRef::upgrade)
                    .is_some_and(|disclosure| x < f64::from(disclosure.width()));
                if folder && (on_disclosure || selected) {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    if let Some(callbacks) = state.callbacks.borrow().clone()
                        && !(callbacks.expand)(&path)
                    {
                        (callbacks.collapse)(&path);
                    }
                }
            });
        }
        widget.add_controller(gesture);
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        {
            let state = self.clone();
            let path = path.to_owned();
            let weak_widget = widget.downgrade();
            source.connect_prepare(move |_, _, _| {
                if state.editing.borrow().is_some()
                    || weak_widget
                        .upgrade()
                        .is_some_and(|widget| widget.has_css_class("pending-move"))
                {
                    return None;
                }
                state.start_drag(&path);
                Some(gdk::ContentProvider::for_value(&path.to_value()))
            });
        }
        {
            let state = self.clone();
            let title = path.rsplit('/').next().unwrap_or(path).to_owned();
            source.connect_drag_begin(move |_, drag| {
                state.active_drag.replace(Some(drag.clone()));
                let release_state = state.clone();
                drag.connect_drop_performed(move |_| release_state.hide_drag_preview());
                let ghost = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                ghost.add_css_class("sidebar-drag-ghost");
                ghost.append(&gtk::Image::from_icon_name(if folder {
                    "folder-symbolic"
                } else {
                    "text-x-generic-symbolic"
                }));
                ghost.append(&gtk::Label::new(Some(&title)));
                gtk::DragIcon::for_drag(drag).set_child(Some(&ghost));
            });
        }
        {
            let state = self.clone();
            source.connect_drag_end(move |_, _, _| {
                state.finish_drag();
                // Leave the drag-end emission before the model replaces this row.
                let state = state.clone();
                glib::idle_add_local_once(move || {
                    state.commit_drop();
                });
            });
        }
        {
            let state = self.clone();
            source.connect_drag_cancel(move |_, _, _| {
                state.finish_drag();
                true
            });
        }
        widget.add_controller(source);
        let directory = if folder {
            path
        } else {
            path.rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or("")
        };
        let row_area = widget
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Box>().ok());
        self.attach_drop(row_area.as_ref().unwrap_or(widget), directory, folder);
    }

    pub fn attach_root(self: &Rc<Self>, widget: &gtk::Box) {
        widget.add_css_class("sidebar-root-drop");
        widget.set_visible(false);
        self.root_target.replace(Some(widget.downgrade()));
        self.attach_drop(widget, "", false);
    }

    pub fn attach_external_drop(
        self: &Rc<Self>,
        widget: &impl IsA<gtk::Widget>,
        directory: &str,
        folder: bool,
    ) {
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::MOVE);
        target.set_preload(true);
        let state = self.clone();
        let path = directory.to_owned();
        let weak = widget.as_ref().downgrade();
        let feedback = Rc::new(move |target: &gtk::DropTarget| {
            let Some(files) = target.value_as::<gdk::FileList>() else {
                return gdk::DragAction::empty();
            };
            let Some(paths) = file_paths(&files) else {
                return gdk::DragAction::empty();
            };
            state.external_paths.replace(paths);
            let valid = state.hover(&path, folder);
            if let Some(widget) = weak.upgrade() {
                widget.remove_css_class("sidebar-drop-valid");
                widget.remove_css_class("sidebar-drop-invalid");
                widget.add_css_class(if valid {
                    "sidebar-drop-valid"
                } else {
                    "sidebar-drop-invalid"
                });
            }
            if valid {
                gdk::DragAction::MOVE
            } else {
                gdk::DragAction::empty()
            }
        });
        let motion = feedback.clone();
        target.connect_motion(move |target, _, _| motion(target));
        let enter = feedback.clone();
        target.connect_enter(move |target, _, _| enter(target));
        target.connect_value_notify(move |target| {
            feedback(target);
        });
        let state = self.clone();
        let path = directory.to_owned();
        let weak = widget.as_ref().downgrade();
        target.connect_leave(move |_| {
            state.leave(&path);
            if let Some(widget) = weak.upgrade() {
                widget.remove_css_class("sidebar-drop-valid");
                widget.remove_css_class("sidebar-drop-invalid");
            }
            let state = state.clone();
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if state.hovered.borrow().is_none() {
                    state.finish_drag();
                }
            });
        });
        let state = self.clone();
        let path = directory.to_owned();
        target.connect_drop(move |_, value, _, _| {
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let Some(paths) = file_paths(&files) else {
                return false;
            };
            let callbacks = state.callbacks.borrow().clone();
            let Some(callbacks) = callbacks else {
                return false;
            };
            if !(callbacks.can_move_files)(&paths, &path) {
                return false;
            }
            state.restore_temporary_folders(Some(&path));
            if !path.is_empty() {
                (callbacks.expand)(&path);
            }
            state.temporary_folders.borrow_mut().clear();
            state.finish_drag();
            let directory = path.clone();
            // Leave GTK's drop emission before replacing the row model.
            glib::idle_add_local_once(move || (callbacks.move_files)(paths, directory));
            true
        });
        widget.add_controller(target);
    }

    fn attach_drop(self: &Rc<Self>, widget: &gtk::Box, directory: &str, folder: bool) {
        self.attach_external_drop(widget, directory, folder);
        self.drop_widgets
            .borrow_mut()
            .retain(|widget| widget.upgrade().is_some());
        self.drop_widgets.borrow_mut().push(widget.downgrade());
        // The source path is already local. Handle the drop signal directly,
        // without waiting for content-provider decoding before clearing feedback.
        let target = gtk::DropTargetAsync::new(
            Some(gdk::ContentFormats::for_type(String::static_type()).union_serialize_mime_types()),
            gdk::DragAction::MOVE,
        );
        let state = self.clone();
        target.connect_accept(move |_, drop| state.owns_drop(drop));
        let state = self.clone();
        let path = directory.to_owned();
        let weak_widget = widget.downgrade();
        target.connect_drag_enter(move |_, _, _, _| {
            let valid = state.hover(&path, folder);
            state.hover_widget.replace(Some(weak_widget.clone()));
            if let Some(widget) = weak_widget.upgrade() {
                widget.add_css_class(if valid {
                    "sidebar-drop-valid"
                } else {
                    "sidebar-drop-invalid"
                });
            }
            if valid {
                gdk::DragAction::MOVE
            } else {
                gdk::DragAction::empty()
            }
        });
        let state = self.clone();
        let path = directory.to_owned();
        let weak_widget = widget.downgrade();
        target.connect_drag_motion(move |_, _, _, _| {
            let valid = state.hover(&path, folder);
            state.hover_widget.replace(Some(weak_widget.clone()));
            if valid {
                gdk::DragAction::MOVE
            } else {
                gdk::DragAction::empty()
            }
        });
        let state = self.clone();
        let path = directory.to_owned();
        let weak_widget = widget.downgrade();
        target.connect_drag_leave(move |_, _| {
            state.leave(&path);
            let state = state.clone();
            let path = path.clone();
            let weak_widget = weak_widget.clone();
            glib::timeout_add_local_once(Duration::from_millis(55), move || {
                if let Some(widget) = weak_widget.upgrade() {
                    let current = state
                        .hover_widget
                        .borrow()
                        .as_ref()
                        .and_then(glib::WeakRef::upgrade);
                    if state.hovered.borrow().as_deref() != Some(&path)
                        || current.as_ref() != Some(&widget)
                    {
                        widget.remove_css_class("sidebar-drop-valid");
                        widget.remove_css_class("sidebar-drop-invalid");
                    }
                }
            });
        });
        let state = self.clone();
        let path = directory.to_owned();
        target.connect_drop(move |_, drop, _, _| {
            if !state.owns_drop(drop) {
                return false;
            }
            let source = state.dragged.borrow().clone().unwrap();
            let accepted = state.drop_entry(&source, &path);
            drop.finish(if accepted {
                gdk::DragAction::MOVE
            } else {
                gdk::DragAction::empty()
            });
            true
        });
        widget.add_controller(target);
    }
}

/// URI decoding belongs to GDK; only local files cross the application port.
pub fn file_paths(files: &gdk::FileList) -> Option<Vec<std::path::PathBuf>> {
    let paths: Option<Vec<_>> = files.files().iter().map(|file| file.path()).collect();
    paths.filter(|paths| !paths.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_delay_cancels_on_leave_and_drag_end_and_rejects_foreign_payloads() {
        let context = glib::MainContext::default();
        let _owner = context.acquire().unwrap();
        let expanded = Rc::new(RefCell::new(Vec::new()));
        let recorded = expanded.clone();
        let moves = Rc::new(RefCell::new(Vec::new()));
        let recorded_moves = moves.clone();
        let state = Rc::new(Interactions::default());
        let release_state = Rc::downgrade(&state);
        state.callbacks.replace(Some(Callbacks {
            begin_drop: Rc::new(native_workspace_application::SidebarDropTrace::default),
            rename: Rc::new(|_, _| true),
            delete: Rc::new(|_| {}),
            move_entry: Rc::new(move |source, directory| {
                let release_state = release_state.upgrade().unwrap();
                assert!(
                    release_state.dragged.borrow().is_none(),
                    "Drop kept the drag active while moving the model"
                );
                assert!(release_state.hovered.borrow().is_none());
                recorded_moves.borrow_mut().push((source, directory));
                true
            }),
            can_move_files: Rc::new(|_, _| false),
            move_files: Rc::new(|_, _| {}),
            can_move: Rc::new(|source, directory| source != directory),
            expand: Rc::new(move |directory| {
                recorded.borrow_mut().push(directory.to_owned());
                true
            }),
            collapse: Rc::new(|_| {}),
        }));
        let pump = |milliseconds| {
            let until = std::time::Instant::now() + Duration::from_millis(milliseconds);
            while std::time::Instant::now() < until {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let open = Rc::new(RefCell::new(HashSet::from(["already-open".to_owned()])));
        let expanded_open = open.clone();
        let collapsed_open = open.clone();
        let branches = Rc::new(Interactions::default());
        branches.callbacks.replace(Some(Callbacks {
            begin_drop: Rc::new(native_workspace_application::SidebarDropTrace::default),
            rename: Rc::new(|_, _| true),
            delete: Rc::new(|_| {}),
            move_entry: Rc::new(|_, _| true),
            can_move_files: Rc::new(|_, _| false),
            move_files: Rc::new(|_, _| {}),
            can_move: Rc::new(|_, _| true),
            expand: Rc::new(move |path| expanded_open.borrow_mut().insert(path.into())),
            collapse: Rc::new(move |path| {
                collapsed_open.borrow_mut().remove(path);
            }),
        }));
        branches.start_drag("source");
        branches.hover("already-open", true);
        pump(500);
        branches.hover("a", true);
        pump(250);
        branches.leave("a");
        branches.hover("a", true);
        pump(250);
        assert!(
            open.borrow().contains("a"),
            "movement must not restart the folder deadline"
        );
        branches.hover("a/child", true);
        pump(500);
        assert!(open.borrow().contains("a"));
        branches.hover("b", true);
        assert!(!open.borrow().contains("a") && !open.borrow().contains("a/child"));
        pump(500);
        assert!(branches.drop_entry("source", "b"));
        assert!(branches.commit_drop());
        assert!(open.borrow().contains("b"));
        branches.start_drag("source");
        assert!(branches.drop_entry("source", "quick-release"));
        assert!(
            !open.borrow().contains("quick-release"),
            "the drop signal must not change the list model"
        );
        assert!(branches.commit_drop());
        assert!(open.borrow().contains("quick-release"));
        branches.start_drag("source");
        branches.hover("cancelled", true);
        pump(500);
        branches.finish_drag();
        assert!(
            !open.borrow().contains("cancelled")
                && open.borrow().contains("b")
                && open.borrow().contains("already-open")
        );

        branches.callbacks.borrow_mut().as_mut().unwrap().move_entry = Rc::new(|_, _| false);
        branches.start_drag("source");
        branches.hover("rejected", true);
        pump(500);
        assert!(open.borrow().contains("rejected"));
        assert!(branches.drop_entry("source", "rejected"));
        assert!(branches.dragged.borrow().is_none());
        assert!(open.borrow().contains("rejected"));
        assert!(!branches.commit_drop());
        assert!(!open.borrow().contains("rejected"));
        branches.start_drag("source");
        assert!(branches.drop_entry("source", "quick-rejected"));
        // A drag-end that never arrives still commits through the fallback.
        pump(1100);
        assert!(!open.borrow().contains("quick-rejected"));

        state.start_drag("source");
        assert!(state.hover("cancelled", true));
        pump(100);
        assert!(expanded.borrow().is_empty());
        state.cancel_hover();
        assert!(state.hover("destination", true));
        pump(700);
        assert_eq!(*expanded.borrow(), ["destination"]);
        assert!(!state.drop_entry("foreign payload", "destination"));
        assert!(!state.hover("source", true));
        assert!(state.hover("ended", true));
        state.finish_drag();
        pump(700);
        assert_eq!(*expanded.borrow(), ["destination"]);
        assert!(moves.borrow().is_empty());
        state.start_drag("source");
        assert!(state.drop_entry("source", "destination"));
        assert!(moves.borrow().is_empty());
        assert!(state.commit_drop());
        assert!(!state.commit_drop());
        assert_eq!(
            *moves.borrow(),
            [("source".to_owned(), "destination".to_owned())]
        );
    }
}
