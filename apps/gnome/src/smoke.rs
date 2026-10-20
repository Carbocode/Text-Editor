//! Native regression driver, excluded from release builds.
use crate::sidebar::sidebar_entry_widget;
use crate::ui::{SidebarRow, SidebarTab, Ui, update_pending_row};
use adw::prelude::*;
use gtk::glib;
use native_workspace_application::AppCommand;
use native_workspace_domain::DocumentId;
use std::{cell::Cell, path::PathBuf, rc::Rc};

pub(super) struct Widgets<'a> {
    pub ui: &'a Ui,
    pub app: &'a adw::Application,
    pub sidebar_header: &'a adw::HeaderBar,
    pub content_header: &'a adw::HeaderBar,
    pub toggle_sidebar: &'a gtk::ToggleButton,
    pub icons: &'a gtk::IconTheme,
    pub sidebar_icon: &'a str,
    pub files_scroll: &'a gtk::ScrolledWindow,
    pub file_tab: &'a gtk::ToggleButton,
    pub text_tab: &'a gtk::ToggleButton,
    pub editor_scroll: &'a gtk::ScrolledWindow,
    pub overwrite: &'a gtk::Button,
    pub save: &'a gtk::Button,
    pub decrease_text_size: &'a gtk::Button,
    pub increase_text_size: &'a gtk::Button,
    pub split: &'a adw::OverlaySplitView,
    pub sidebar_frame: &'a gtk::Overlay,
    pub sidebar_resize: &'a gtk::Box,
}

fn settle(ui: &Ui) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        ui.poll_core();
        if !ui.core.borrow().snapshot().activity.busy && !ui.snapshot.borrow().search_in_progress {
            break;
        }
        assert!(std::time::Instant::now() < until, "session did not settle");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn dispatch(ui: &Ui, command: AppCommand) {
    ui.dispatch(command);
    settle(ui);
}

