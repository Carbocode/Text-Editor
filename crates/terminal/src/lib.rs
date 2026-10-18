//! PTY and Alacritty adapter. All blocking I/O and emulator handles stay off the UI.
use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{self, TermMode, cell::Flags, test::TermSize},
    vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    cell::RefCell,
    io::{Read, Write},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MAX_INPUT: usize = 64 * 1024;
const SCROLLBACK: usize = 10_000;
const FOREGROUND: u32 = 0xdcdfe4;
const BACKGROUND: u32 = 0x191b1f;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Cell {
    pub text: String,
    pub width: u8,
    pub foreground: u32,
    pub background: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Option<[u16; 2]>,
    pub background: u32,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub revision: u64,
    pub title: String,
    pub running: bool,
    /// A program other than the shell owns the terminal's foreground.
    pub busy: bool,
    pub status: String,
    pub frame: Option<Arc<Frame>>,
}

/// Native key name plus produced text; modifiers: Shift=1, Ctrl=2, Alt=4, Super=8.
#[derive(Clone, Debug)]
pub struct KeyInput {
    pub key: String,
    pub text: String,
    pub modifiers: u16,
}

#[derive(Clone, Debug)]
pub enum Input {
    Key(KeyInput),
    Text(String),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    Scroll(i32),
}

enum Message {
    Input(Input),
    Output(Vec<u8>),
    Eof,
    Error(String),
}

/// Dropping a handle requests shutdown; it never joins or waits on the caller.
pub struct Process {
    sender: SyncSender<Message>,
    latest: Arc<Mutex<Update>>,
    stopped: Arc<AtomicBool>,
}

impl Process {
    pub fn spawn(directory: PathBuf) -> Result<Self, String> {
        Self::spawn_command(directory, None)
    }

