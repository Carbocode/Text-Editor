//! Native GTK terminal presentation; PTY, emulation and tabs live in Session.
use adw::prelude::*;
use gtk::pango;
use gtk::{gdk, glib};
use native_workspace_application::{
    Session, TerminalCommand as Command, TerminalFont, TerminalInput as Input, TerminalKey,
    TerminalSnapshot, terminal_close_confirmation, terminal_font,
};
use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
    rc::Rc,
    sync::Arc,
};

const PADDING_X: f64 = 6.;
const PADDING_Y: f64 = 4.;

#[derive(Clone)]
pub struct TerminalPanel {
    pub root: gtk::Box,
    tabs: gtk::ListBox,
    surface: gtk::DrawingArea,
    status: gtk::Label,
    core: Rc<RefCell<Session>>,
    state: Rc<RefCell<Arc<TerminalSnapshot>>>,
    window: adw::ApplicationWindow,
    #[cfg(debug_assertions)]
    im: gtk::IMMulticontext,
    #[cfg(debug_assertions)]
    keys: gtk::EventControllerKey,
}
/// Pango loads app fonts from files, so the core's faces are cached by content.
fn write_face(path: &Path, face: &[u8]) -> std::io::Result<()> {
    if path.is_file() {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("font cache directory"))?;
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    std::fs::write(&partial, face)?;
    std::fs::rename(partial, path)
}

/// Pango's CoreText font map cannot add files; fonts registered for the process appear in it.
#[cfg(target_os = "macos")]
fn add_face(_: &pango::FontMap, path: &Path) -> Result<(), String> {
    use std::{ffi::c_void, os::unix::ffi::OsStrExt};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            path: *const u8,
            length: isize,
            directory: u8,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterFontsForURL(
            url: *const c_void,
            scope: u32,
            error: *mut *const c_void,
        ) -> u8;
    }
    const PROCESS_SCOPE: u32 = 1;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: the URL is created from a live path buffer and released after registration.
    let registered = unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            0,
        );
        if url.is_null() {
            return Err("invalid font path".into());
        }
        let registered = CTFontManagerRegisterFontsForURL(url, PROCESS_SCOPE, std::ptr::null_mut());
        CFRelease(url);
        registered != 0
    };
    registered
        .then_some(())
        .ok_or_else(|| "CoreText rejected the font".into())
}

#[cfg(not(target_os = "macos"))]
fn add_face(map: &pango::FontMap, path: &Path) -> Result<(), String> {
    use pango::prelude::FontMapExt;
    map.add_font_file(path).map_err(|error| error.to_string())
}

/// The bundled terminal faces with the system fonts as fallbacks for other glyphs.
fn terminal_font_map(font: &TerminalFont) -> pango::FontMap {
    let map: pango::FontMap = pangocairo::FontMap::new();
    let register = || {
        let directory = glib::user_cache_dir().join("native-workspace/fonts");
        for face in font.faces {
            let mut hasher = DefaultHasher::new();
            face.hash(&mut hasher);
            let path = directory.join(format!("terminal-{:016x}.ttf", hasher.finish()));
            let added = write_face(&path, face)
                .map_err(|error| error.to_string())
                .and_then(|()| add_face(&map, &path));
            if let Err(error) = added {
                eprintln!("Bundled terminal font unavailable: {error}");
            }
        }
    };
    // CoreText registration is process-wide and rejects a second registration.
    #[cfg(target_os = "macos")]
    {
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        REGISTERED.call_once(register);
    }
    #[cfg(not(target_os = "macos"))]
    register();
    map
}

fn dispatch(core: &RefCell<Session>, command: Command) {
    let result = core.borrow_mut().terminal_command(command);
    if let Err(error) = result {
        core.borrow_mut().report(error);
    }
}
fn input(core: &RefCell<Session>, state: &RefCell<Arc<TerminalSnapshot>>, input: Input) {
    if let Some(id) = state.borrow().active {
        dispatch(core, Command::Input { id, input });
    }
}

