//! Native sidebar editing and drag-and-drop. Payloads are accepted only from this sidebar.
use gtk::{gdk, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

type MoveValidator = Rc<dyn Fn(&str, &str) -> bool>;

#[derive(Clone)]
pub struct Callbacks {
    pub rename: Rc<dyn Fn(String, String) -> bool>,
    pub move_entry: Rc<dyn Fn(String, String) -> bool>,
    pub can_move: MoveValidator,
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
    hovered: RefCell<Option<String>>,
    hover_widget: RefCell<Option<glib::WeakRef<gtk::Box>>>,
    expand_timer: RefCell<Option<glib::SourceId>>,
    leave_timer: RefCell<Option<glib::SourceId>>,
    temporary_folders: RefCell<HashSet<String>>,
    root_target: RefCell<Option<glib::WeakRef<gtk::Box>>>,
}

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
        self.finish_drag_to(None);
    }

    fn finish_drag_to(&self, destination: Option<&str>) {
        self.dragged.take();
        self.cancel_hover();
        self.restore_temporary_folders(destination);
        self.temporary_folders.borrow_mut().clear();
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

    pub fn drop_entry(&self, source: &str, directory: &str) -> bool {
        if self.dragged.borrow().as_deref() != Some(source) || !self.can_drop(directory) {
            return false;
        }
        let callbacks = self.callbacks.borrow().clone();
        let succeeded = callbacks
            .is_some_and(|callbacks| (callbacks.move_entry)(source.into(), directory.into()));
        self.finish_drag_to(succeeded.then_some(directory));
        succeeded
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
                menu.set_child(Some(&button));
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
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        {
            let state = self.clone();
            let path = path.to_owned();
            source.connect_prepare(move |_, _, _| {
                if state.editing.borrow().is_some() {
                    return None;
                }
                state.start_drag(&path);
                Some(gdk::ContentProvider::for_value(&path.to_value()))
            });
        }
        {
            let title = path.rsplit('/').next().unwrap_or(path).to_owned();
            source.connect_drag_begin(move |_, drag| {
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
            source.connect_drag_end(move |_, _, _| state.finish_drag());
        }
        {
            let state = self.clone();
            source.connect_drag_cancel(move |_, _, _| {
                state.finish_drag();
                false
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

    fn attach_drop(self: &Rc<Self>, widget: &gtk::Box, directory: &str, folder: bool) {
        self.drop_widgets
            .borrow_mut()
            .retain(|widget| widget.upgrade().is_some());
        self.drop_widgets.borrow_mut().push(widget.downgrade());
        let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        let state = self.clone();
        target.connect_accept(move |_, _| state.dragged.borrow().is_some());
        let state = self.clone();
        let path = directory.to_owned();
        let weak_widget = widget.downgrade();
        target.connect_enter(move |_, _, _| {
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
        target.connect_motion(move |_, _, _| {
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
        target.connect_leave(move |_| {
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
        target.connect_drop(move |_, value, _, _| {
            value
                .get::<String>()
                .is_ok_and(|source| state.drop_entry(&source, &path))
        });
        widget.add_controller(target);
    }
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
        state.callbacks.replace(Some(Callbacks {
            rename: Rc::new(|_, _| true),
            move_entry: Rc::new(move |source, directory| {
                recorded_moves.borrow_mut().push((source, directory));
                true
            }),
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
            rename: Rc::new(|_, _| true),
            move_entry: Rc::new(|_, _| true),
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
        assert!(open.borrow().contains("b"));
        branches.start_drag("source");
        branches.hover("cancelled", true);
        pump(500);
        branches.finish_drag();
        assert!(
            !open.borrow().contains("cancelled")
                && open.borrow().contains("b")
                && open.borrow().contains("already-open")
        );

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
        assert_eq!(
            *moves.borrow(),
            [("source".to_owned(), "destination".to_owned())]
        );
    }
}