    fn spawn_command(directory: PathBuf, command: Option<CommandBuilder>) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(64);
        let latest = Arc::new(Mutex::new(Update {
            revision: 0,
            title: "Terminal".into(),
            running: true,
            busy: false,
            status: "Starting…".into(),
            frame: None,
        }));
        let stopped = Arc::new(AtomicBool::new(false));
        let process = Self {
            sender: sender.clone(),
            latest: latest.clone(),
            stopped: stopped.clone(),
        };
        thread::Builder::new()
            .name("terminal".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(directory, command, sender, receiver, &latest, &stopped)
                }));
                let status = match result {
                    Ok(Ok(status)) => status,
                    Ok(Err(error)) => format!("Terminal failed: {error}"),
                    Err(_) => "Terminal worker stopped unexpectedly".into(),
                };
                if let Ok(mut update) = latest.lock() {
                    update.running = false;
                    update.busy = false;
                    update.status = status;
                    update.revision += 1;
                }
            })
            .map_err(|e| format!("Could not start terminal: {e}"))?;
        Ok(process)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    pub fn send(&self, input: Input) -> Result<(), String> {
        let len = match &input {
            Input::Text(s) | Input::Paste(s) => s.len(),
            Input::Key(k) => k.text.len(),
            Input::Resize { cols, rows }
                if !(2..=500).contains(cols) || !(1..=200).contains(rows) =>
            {
                return Err("Terminal size is out of range".into());
            }
            Input::Resize { .. } | Input::Scroll(_) => 0,
        };
        if len > MAX_INPUT {
            return Err("Terminal input exceeds 64 KiB".into());
        }
        match self.sender.try_send(Message::Input(input)) {
            Err(mpsc::TrySendError::Full(_)) => Err("Terminal is busy; retry the input".into()),
            // The worker has exited; its final status is already being published.
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => Ok(()),
        }
    }

    /// No waiting on a worker holding a mutex.
    pub fn poll(&self, previous: u64) -> Option<Update> {
        self.latest
            .try_lock()
            .ok()
            .filter(|u| u.revision != previous)
            .map(|u| u.clone())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

struct ChildGuard {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}
impl ChildGuard {
    /// Job-control shells hand the PTY foreground to the program they run.
    fn busy(&self) -> bool {
        #[cfg(unix)]
        {
            matches!(
                (self.master.process_group_leader(), self.child.process_id()),
                (Some(group), Some(shell)) if u32::try_from(group).ok() != Some(shell)
            )
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        let groups = [
            self.master.process_group_leader(),
            self.child.process_id().map(|p| p as i32),
        ];
        #[cfg(unix)]
        for group in groups.into_iter().flatten().filter(|p| *p > 1) {
            // SAFETY: signal only the PTY child/foreground groups, never our own.
            unsafe {
                if group != libc::getpgrp() {
                    libc::kill(-group, libc::SIGHUP);
                }
            }
        }
        let _ = self.child.kill();
        #[cfg(unix)]
        for group in groups.into_iter().flatten().filter(|p| *p > 1) {
            // SAFETY: same validated process groups; termination was explicitly requested.
            unsafe {
                if group != libc::getpgrp() {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
        }
        let _ = self.child.wait();
    }
}

fn run(
    directory: PathBuf,
    command: Option<CommandBuilder>,
    sender: SyncSender<Message>,
    receiver: Receiver<Message>,
    latest: &Mutex<Update>,
    stopped: &AtomicBool,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut emulator = Emulator::new(80, 24, SCROLLBACK);
    let pair = native_pty_system().openpty(size(80, 24))?;
    if !directory.is_dir() {
        return Err("The workspace directory is no longer available".into());
    }
    let mut command = command.unwrap_or_else(CommandBuilder::new_default_prog);
    command.cwd(directory);
    // Advertise only protocols implemented by our views, including no graphics.
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    let mut child = ChildGuard {
        child: pair.slave.spawn_command(command)?,
        master: pair.master,
    };
    drop(pair.slave);
    let mut reader = child.master.try_clone_reader()?;
    let mut writer = child.master.take_writer()?;
    let (writes, pending_writes) = mpsc::sync_channel::<Vec<u8>>(64);
    let writer_sender = sender.clone();
    thread::Builder::new()
        .name("terminal-writer".into())
        .spawn(move || {
            while let Ok(bytes) = pending_writes.recv() {
                if let Err(error) = writer.write_all(&bytes) {
                    let _ = writer_sender.send(Message::Error(error.to_string()));
                    break;
                }
            }
        })?;
    thread::Builder::new()
        .name("terminal-reader".into())
        .spawn(move || {
            let mut bytes = [0; 16 * 1024];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(Message::Eof);
                        break;
                    }
                    Ok(n) => {
                        if sender.send(Message::Output(bytes[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;
    let mut dirty = true;
    let mut last_frame = Instant::now() - FRAME_INTERVAL;
    let mut exited = None;
    let mut busy = false;
    loop {
        if stopped.load(Ordering::Acquire) {
            return Ok("Terminated".into());
        }
        if let Some(status) = child.child.try_wait()? {
            exited = Some(status);
        }
        match receiver.recv_timeout(FRAME_INTERVAL) {
            Ok(Message::Output(bytes)) => {
                emulator.write(&bytes);
                dirty = true;
            }
            Ok(Message::Input(input)) => {
                match input {
                    Input::Resize { cols, rows } => {
                        child.master.resize(size(cols, rows))?;
                        emulator.resize(cols, rows);
                    }
                    // Positive deltas scroll towards the bottom, as on every native view.
                    Input::Scroll(lines) => emulator.term.scroll_display(Scroll::Delta(-lines)),
                    input => {
                        let bytes = emulator.encode(input);
                        // This thread can apply PTY backpressure; Session never waits here.
                        writes.try_send(bytes)?;
                        emulator.term.scroll_display(Scroll::Bottom);
                    }
                }
                dirty = true;
            }
            Ok(Message::Error(error)) => return Err(error.into()),
            Ok(Message::Eof) => {
                for _ in 0..20 {
                    if exited.is_some() {
                        break;
                    }
                    exited = child.child.try_wait()?;
                    if exited.is_none() {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                publish(&mut emulator, latest);
                return Ok(exited.map_or_else(
                    || "Exited".into(),
                    |s| format!("Exited ({})", s.exit_code()),
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok("Terminated".into()),
        }
        if child.busy() != busy {
            busy = !busy;
            if let Ok(mut update) = latest.lock() {
                update.busy = busy;
                update.revision += 1;
            }
        }
        emulator.finish_sync();
        for reply in emulator.replies() {
            writes.try_send(reply)?;
        }
        if dirty && last_frame.elapsed() >= FRAME_INTERVAL {
            publish(&mut emulator, latest);
            dirty = false;
            last_frame = Instant::now();
        }
    }
}

fn publish(emulator: &mut Emulator, latest: &Mutex<Update>) {
    let frame = Arc::new(emulator.frame());
    let title = emulator
        .title
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect::<String>();
    if let Ok(mut update) = latest.lock() {
        update.frame = Some(frame);
        if !title.is_empty() {
            update.title = title;
        }
        update.status.clear();
        update.revision += 1;
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Collects emulator requests; the worker drains them after each PTY read.
#[derive(Clone, Default)]
struct Events(Rc<RefCell<Vec<Event>>>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

struct Emulator {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    title: String,
}
impl Emulator {
    fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let events = Events::default();
        let config = term::Config {
            scrolling_history: scrollback,
            osc52: term::Osc52::Disabled,
            ..term::Config::default()
        };
        Self {
            term: Term::new(config, &grid_size(cols, rows), events.clone()),
            parser: Processor::new(),
            events,
            title: String::new(),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(grid_size(cols, rows));
    }

    /// Ends a synchronized update whose terminating sequence never arrived.
    fn finish_sync(&mut self) {
        if self
            .parser
            .sync_timeout()
            .sync_timeout()
            .is_some_and(|deadline| deadline <= Instant::now())
        {
            self.parser.stop_sync(&mut self.term);
        }
    }

    /// Applies title changes and returns the bytes the terminal answers with.
    fn replies(&mut self) -> Vec<Vec<u8>> {
        let events = std::mem::take(&mut *self.events.0.borrow_mut());
        let mut replies = Vec::new();
        for event in events {
            match event {
                Event::Title(title) => self.title = title,
                Event::ResetTitle => self.title.clear(),
                Event::PtyWrite(text) => replies.push(text.into_bytes()),
                Event::ColorRequest(index, format) => {
                    let color = self.indexed(index);
                    replies.push(
                        format(Rgb {
                            r: (color >> 16) as u8,
                            g: (color >> 8) as u8,
                            b: color as u8,
                        })
                        .into_bytes(),
                    );
                }
                _ => {}
            }
        }
        replies
    }

    fn encode(&self, input: Input) -> Vec<u8> {
        let mode = *self.term.mode();
        match input {
            Input::Text(text) => text.into_bytes(),
            Input::Paste(text) if mode.contains(TermMode::BRACKETED_PASTE) => {
                // A pasted end marker must not let the clipboard escape the bracket.
                format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
            }
            Input::Paste(text) => text.replace("\r\n", "\r").replace('\n', "\r").into_bytes(),
            Input::Key(input) => encode_key(&input, mode.contains(TermMode::APP_CURSOR)),
            Input::Resize { .. } | Input::Scroll(_) => Vec::new(),
        }
    }

    fn color(&self, color: Color) -> u32 {
        match color {
            Color::Spec(rgb) => pack(rgb),
            Color::Indexed(index) => self.indexed(usize::from(index)),
            Color::Named(name) => self.indexed(name as usize),
        }
    }

    /// Palette changes made by the program (OSC 4/10/11) win over the defaults.
    fn indexed(&self, index: usize) -> u32 {
        self.term.colors()[index].map_or_else(|| default_color(index), pack)
    }

    fn frame(&self) -> Frame {
        let content = self.term.renderable_content();
        let (cols, rows) = (self.term.columns(), self.term.screen_lines());
        let mut cells = Vec::with_capacity(cols * rows);
        for indexed in content.display_iter {
            let cell = indexed.cell;
            let mut foreground = self.color(cell.fg);
            let mut background = self.color(cell.bg);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if cell.flags.contains(Flags::DIM) {
                foreground = dim(foreground);
            }
            if cell.flags.contains(Flags::HIDDEN) {
                foreground = background;
            }
            let spacer = cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let mut text = String::new();
            if !spacer && cell.c != ' ' {
                text.push(cell.c);
                text.extend(cell.zerowidth().into_iter().flatten());
            }
            cells.push(Cell {
                text,
                width: if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    0
                } else {
                    1
                },
                foreground,
                background,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
            });
        }
        let line = content.cursor.point.line.0 + content.display_offset as i32;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && (0..rows as i32).contains(&line))
        .then_some([content.cursor.point.column.0 as u16, line as u16]);
        Frame {
            cols: cols as u16,
            rows: rows as u16,
            cells,
            cursor,
            background: self.indexed(NamedColor::Background as usize),
        }
    }
}

fn grid_size(cols: u16, rows: u16) -> TermSize {
    TermSize::new(usize::from(cols), usize::from(rows))
}

fn pack(rgb: Rgb) -> u32 {
    (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
}

fn dim(rgb: u32) -> u32 {
    let channel = |shift: u32| (((rgb >> shift) & 255) * 2 / 3) << shift;
    channel(16) | channel(8) | channel(0)
}

/// The xterm palette plus the panel's default foreground and background.
fn default_color(index: usize) -> u32 {
    const ANSI: [u32; 16] = [
        0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6, 0x666666,
        0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
    ];
    let level = |v: usize| if v == 0 { 0 } else { 55 + v as u32 * 40 };
    match index {
        0..16 => ANSI[index],
        16..232 => {
            let i = index - 16;
            (level(i / 36) << 16) | (level(i / 6 % 6) << 8) | level(i % 6)
        }
        232..256 => 0x010101 * (8 + 10 * (index - 232) as u32),
        i if i == NamedColor::Background as usize => BACKGROUND,
        i if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
            dim(ANSI[i - NamedColor::DimBlack as usize])
        }
        i if i == NamedColor::DimForeground as usize => dim(FOREGROUND),
        _ => FOREGROUND,
    }
}

/// xterm key encoding; modifiers use KeyInput's Shift=1, Ctrl=2, Alt=4, Super=8 bits.
fn encode_key(input: &KeyInput, app_cursor: bool) -> Vec<u8> {
    let (shift, ctrl, alt) = (
        input.modifiers & 1 != 0,
        input.modifiers & 2 != 0,
        input.modifiers & 4 != 0,
    );
    // xterm's parameter: 1 + Shift(1) + Alt(2) + Ctrl(4) + Meta(8).
    let param = 1
        + u16::from(shift)
        + 2 * u16::from(alt)
        + 4 * u16::from(ctrl)
        + 8 * u16::from(input.modifiers & 8 != 0);
    let cursor = |end: char| {
        if param > 1 {
            format!("\x1b[1;{param}{end}")
        } else if app_cursor {
            format!("\x1bO{end}")
        } else {
            format!("\x1b[{end}")
        }
    };
    let function = |end: char| {
        if param > 1 {
            format!("\x1b[1;{param}{end}")
        } else {
            format!("\x1bO{end}")
        }
    };
    let tilde = |code: u8| {
        if param > 1 {
            format!("\x1b[{code};{param}~")
        } else {
            format!("\x1b[{code}~")
        }
    };
    let prefix = |bytes: &str| {
        if alt {
            format!("\x1b{bytes}")
        } else {
            bytes.into()
        }
    };
    let sequence = match input.key.as_str() {
        "Enter" => prefix("\r"),
        "Tab" if shift => "\x1b[Z".into(),
        "Tab" => prefix("\t"),
        "Backspace" => prefix(if ctrl { "\x08" } else { "\x7f" }),
        "Escape" => prefix("\x1b"),
        "ArrowUp" => cursor('A'),
        "ArrowDown" => cursor('B'),
        "ArrowRight" => cursor('C'),
        "ArrowLeft" => cursor('D'),
        "Home" => cursor('H'),
        "End" => cursor('F'),
        "Insert" => tilde(2),
        "Delete" => tilde(3),
        "PageUp" => tilde(5),
        "PageDown" => tilde(6),
        "F1" => function('P'),
        "F2" => function('Q'),
        "F3" => function('R'),
        "F4" => function('S'),
        "F5" => tilde(15),
        "F6" => tilde(17),
        "F7" => tilde(18),
        "F8" => tilde(19),
        "F9" => tilde(20),
        "F10" => tilde(21),
        "F11" => tilde(23),
        "F12" => tilde(24),
        key => {
            let mut chars = key.chars();
            let control = match (ctrl, chars.next(), chars.next()) {
                (true, Some(c), None) => control_byte(c),
                _ => None,
            };
            match control {
                Some(byte) => prefix(&char::from(byte).to_string()),
                None if input.text.is_empty() || input.text.chars().any(char::is_control) => {
                    // A key without printable text (e.g. a bare modifier) sends nothing.
                    String::new()
                }
                None => prefix(&input.text),
            }
        }
    };
    sequence.into_bytes()
}

/// The C0 byte xterm sends for Ctrl plus a character key.
fn control_byte(key: char) -> Option<u8> {
    match key.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(frame: &Frame) -> String {
        frame
            .cells
            .iter()
            .filter(|c| c.width > 0)
            .map(|c| if c.text.is_empty() { " " } else { &c.text })
            .collect()
    }
    fn wait(process: &Process, done: impl Fn(&Update) -> bool) -> Update {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(update) = process.poll(u64::MAX)
                && done(&update)
            {
                return update;
            }
            assert!(Instant::now() < deadline, "terminal did not complete");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn emulator_preserves_split_utf8_graphemes_styles_and_cursor_addressing() {
        let mut emulator = Emulator::new(20, 4, SCROLLBACK);
        let bytes = "\x1b[31mRED\x1b[0m\r\n界e\u{301}".as_bytes();
        for byte in bytes {
            emulator.write(&[*byte]);
        }
        let frame = emulator.frame();
        assert!(text(&frame).contains("界e\u{301}"));
        assert_eq!(frame.cells[20].width, 2);
        assert_eq!(frame.cells[21].width, 0);
        assert_ne!(frame.cells[0].foreground, frame.cells[4].foreground);
        assert_eq!(frame.cursor, Some([3, 1]));
        emulator.write(b"\x1b[1;1HX");
        assert_eq!(emulator.frame().cells[0].text, "X");
    }

    #[test]
    fn key_and_paste_encoding_follow_application_modes_and_resize() {
        let mut emulator = Emulator::new(20, 4, SCROLLBACK);
        let key = |key: &str, text: &str, modifiers| {
            Input::Key(KeyInput {
                key: key.into(),
                text: text.into(),
                modifiers,
            })
        };
        assert_eq!(emulator.encode(key("ArrowUp", "", 0)), b"\x1b[A");
        assert_eq!(emulator.encode(key("ArrowUp", "", 2)), b"\x1b[1;5A");
        assert_eq!(emulator.encode(key("c", "\x03", 2)), b"\x03");
        assert_eq!(emulator.encode(key("b", "b", 4)), b"\x1bb");
        assert_eq!(emulator.encode(key("Shift_L", "", 1)), b"");
        assert_eq!(emulator.encode(Input::Paste("a\nb".into())), b"a\rb");
        emulator.write(b"\x1b[?1h\x1b[?2004h");
        assert_eq!(emulator.encode(key("ArrowUp", "", 0)), b"\x1bOA");
        assert_eq!(
            emulator.encode(Input::Paste("a\nb\x1b[201~".into())),
            b"\x1b[200~a\nb[201~\x1b[201~"
        );
        emulator.write(b"\x1b[?1049hALT");
        assert!(text(&emulator.frame()).starts_with("ALT"));
        emulator.resize(35, 6);
        let frame = emulator.frame();
        assert_eq!((frame.cols, frame.rows, frame.cells.len()), (35, 6, 210));
        emulator.write(b"\x1b[?1049l");
        assert!(!text(&emulator.frame()).contains("ALT"));
    }

    #[test]
    fn requests_are_answered_and_titles_published_without_osc52_clipboard_access() {
        let mut emulator = Emulator::new(20, 4, SCROLLBACK);
        emulator.write(b"\x1b]0;build\x07\x1b[6n\x1b]11;?\x07\x1b]52;c;aGk=\x07");
        let replies = emulator.replies();
        assert_eq!(emulator.title, "build");
        assert!(replies.contains(&b"\x1b[1;1R".to_vec()));
        assert!(
            replies
                .iter()
                .any(|r| r.starts_with(b"\x1b]11;rgb:1919/1b1b/1f1f"))
        );
        assert_eq!(replies.len(), 2);
    }

    #[test]
    fn sustained_output_keeps_scrollback_bounded_and_the_latest_screen() {
        let mut emulator = Emulator::new(80, 24, 100);
        for _ in 0..1000 {
            emulator.write(b"A line of sustained output\r\n");
        }
        assert_eq!(emulator.term.history_size(), 100);
        for _ in 0..1000 {
            emulator.write(b"More sustained output\r\n");
        }
        assert_eq!(emulator.term.history_size(), 100);
        emulator.write(b"LAST_FRAME");
        assert!(text(&emulator.frame()).contains("LAST_FRAME"));
        emulator.term.scroll_display(Scroll::Delta(50));
        let frame = emulator.frame();
        assert!(!text(&frame).contains("LAST_FRAME"));
        assert_eq!(frame.cursor, None);
        emulator.term.scroll_display(Scroll::Bottom);
        assert!(text(&emulator.frame()).contains("LAST_FRAME"));
    }

    #[test]
    fn real_pty_is_interactive_keeps_final_output_and_reports_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args([
            "-c",
            "stty -echo; printf READY; read answer; printf '\\r\\nRESULT:%s' \"$answer\"; exit 7",
        ]);
        let process = Process::spawn_command(dir.path().into(), Some(command)).unwrap();
        wait(&process, |u| {
            u.frame.as_ref().is_some_and(|f| text(f).contains("READY"))
        });
        process
            .send(Input::Text("independent input\r".into()))
            .unwrap();
        let final_state = wait(&process, |u| !u.running);
        assert!(text(final_state.frame.as_ref().unwrap()).contains("RESULT:independent input"));
        assert!(final_state.status.contains('7'));
    }

    #[test]
    fn foreground_programs_mark_an_interactive_shell_busy() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-i");
        command.env("PS1", "READY$ ");
        let process = Process::spawn_command(dir.path().into(), Some(command)).unwrap();
        let idle = wait(&process, |u| {
            u.frame.as_ref().is_some_and(|f| text(f).contains("READY$"))
        });
        assert!(!idle.busy);
        process.send(Input::Text("sleep 30\r".into())).unwrap();
        wait(&process, |u| u.busy);
        process.send(Input::Text("\x03".into())).unwrap();
        assert!(wait(&process, |u| !u.busy).running);
    }

    #[test]
    fn failed_spawn_is_published_and_termination_does_not_wait_on_ui() {
        let dir = tempfile::tempdir().unwrap();
        let process = Process::spawn_command(
            dir.path().into(),
            Some(CommandBuilder::new("/missing/native-workspace-shell")),
        )
        .unwrap();
        assert!(wait(&process, |u| !u.running).status.contains("failed"));
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; printf READY; exec sleep 30"]);
        let process = Process::spawn_command(dir.path().into(), Some(command)).unwrap();
        wait(&process, |u| {
            u.frame.as_ref().is_some_and(|f| text(f).contains("READY"))
        });
        let start = Instant::now();
        process.stop();
        assert!(start.elapsed() < Duration::from_millis(50));
        wait(&process, |u| !u.running);
    }
}