/// The vertical tab column: actions on top, one sidebar row per terminal.
fn tab_column(
    core: &Rc<RefCell<Session>>,
    state: &Rc<RefCell<Arc<TerminalSnapshot>>>,
    tabs: &gtk::ListBox,
) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(180, -1);
    // Expanding tab titles must not claim width from the terminal surface.
    column.set_hexpand(false);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    actions.set_halign(gtk::Align::End);
    actions.set_margin_top(4);
    actions.set_margin_end(4);
    let new = gtk::Button::from_icon_name("list-add-symbolic");
    new.add_css_class("flat");
    new.set_tooltip_text(Some("New terminal"));
    let new_core = core.clone();
    new.connect_clicked(move |_| dispatch(&new_core, Command::New));
    actions.append(&new);
    let activate_core = core.clone();
    let state = state.clone();
    tabs.connect_row_activated(move |_, row| {
        let id = usize::try_from(row.index())
            .ok()
            .and_then(|i| state.borrow().tabs.get(i).map(|tab| tab.id));
        if let Some(id) = id {
            dispatch(&activate_core, Command::Activate(id));
        }
    });
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(tabs));
    column.append(&actions);
    column.append(&scroll);
    column
}

impl TerminalPanel {
    pub fn new(core: Rc<RefCell<Session>>, window: &adw::ApplicationWindow) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_visible(false);
        root.set_size_request(-1, 120);
        let tabs = gtk::ListBox::new();
        tabs.add_css_class("navigation-sidebar");
        let surface = gtk::DrawingArea::new();
        surface.set_widget_name("terminalSurface");
        surface.set_focusable(true);
        surface.set_hexpand(true);
        surface.set_vexpand(true);
        surface.set_content_height(200);
        let font = terminal_font();
        surface.set_font_map(Some(&terminal_font_map(&font)));
        let status = gtk::Label::new(None);
        let state = Rc::new(RefCell::new(core.borrow().terminal_snapshot()));
        let selection = Rc::new(RefCell::new(None::<(usize, usize)>));
        let screen = gtk::Box::new(gtk::Orientation::Vertical, 0);
        screen.append(&surface);
        screen.append(&status);
        root.append(&screen);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        root.append(&tab_column(&core, &state, &tabs));
        {
            let state = state.clone();
            let selection = selection.clone();
            surface.set_draw_func(move |area, cr, _, _| {
                let state = state.borrow();
                let Some(frame) = &state.frame else { return };
                let color = |rgb: u32| {
                    cr.set_source_rgb(
                        f64::from((rgb >> 16) & 255) / 255.,
                        f64::from((rgb >> 8) & 255) / 255.,
                        f64::from(rgb & 255) / 255.,
                    )
                };
                color(frame.background);
                let _ = cr.paint();
                let selected = *selection.borrow();
                let cols = usize::from(frame.cols);
                // Fractional cell widths: snap edges so adjacent backgrounds do not blend.
                let x = |column: usize| (PADDING_X + column as f64 * font.cell_width).round();
                let y = |row: usize| (PADDING_Y + row as f64 * font.cell_height).round();
                for (i, cell) in frame.cells.iter().enumerate() {
                    color(
                        if selected.is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&i)) {
                            0x365880
                        } else {
                            cell.background
                        },
                    );
                    let (column, row) = (i % cols, i / cols);
                    cr.rectangle(
                        x(column),
                        y(row),
                        x(column + 1) - x(column),
                        y(row + 1) - y(row),
                    );
                    let _ = cr.fill();
                }
                let layout = area.create_pango_layout(None);
                let mut description = pango::FontDescription::new();
                // A missing bundled face must still fall back to a monospace font.
                description.set_family(&format!("{}, monospace", font.family));
                description.set_absolute_size(font.size * f64::from(pango::SCALE));
                layout.set_font_description(Some(&description));
                for (i, cell) in frame
                    .cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.text.is_empty() && c.width > 0)
                {
                    let mut text = glib::markup_escape_text(&cell.text).to_string();
                    if cell.bold {
                        text = format!("<b>{text}</b>");
                    }
                    if cell.italic {
                        text = format!("<i>{text}</i>");
                    }
                    if cell.underline {
                        text = format!("<u>{text}</u>");
                    }
                    layout.set_markup(&text);
                    color(cell.foreground);
                    // Place glyphs on the shared baseline, whatever face Pango falls back to.
                    let baseline = f64::from(layout.baseline()) / f64::from(pango::SCALE);
                    cr.move_to(
                        PADDING_X + (i % cols) as f64 * font.cell_width,
                        PADDING_Y + (i / cols) as f64 * font.cell_height + font.baseline - baseline,
                    );
                    pangocairo::functions::show_layout(cr, &layout);
                }
                if let Some([x, y]) = frame.cursor {
                    color(0xffffff);
                    cr.rectangle(
                        PADDING_X + f64::from(x) * font.cell_width,
                        PADDING_Y + f64::from(y) * font.cell_height,
                        font.cell_width,
                        font.cell_height,
                    );
                    let _ = cr.stroke();
                }
            });
        }
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(&surface));
        {
            let core = core.clone();
            let state = state.clone();
            im.connect_commit(move |_, text| input(&core, &state, Input::Text(text.into())));
        }
        let keys = gtk::EventControllerKey::new();
        keys.set_im_context(Some(&im));
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let core = core.clone();
            let state = state.clone();
            let selection = selection.clone();
            let clipboard = surface.clipboard();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
                let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
                let shortcut = if cfg!(target_os = "macos") {
                    modifiers.contains(gdk::ModifierType::META_MASK)
                } else {
                    control && shift
                };
                if shortcut
                    && key
                        .to_unicode()
                        .is_some_and(|c| c.eq_ignore_ascii_case(&'c'))
                {
                    if let (Some((a, b)), Some(frame)) =
                        (*selection.borrow(), state.borrow().frame.as_ref())
                    {
                        let mut text = String::new();
                        for i in a.min(b)..=a.max(b).min(frame.cells.len().saturating_sub(1)) {
                            if i > a.min(b) && i % usize::from(frame.cols) == 0 {
                                text.push('\n');
                            }
                            let cell = &frame.cells[i];
                            if cell.width > 0 {
                                text.push_str(if cell.text.is_empty() {
                                    " "
                                } else {
                                    &cell.text
                                });
                            }
                        }
                        clipboard.set_text(&text);
                    }
                    return glib::Propagation::Stop;
                }
                if shortcut
                    && key
                        .to_unicode()
                        .is_some_and(|c| c.eq_ignore_ascii_case(&'v'))
                {
                    let core = core.clone();
                    let state = state.clone();
                    clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                        if let Ok(Some(text)) = result {
                            input(&core, &state, Input::Paste(text.into()));
                        }
                    });
                    return glib::Propagation::Stop;
                }
                let name = match key {
                    gdk::Key::Return | gdk::Key::KP_Enter => "Enter",
                    gdk::Key::Tab | gdk::Key::ISO_Left_Tab => "Tab",
                    gdk::Key::BackSpace => "Backspace",
                    gdk::Key::Escape => "Escape",
                    gdk::Key::Delete => "Delete",
                    gdk::Key::Home => "Home",
                    gdk::Key::End => "End",
                    gdk::Key::Insert => "Insert",
                    gdk::Key::Page_Up => "PageUp",
                    gdk::Key::Page_Down => "PageDown",
                    gdk::Key::Up => "ArrowUp",
                    gdk::Key::Down => "ArrowDown",
                    gdk::Key::Left => "ArrowLeft",
                    gdk::Key::Right => "ArrowRight",
                    _ => "",
                };
                let text = key.to_unicode().map(|c| c.to_string()).unwrap_or_default();
                let name = if name.is_empty() {
                    if text.is_empty() {
                        key.name().map(|n| n.to_string()).unwrap_or_default()
                    } else {
                        text.clone()
                    }
                } else {
                    name.into()
                };
                let modifiers = u16::from(shift)
                    | (u16::from(control) << 1)
                    | (u16::from(modifiers.contains(gdk::ModifierType::ALT_MASK)) << 2);
                input(
                    &core,
                    &state,
                    Input::Key(TerminalKey {
                        key: name,
                        text,
                        modifiers,
                    }),
                );
                glib::Propagation::Stop
            });
        }
        surface.add_controller(keys.clone());
        let focus = gtk::EventControllerFocus::new();
        {
            let im = im.clone();
            focus.connect_enter(move |_| im.focus_in());
        }
        let focus_im = im.clone();
        focus.connect_leave(move |_| focus_im.focus_out());
        surface.add_controller(focus);
        let drag = gtk::GestureDrag::new();
        let start = Rc::new(RefCell::new((0., 0.)));
        {
            let surface = surface.downgrade();
            let selection = selection.clone();
            let state = state.clone();
            let start = start.clone();
            drag.connect_drag_begin(move |_, x, y| {
                *start.borrow_mut() = (x, y);
                if let Some(area) = surface.upgrade() {
                    area.grab_focus();
                    area.queue_draw();
                }
                if let Some(frame) = &state.borrow().frame {
                    let i = cell_index(x, y, frame.cols, frame.rows);
                    *selection.borrow_mut() = Some((i, i));
                }
            });
        }
        {
            let surface = surface.downgrade();
            let selection = selection.clone();
            let state = state.clone();
            drag.connect_drag_update(move |_, x, y| {
                let (sx, sy) = *start.borrow();
                if let Some(frame) = &state.borrow().frame {
                    let current = *selection.borrow();
                    if let Some((a, _)) = current {
                        let b = cell_index(sx + x, sy + y, frame.cols, frame.rows);
                        *selection.borrow_mut() = Some((a, b));
                    }
                }
                if let Some(area) = surface.upgrade() {
                    area.queue_draw();
                }
            });
        }
        surface.add_controller(drag);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let core = core.clone();
            let state = state.clone();
            scroll.connect_scroll(move |_, _, dy| {
                input(&core, &state, Input::Scroll(if dy < 0. { -3 } else { 3 }));
                glib::Propagation::Stop
            });
        }
        surface.add_controller(scroll);
        {
            let core = core.clone();
            let state = state.clone();
            let last = std::cell::Cell::new((0, 0, 0));
            surface.add_tick_callback(move |area, _| {
                if let Some(id) = state.borrow().active {
                    let font = terminal_font();
                    let cols = ((f64::from(area.width()) - 2. * PADDING_X) / font.cell_width)
                        .clamp(2., 500.) as u16;
                    let rows = ((f64::from(area.height()) - 2. * PADDING_Y) / font.cell_height)
                        .clamp(1., 200.) as u16;
                    if last.get() != (id, cols, rows) {
                        input(&core, &state, Input::Resize { cols, rows });
                        last.set((id, cols, rows));
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        Self {
            root,
            tabs,
            surface,
            status,
            core,
            state,
            window: window.clone(),
            #[cfg(debug_assertions)]
            im,
            #[cfg(debug_assertions)]
            keys,
        }
    }
    pub fn toggle(&self) {
        if self.core.borrow().terminal_snapshot().tabs.is_empty() {
            dispatch(&self.core, Command::New);
        }
        self.root.set_visible(!self.root.is_visible());
        self.refresh();
        self.surface.grab_focus();
    }
    pub fn request_quit(&self) {
        if !self.core.borrow().has_running_terminals() {
            if let Some(app) = self.window.application() {
                app.quit();
            }
            return;
        }
        let prompt = terminal_close_confirmation();
        let dialog = adw::AlertDialog::builder()
            .heading(&prompt.title)
            .body(prompt.body)
            .build();
        dialog.add_responses(&[("cancel", prompt.cancel), ("terminate", prompt.confirm)]);
        dialog.set_response_appearance("terminate", adw::ResponseAppearance::Destructive);
        dialog.set_close_response("cancel");
        let core = self.core.clone();
        let window = self.window.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response != "terminate" {
                return;
            }
            if let Some(window) = window.upgrade() {
                window.set_sensitive(false);
            }
            dispatch(&core, Command::CloseAll { confirmed: true });
            let core = core.clone();
            let window = window.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                core.borrow_mut().poll();
                if core.borrow().has_running_terminals() {
                    return glib::ControlFlow::Continue;
                }
                if let Some(window) = window.upgrade()
                    && let Some(app) = window.application()
                {
                    app.quit();
                }
                glib::ControlFlow::Break
            });
        });
        let parent = self
            .window
            .application()
            .and_then(|app| app.active_window())
            .unwrap_or_else(|| self.window.clone().upcast());
        dialog.present(Some(&parent));
    }
    pub fn refresh(&self) {
        let next = self.core.borrow().terminal_snapshot();
        if next.revision == self.state.borrow().revision {
            return;
        }
        let switched = next.active != self.state.borrow().active;
        if next.tabs != self.state.borrow().tabs || switched {
            while let Some(row) = self.tabs.first_child() {
                self.tabs.remove(&row);
            }
            for tab in &next.tabs {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                row.append(&gtk::Image::from_icon_name("utilities-terminal-symbolic"));
                let title = gtk::Label::new(Some(&tab.title));
                title.set_hexpand(true);
                title.set_xalign(0.);
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                row.append(&title);
                let close = gtk::Button::from_icon_name("window-close-symbolic");
                close.add_css_class("flat");
                close.add_css_class("circular");
                close.set_tooltip_text(Some("Close terminal"));
                let core = self.core.clone();
                let window = self.window.clone();
                let (id, busy) = (tab.id, tab.busy);
                close.connect_clicked(move |_| {
                    if !busy {
                        dispatch(
                            &core,
                            Command::Close {
                                id,
                                confirmed: false,
                            },
                        );
                        return;
                    }
                    let prompt = terminal_close_confirmation();
                    let dialog = adw::AlertDialog::builder()
                        .heading(&prompt.title)
                        .body(prompt.body)
                        .build();
                    dialog
                        .add_responses(&[("cancel", prompt.cancel), ("terminate", prompt.confirm)]);
                    dialog
                        .set_response_appearance("terminate", adw::ResponseAppearance::Destructive);
                    dialog.set_close_response("cancel");
                    let core = core.clone();
                    dialog.connect_response(None, move |_, response| {
                        if response == "terminate" {
                            dispatch(
                                &core,
                                Command::Close {
                                    id,
                                    confirmed: true,
                                },
                            );
                        }
                    });
                    dialog.present(Some(&window));
                });
                row.append(&close);
                let list_row = gtk::ListBoxRow::new();
                list_row.set_child(Some(&row));
                self.tabs.append(&list_row);
                if next.active == Some(tab.id) {
                    self.tabs.select_row(Some(&list_row));
                }
            }
        }
        self.status.set_label(
            next.tabs
                .iter()
                .find(|t| Some(t.id) == next.active)
                .map(|t| t.status.as_str())
                .unwrap_or(""),
        );
        *self.state.borrow_mut() = next;
        if switched {
            self.surface.grab_focus();
        }
        self.surface.queue_draw();
    }
}