pub(super) fn run(widgets: Widgets<'_>) {
    let Widgets {
        ui,
        app,
        sidebar_header,
        content_header,
        toggle_sidebar,
        icons,
        sidebar_icon,
        files_scroll,
        file_tab,
        text_tab,
        editor_scroll,
        overwrite,
        save,
        decrease_text_size,
        increase_text_size,
        split,
        sidebar_frame,
        sidebar_resize,
    } = widgets;
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
        assert_eq!(ui.snapshot.borrow().revision, 0);
        println!("GTK → Rust smoke test passed");
        if std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE").is_none() {
            app.quit();
        }
    }
    if let Some(path) = std::env::var_os("NATIVE_WORKSPACE_SMOKE_WORKSPACE") {
        let workspace_path = PathBuf::from(path);
        dispatch(ui, AppCommand::OpenWorkspace(workspace_path.clone()));
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
        assert_eq!(ui.window.default_width(), 1280);
        assert_eq!(ui.window.default_height(), 800);
        app.activate_action("home", None);
        assert!(ui.home_window.is_visible());
        assert!(!ui.window.is_visible());
        ui.resume.emit_clicked();
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
                    .compute_point(sidebar_frame, &gtk::graphene::Point::new(3.0, y as f32))
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
            ui.window.set_default_size(1280, 800);
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
            ui.switch_sidebar(SidebarTab::Git);
            assert_eq!(
                ui.sidebar_pages.visible_child_name().as_deref(),
                Some("git")
            );
            ui.switch_sidebar(SidebarTab::Files);
        }
        settle(ui);
        let has_git_changes = ui
            .snapshot
            .borrow()
            .git
            .repository
            .as_ref()
            .is_some_and(|repo| !repo.changes.is_empty());
        if has_git_changes {
            ui.switch_sidebar(SidebarTab::Git);
            settle(ui);
            fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
                let mut result = vec![root.clone()];
                let mut child = root.first_child();
                while let Some(widget) = child {
                    result.extend(descendants(&widget));
                    child = widget.next_sibling();
                }
                result
            }
            let split = descendants(ui.git.root.upcast_ref())
                .into_iter()
                .find(|widget| widget.widget_name() == "gitSplit")
                .unwrap()
                .downcast::<gtk::Paned>()
                .unwrap();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while (split.height() == 0 || split.position() == 0)
                && std::time::Instant::now() < until
            {
                glib::MainContext::default().iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if split.height() > 0 {
                assert!(
                    (split.position() as f64 / split.height() as f64 - 0.5).abs() < 0.15,
                    "Git split is not initially half height"
                );
            }
            // Rows ellipsize; content never raises the sidebar's minimum width.
            let (minimum, _, _, _) = ui.git.root.measure(gtk::Orientation::Horizontal, -1);
            let widest = descendants(ui.git.root.upcast_ref())
                .into_iter()
                .filter(|w| w.is_mapped() && w.measure(gtk::Orientation::Horizontal, -1).0 > 240)
                .last()
                .map(|w| {
                    let text = w.downcast_ref::<gtk::Label>().map(|l| l.text());
                    format!("{} {:?} {text:?}", w.type_().name(), w.css_classes())
                });
            assert!(
                minimum <= 240,
                "Git panel needs {minimum}px horizontally: {widest:?}"
            );
            let button = descendants(ui.git.root.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.widget_name() == "gitReview")
                .expect("Git review button");
            button.emit_clicked();
            settle(ui);
            assert!(ui.snapshot.borrow().error.is_none());
            let dialog = gtk::Window::list_toplevels()
                .into_iter()
                .find(|widget| widget.widget_name() == "gitDiffDialog")
                .unwrap()
                .downcast::<gtk::Window>()
                .unwrap();
            let text = descendants(dialog.upcast_ref())
                .into_iter()
                .find(|widget| widget.widget_name() == "gitDiffText")
                .unwrap()
                .downcast::<gtk::TextView>()
                .unwrap();
            let buffer = text.buffer();
            let content = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            assert!(!content.is_empty() && content != "Loading diff…");
            dialog.close();
            ui.switch_sidebar(SidebarTab::Files);
            println!("GTK Git split/graph/review dialog smoke test passed");
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
            ui.window.title().as_deref(),
            ui.snapshot.borrow().workspace_name()
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
            dispatch(ui, AppCommand::QueueSearchQuery("README".into()));
            assert!(ui.sidebar().sidebar_rows.borrow().is_empty());
            dispatch(ui, AppCommand::QueueSearchQuery("phase 1".into()));
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
                Some(ui.snapshot.borrow().search_results[0].snippet.as_str()),
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
            dispatch(ui, AppCommand::QueueSearchQuery("unlikely-no-match".into()));
            assert!(ui.snapshot.borrow().search_results.is_empty());
            assert_eq!(ui.search.text(), "unlikely-no-match");
            dispatch(ui, AppCommand::QueueSearchQuery(String::new()));
            file_tab.set_active(true);
            assert!(ui.snapshot.borrow().search_results.is_empty());
            let document_path = workspace_path.join(id.as_str());
            text_tab.set_active(true);
            std::fs::write(&document_path, "external clean edit").unwrap();
            std::fs::write(workspace_path.join("new-while-hidden.txt"), "new file").unwrap();
            dispatch(ui, AppCommand::RefreshWorkspace);
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
            ui.resume.emit_clicked();
            assert_eq!(
                ui.buffer
                    .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
                "local edit"
            );
            assert!(ui.snapshot.borrow().active_document.as_ref().unwrap().dirty);
            std::fs::write(&document_path, "external conflict").unwrap();
            dispatch(ui, AppCommand::RefreshWorkspace);
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
            // Let the virtualized row bind before activating its inline ui.editor.
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
            assert_eq!(file_stack.visible_child_name().as_deref(), Some("label"));
            ui.sidebars[0]
                .files
                .emit_by_name::<()>("activate", &[&(file_index as u32)]);
            assert!(ui.snapshot.borrow().preview_document.is_none());
            ui.sidebars[0].interactions.begin_rename("notes/todo.md");
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
            println!("GTK double-click keep-open/explicit rename/stem selection smoke test passed");
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
            settle(ui);
            let active = ui.snapshot.borrow().active_document.clone().unwrap();
            assert_eq!(active.id.as_str(), "renamed/todo.md");
            assert_eq!(active.content.as_ref(), "unsaved rename");
            assert!(active.dirty);
            dispatch(ui, AppCommand::SaveDocument(active.id));
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
                    .delay_next_background_job(std::time::Duration::from_secs(1));
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
            assert_eq!(active.content.as_ref(), "unsaved move");
            assert!(active.dirty);
            settle(ui);
            dispatch(
                ui,
                AppCommand::MoveEntry {
                    id: active.id,
                    directory: String::new(),
                },
            );
            dispatch(
                ui,
                AppCommand::SaveDocument(DocumentId::parse("todo.md").unwrap()),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while ui.core.borrow().snapshot().activity.busy && std::time::Instant::now() < deadline
            {
                ui.core.borrow_mut().poll();
                ui.render_current();
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
            assert!(!ui.core.borrow().snapshot().activity.busy);
            assert_eq!(
                std::fs::read_to_string(workspace_path.join("todo.md")).unwrap(),
                "unsaved move"
            );
            println!("GTK drag-and-drop/buffer smoke test passed");
            println!("GTK workspace edit/save smoke test passed");
            for name in ["tab-review-a.txt", "tab-review-b.txt"] {
                dispatch(
                    ui,
                    AppCommand::CreateEntry {
                        id: DocumentId::parse(name).unwrap(),
                        directory: false,
                    },
                );
                dispatch(
                    ui,
                    AppCommand::OpenDocument(DocumentId::parse(name).unwrap()),
                );
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
            dispatch(ui, AppCommand::SaveAllDocuments);
            dispatch(
                ui,
                AppCommand::OpenDocument(DocumentId::parse("tab-review-a.txt").unwrap()),
            );
            ui.buffer.begin_user_action();
            ui.buffer.insert(&mut ui.buffer.end_iter(), "edited");
            ui.buffer.end_user_action();
            assert!(
                ui.buffer.can_undo(),
                "Fixture must contain an undoable edit"
            );
            dispatch(
                ui,
                AppCommand::OpenDocument(DocumentId::parse("tab-review-b.txt").unwrap()),
            );
            assert!(
                !ui.buffer.can_undo(),
                "Undo history leaked between equal document buffers"
            );
            ui.buffer.begin_user_action();
            ui.buffer.insert(&mut ui.buffer.end_iter(), "!");
            ui.buffer.end_user_action();
            dispatch(
                ui,
                AppCommand::RenameEntry {
                    id: DocumentId::parse("tab-review-b.txt").unwrap(),
                    new_name: "tab-review-renamed.txt".into(),
                },
            );
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
            dispatch(ui, AppCommand::OpenDocument(readme.clone()));
            dispatch(ui, AppCommand::OpenDocument(active_id));
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
            dispatch(ui, AppCommand::CloseDocument(readme));
            assert!(
                !readme_indicator.is_visible(),
                "Closed file retained its editor indicator"
            );
            println!("GTK sidebar open/closed file indicator smoke test passed");
            for name in ["preview-one.txt", "preview-two.txt"] {
                dispatch(
                    ui,
                    AppCommand::CreateEntry {
                        id: DocumentId::parse(name).unwrap(),
                        directory: false,
                    },
                );
            }
            file_tab.set_active(true);
            let select_file = |name: &str| {
                let index = ui.sidebars[0]
                    .sidebar_rows
                    .borrow()
                    .iter()
                    .position(|row| matches!(row, SidebarRow::File(id) if id.as_str() == name))
                    .unwrap();
                ui.sidebars[0].file_selection.set_selected(index as u32);
                index as u32
            };
            select_file("preview-one.txt");
            assert_eq!(
                ui.snapshot
                    .borrow()
                    .preview_document
                    .as_ref()
                    .unwrap()
                    .as_str(),
                "preview-one.txt"
            );
            let second = select_file("preview-two.txt");
            assert!(
                !ui.snapshot
                    .borrow()
                    .open_documents
                    .contains(&DocumentId::parse("preview-one.txt").unwrap())
            );
            ui.sidebars[0]
                .files
                .emit_by_name::<()>("activate", &[&second]);
            assert!(
                ui.snapshot.borrow().preview_document.is_none(),
                "Sidebar double click did not keep the preview"
            );
            select_file("preview-one.txt");
            let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
            while std::time::Instant::now() < until {
                glib::MainContext::default().iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            fn find_preview(widget: &gtk::Widget) -> Option<gtk::Widget> {
                if widget.has_css_class("workspace-preview") {
                    return Some(widget.clone());
                }
                let mut child = widget.first_child();
                while let Some(widget) = child {
                    child = widget.next_sibling();
                    if let Some(found) = find_preview(&widget) {
                        return Some(found);
                    }
                }
                None
            }
            let preview_widget =
                find_preview(ui.document_tab_bar.upcast_ref()).expect("native italic preview tab");
            fn title_label(widget: &gtk::Widget) -> Option<gtk::Label> {
                if let Some(label) = widget.downcast_ref::<gtk::Label>()
                    && label.text() == "preview-one.txt"
                {
                    return Some(label.clone());
                }
                let mut child = widget.first_child();
                while let Some(widget) = child {
                    child = widget.next_sibling();
                    if let Some(label) = title_label(&widget) {
                        return Some(label);
                    }
                }
                None
            }
            let label = title_label(&preview_widget).expect("native preview title label");
            assert_eq!(
                label.pango_context().font_description().unwrap().style(),
                gtk::pango::Style::Italic,
                "Native preview title did not render in italics"
            );

            let point = preview_widget
                .compute_point(
                    &ui.document_tab_bar,
                    &gtk::graphene::Point::new(
                        preview_widget.width() as f32 / 2.0,
                        preview_widget.height() as f32 / 2.0,
                    ),
                )
                .unwrap();
            let controllers = ui.document_tab_bar.observe_controllers();
            let double_click = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .filter_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
                .find(|gesture| gesture.name().as_deref() == Some("document-tab-keep-open"))
                .unwrap();
            double_click.emit_by_name::<()>(
                "pressed",
                &[&2i32, &f64::from(point.x()), &f64::from(point.y())],
            );
            assert!(
                ui.snapshot.borrow().preview_document.is_none(),
                "Tab double click did not keep the preview"
            );
            assert!(!preview_widget.has_css_class("workspace-preview"));
            println!(
                "GTK native preview/replacement/italic/double-click keep-open smoke test passed"
            );
            dispatch(
                ui,
                AppCommand::PreviewDocument(DocumentId::parse("README.md").unwrap()),
            );
            if std::env::var_os("NATIVE_WORKSPACE_SMOKE_TAB_SCROLL").is_some() {
                for index in 0..20 {
                    let id = DocumentId::parse(format!("scroll-tab-{index:02}.rs")).unwrap();
                    dispatch(
                        ui,
                        AppCommand::CreateEntry {
                            id: id.clone(),
                            directory: false,
                        },
                    );
                    dispatch(ui, AppCommand::OpenDocument(id));
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
                    scroll.scroll(gtk::gdk::ScrollUnit::Wheel, 0.0, 1.0),
                    glib::Propagation::Proceed
                );
                for (start, delta, boundary) in
                    [(upper - 5.0, 1.0, upper), (lower + 5.0, -1.0, lower)]
                {
                    adjustment.set_value(start);
                    for _ in 0..10 {
                        assert_eq!(
                            scroll.scroll(gtk::gdk::ScrollUnit::Wheel, delta, 0.0),
                            glib::Propagation::Stop
                        );
                    }
                    settle();
                    assert!((adjustment.value() - boundary).abs() < 0.01);
                    scroll.scroll(gtk::gdk::ScrollUnit::Wheel, -delta, 0.0);
                    let step = adjustment.page_size().powf(2.0 / 3.0);
                    assert!((adjustment.value() - (boundary - delta * step)).abs() < 0.01);
                }
                println!(
                    "GTK touchpad and horizontal wheel boundary/reverse/axis lock/kinetic scrolling smoke test passed"
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
        external_file_drops(ui);
        search_and_replace(ui);
        if std::env::var_os("NATIVE_WORKSPACE_SMOKE_RECORD_DROP").is_some() {
            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || app.quit());
        } else {
            app.quit();
        }
    }
}

fn external_file_drops(ui: &Ui) {
    fn target(widget: &gtk::Widget, name: Option<&str>) -> Option<gtk::DropTarget> {
        let found = widget
            .observe_controllers()
            .iter::<glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|object| object.downcast::<gtk::DropTarget>().ok())
            .find(|target| name.is_none_or(|name| target.name().as_deref() == Some(name)));
        if found.is_some() {
            return found;
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(found) = target(&widget, name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    let outside = std::env::temp_dir().join(format!(
        "native-workspace-external-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&outside).unwrap();
    let path = outside.join("external drop ü.txt");
    std::fs::write(&path, "external saved").unwrap();
    let files = gtk::gdk::FileList::from_array(&[gtk::gio::File::for_path(&path)]);
    let value = glib::BoxedValue(files.to_value());
    let open =
        target(ui.window.upcast_ref(), Some("os-files-open")).expect("editor file drop target");
    assert!(open.emit_by_name::<bool>("drop", &[&value, &0f64, &0f64]));
    // The real drop callback dispatches from the next GTK main-loop turn.
    let expected = std::fs::canonicalize(&path).unwrap();
    finish_external_drop(ui, expected.to_str().unwrap());
    let external = ui.snapshot.borrow().active_id().unwrap().clone();
    assert!(external.is_external());
    assert_eq!(
        ui.buffer
            .text(&ui.buffer.start_iter(), &ui.buffer.end_iter(), false),
        "external saved"
    );
    ui.buffer.set_text("external edited");
    let generation = ui.snapshot.borrow().editor_generation;
    let move_target =
        target(ui.sidebar().scroll.upcast_ref(), None).expect("tree file drop target");
    assert!(move_target.emit_by_name::<bool>("drop", &[&value, &0f64, &0f64]));
    finish_external_drop(ui, "external drop ü.txt");
    let snapshot = ui.snapshot.borrow().clone();
    assert_eq!(
        snapshot.active_id().unwrap().as_str(),
        "external drop ü.txt"
    );
    assert_eq!(snapshot.editor_generation, generation);
    assert!(snapshot.active_document.as_ref().unwrap().dirty);
    assert!(!path.exists());
    ui.dispatch(AppCommand::SaveAllDocuments);
    let root = snapshot.workspace_root().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("external drop ü.txt")).unwrap(),
        "external edited"
    );
    // A repeated drop must reject the collision before touching the source.
    std::fs::write(&path, "second source").unwrap();
    assert!(!move_target.emit_by_name::<bool>("drop", &[&value, &0f64, &0f64]));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second source");
    std::fs::remove_dir_all(outside).unwrap();
    println!("GTK external FileList drop/open/move/dirty-buffer/collision smoke test passed");
}

fn finish_external_drop(ui: &Ui, expected: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        glib::MainContext::default().iteration(false);
        ui.poll_core();
        let snapshot = ui.snapshot.borrow();
        if !snapshot.activity.busy
            && snapshot
                .active_id()
                .is_some_and(|id| id.as_str() == expected)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "External drop did not finish: {:?}",
            snapshot.error
        );
        drop(snapshot);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Drive the real Search widgets and the asynchronous preview/apply callbacks.
fn search_and_replace(ui: &Ui) {
    let document = DocumentId::parse("search-panel.txt").unwrap();
    dispatch(
        ui,
        AppCommand::CreateEntry {
            id: document.clone(),
            directory: false,
        },
    );
    dispatch(ui, AppCommand::OpenDocument(document.clone()));
    ui.buffer.set_text("cat CAT Cat concatenate 💡 cat");
    dispatch(ui, AppCommand::SaveDocument(document.clone()));
    ui.switch_sidebar(SidebarTab::Search);
    ui.search.set_text("cat");
    ui.search.emit_by_name::<()>("search-changed", &[]);
    settle(ui);
    ui.search_controls.word.set_active(true);
    settle(ui);
    assert_eq!(ui.snapshot.borrow().search_results.len(), 4);
    ui.search_controls.case.set_active(true);
    settle(ui);
    assert_eq!(ui.snapshot.borrow().search_results.len(), 2);
    ui.search_controls.case.set_active(false);
    ui.search_controls.regex.set_active(true);
    ui.search.set_text("[");
    ui.search.emit_by_name::<()>("search-changed", &[]);
    settle(ui);
    assert!(ui.snapshot.borrow().search_error.is_some());
    ui.search_controls.regex.set_active(false);
    ui.search.set_text("cat");
    ui.search.emit_by_name::<()>("search-changed", &[]);
    settle(ui);
    ui.search_controls.replacement.set_text("dog");
    ui.search_controls.preserve.set_active(true);
    ui.search_controls.all.emit_clicked();
    settle(ui);
    assert_eq!(
        ui.snapshot
            .borrow()
            .replacement_preview
            .as_ref()
            .unwrap()
            .occurrences,
        4
    );
    assert_eq!(
        &*ui.snapshot
            .borrow()
            .active_document
            .as_ref()
            .unwrap()
            .content,
        "cat CAT Cat concatenate 💡 cat"
    );
    let apply = ui
        .search_controls
        .preview
        .borrow()
        .as_ref()
        .unwrap()
        .apply
        .clone();
    apply.emit_clicked();
    settle(ui);
    assert!(ui.snapshot.borrow().replacement_preview.is_none());
    assert!(ui.snapshot.borrow().active_document.as_ref().unwrap().dirty);
    assert_eq!(
        &*ui.snapshot
            .borrow()
            .active_document
            .as_ref()
            .unwrap()
            .content,
        "dog DOG Dog concatenate 💡 dog"
    );
    dispatch(ui, AppCommand::SaveDocument(document.clone()));
    dispatch(ui, AppCommand::QueueSearchQuery("dog".into()));
    ui.search_controls.replacement.set_text("fox");
    ui.search_controls.all.emit_clicked();
    settle(ui);
    ui.buffer.set_text("dog typed after preview");
    let apply = ui
        .search_controls
        .preview
        .borrow()
        .as_ref()
        .unwrap()
        .apply
        .clone();
    apply.emit_clicked();
    settle(ui);
    assert!(ui.snapshot.borrow().error.is_some());
    assert_eq!(
        &*ui.snapshot
            .borrow()
            .active_document
            .as_ref()
            .unwrap()
            .content,
        "dog typed after preview"
    );
    let preview_window = ui
        .search_controls
        .preview
        .borrow()
        .as_ref()
        .unwrap()
        .window
        .clone();
    preview_window.close();
    assert!(ui.snapshot.borrow().replacement_preview.is_none());
    println!("GTK Search options/Unicode/replace preview/stale buffer smoke test passed");
}