#[cfg(debug_assertions)]
pub fn smoke(panel: TerminalPanel, fixture: std::path::PathBuf) {
    use native_workspace_application::AppCommand;
    panel
        .core
        .borrow_mut()
        .dispatch(AppCommand::OpenWorkspace(fixture))
        .expect("smoke workspace");
    panel.root.set_visible(true);
    panel.window.present();
    let mut stage = 0;
    let mut ticks = 0;
    let mut first = 0;
    let mut second = 0;
    glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        ticks += 1;
        assert!(
            ticks < 600,
            "GNOME terminal smoke timed out at stage {stage}"
        );
        panel.core.borrow_mut().poll();
        panel.refresh();
        if panel.core.borrow().snapshot().activity.busy {
            return glib::ControlFlow::Continue;
        }
        let state = panel.core.borrow().terminal_snapshot();
        let text = state
            .frame
            .as_ref()
            .map(|f| f.cells.iter().map(|c| c.text.as_str()).collect::<String>())
            .unwrap_or_default();
        match stage {
            0 => {
                dispatch(&panel.core, Command::New);
                stage = 1;
            }
            1 if state.frame.is_some() => {
                first = state.active.unwrap();
                panel
                    .im
                    .emit_by_name::<()>("commit", &[&"printf 'NATIVE_%s\\n' TERMINAL"]);
                panel.keys.emit_by_name::<bool>(
                    "key-pressed",
                    &[&gdk::Key::Return, &0u32, &gdk::ModifierType::empty()],
                );
                stage = 2;
            }
            2 if text.contains("NATIVE_TERMINAL") => {
                dispatch(&panel.core, Command::New);
                stage = 3;
            }
            3 if state.tabs.len() == 2 && state.frame.is_some() => {
                second = state.active.unwrap();
                assert_ne!(first, second);
                dispatch(&panel.core, Command::Activate(first));
                stage = 4;
            }
            4 if state.active == Some(first) => {
                assert!(text.contains("NATIVE_TERMINAL"));
                assert!(
                    panel
                        .core
                        .borrow_mut()
                        .terminal_command(Command::Close {
                            id: second,
                            confirmed: false
                        })
                        .is_ok(),
                    "Closing an idle shell needs no confirmation"
                );
                dispatch(&panel.core, Command::CloseAll { confirmed: true });
                stage = 5;
            }
            5 if !panel.core.borrow().has_running_terminals() => {
                assert!(state.tabs.is_empty());
                println!(
                    "GNOME native terminal input/render/tab isolation/termination smoke passed"
                );
                if let Some(app) = panel.window.application() {
                    app.quit();
                }
                return glib::ControlFlow::Break;
            }
            _ => {}
        }
        glib::ControlFlow::Continue
    });
}
fn cell_index(x: f64, y: f64, cols: u16, rows: u16) -> usize {
    let font = terminal_font();
    ((y - PADDING_Y) / font.cell_height)
        .max(0.)
        .min(f64::from(rows - 1)) as usize
        * usize::from(cols)
        + ((x - PADDING_X) / font.cell_width)
            .max(0.)
            .min(f64::from(cols - 1)) as usize
}
