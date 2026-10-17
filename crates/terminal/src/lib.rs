//! PTY and Ghostty adapter. All blocking I/O and emulator handles stay off the UI.
use ghostty_vt::{RenderState, Terminal, TerminalOptions, key, render, style::RgbColor, terminal};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::PathBuf,
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
        self.sender
            .try_send(Message::Input(input))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => "Terminal is busy; retry the input".into(),
                mpsc::TrySendError::Disconnected(_) => "Terminal has exited".into(),
            })
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
    let mut emulator = Emulator::new(80, 24)?;
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
    loop {
        if stopped.load(Ordering::Acquire) {
            return Ok("Terminated".into());
        }
        if let Some(status) = child.child.try_wait()? {
            exited = Some(status);
        }
        match receiver.recv_timeout(FRAME_INTERVAL) {
            Ok(Message::Output(bytes)) => {
                emulator.terminal.vt_write(&bytes);
                dirty = true;
            }
            Ok(Message::Input(input)) => {
                match input {
                    Input::Resize { cols, rows } => {
                        child.master.resize(size(cols, rows))?;
                        emulator.terminal.resize(cols, rows, 0, 0)?;
                    }
                    Input::Scroll(lines) => emulator
                        .terminal
                        .scroll_viewport(terminal::ScrollViewport::Delta(lines as isize)),
                    input => {
                        let bytes = emulator.encode(input)?;
                        // This thread can apply PTY backpressure; Session never waits here.
                        writes.try_send(bytes)?;
                        emulator
                            .terminal
                            .scroll_viewport(terminal::ScrollViewport::Bottom);
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
                publish(&mut emulator, latest)?;
                return Ok(exited.map_or_else(
                    || "Exited".into(),
                    |s| format!("Exited ({})", s.exit_code()),
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok("Terminated".into()),
        }
        for event in emulator.terminal.events() {
            if let terminal::Event::PtyWrite(bytes) = event {
                writes.try_send(bytes)?;
            }
        }
        if dirty && last_frame.elapsed() >= FRAME_INTERVAL {
            publish(&mut emulator, latest)?;
            dirty = false;
            last_frame = Instant::now();
        }
    }
}

fn publish(
    emulator: &mut Emulator,
    latest: &Mutex<Update>,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = Arc::new(emulator.frame()?);
    let title = emulator
        .terminal
        .title()?
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
    Ok(())
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

struct Emulator {
    terminal: Terminal,
    render: RenderState,
    keys: key::Encoder,
}
impl Emulator {
    fn new(cols: u16, rows: u16) -> ghostty_vt::error::Result<Self> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: 8 * 1024 * 1024,
        })?;
        terminal.set_scrollback_max_lines(Some(10_000))?;
        terminal.set_default_fg_color(Some(RgbColor {
            r: 220,
            g: 223,
            b: 228,
        }))?;
        terminal.set_default_bg_color(Some(RgbColor {
            r: 25,
            g: 27,
            b: 31,
        }))?;
        terminal.set_clipboard_write_policy(terminal::ClipboardWritePolicy::Deny);
        Ok(Self {
            terminal,
            render: RenderState::new()?,
            keys: key::Encoder::new()?,
        })
    }

    fn encode(&mut self, input: Input) -> ghostty_vt::error::Result<Vec<u8>> {
        match input {
            Input::Text(text) => Ok(text.into_bytes()),
            Input::Paste(text) => {
                let mut data = text.into_bytes();
                let mut output = vec![0; data.len() + 32];
                let bracketed = self.terminal.mode(terminal::Mode::BRACKETED_PASTE)?;
                let len = ghostty_vt::paste::encode(&mut data, bracketed, &mut output)?;
                output.truncate(len);
                Ok(output)
            }
            Input::Key(input) => {
                let mut event = key::Event::new()?;
                event
                    .set_action(key::Action::Press)
                    .set_key(key_name(&input.key))
                    .set_mods(key::Mods::from_bits_truncate(input.modifiers));
                if !input.text.is_empty() {
                    event.set_utf8(Some(input.text));
                }
                self.keys.set_options_from_terminal(&self.terminal);
                let mut output = Vec::new();
                self.keys.encode_to_vec(&event, &mut output)?;
                Ok(output)
            }
            Input::Resize { .. } | Input::Scroll(_) => Ok(Vec::new()),
        }
    }

    fn frame(&mut self) -> ghostty_vt::error::Result<Frame> {
        let snapshot = self.render.update(&self.terminal)?;
        let colors = snapshot.colors()?;
        let mut rows = render::RowIterator::new()?;
        let mut cells = render::CellIterator::new()?;
        let mut output = Vec::new();
        let mut row_iter = rows.update(&snapshot)?;
        while let Some(row) = row_iter.next() {
            let mut cell_iter = cells.update(row)?;
            while let Some(cell) = cell_iter.next() {
                let style = cell.style()?;
                let mut text = String::new();
                cell.graphemes_utf8(&mut text)?;
                let mut foreground = rgb(cell.fg_color()?.unwrap_or(colors.foreground));
                let mut background = rgb(cell.bg_color()?.unwrap_or(colors.background));
                if style.inverse {
                    std::mem::swap(&mut foreground, &mut background);
                }
                if style.invisible {
                    foreground = background;
                }
                let width = match cell.raw_cell()?.wide()? {
                    ghostty_vt::screen::CellWide::Wide => 2,
                    ghostty_vt::screen::CellWide::SpacerTail => 0,
                    _ => 1,
                };
                output.push(Cell {
                    text,
                    width,
                    foreground,
                    background,
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline != ghostty_vt::style::Underline::None,
                });
            }
        }
        let cursor = snapshot.cursor()?;
        let cursor = if cursor.visible {
            cursor.viewport.map(|v| [v.x, v.y])
        } else {
            None
        };
        let frame = Frame {
            cols: snapshot.cols()?,
            rows: snapshot.rows()?,
            cells: output,
            cursor,
            background: rgb(colors.background),
        };
        snapshot.clean()?;
        Ok(frame)
    }
}

fn rgb(color: RgbColor) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn key_name(name: &str) -> key::Key {
    use key::Key;
    match name {
        "Enter" => Key::Enter,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Escape" => Key::Escape,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        _ => {
            let c = name.chars().next().unwrap_or('\0').to_ascii_lowercase();
            if name.len() == 1 && c.is_ascii_alphabetic() {
                Key::try_from(Key::A as i32 + i32::from(c as u8 - b'a'))
                    .unwrap_or(Key::Unidentified)
            } else {
                Key::Unidentified
            }
        }
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
    fn ghostty_preserves_split_utf8_graphemes_styles_and_cursor_addressing() {
        let mut emulator = Emulator::new(20, 4).unwrap();
        let bytes = "\x1b[31mRED\x1b[0m\r\n界e\u{301}".as_bytes();
        for byte in bytes {
            emulator.terminal.vt_write(&[*byte]);
        }
        let frame = emulator.frame().unwrap();
        assert!(text(&frame).contains("界e\u{301}"));
        assert_eq!(frame.cells[20].width, 2);
        assert_eq!(frame.cells[21].width, 0);
        assert_ne!(frame.cells[0].foreground, frame.cells[4].foreground);
        emulator.terminal.vt_write(b"\x1b[1;1HX");
        assert_eq!(emulator.frame().unwrap().cells[0].text, "X");
    }

    #[test]
    fn key_and_paste_encoding_follow_application_modes_and_resize() {
        let mut emulator = Emulator::new(20, 4).unwrap();
        emulator.terminal.vt_write(b"\x1b[?1h\x1b[?2004h");
        assert_eq!(
            emulator
                .encode(Input::Key(KeyInput {
                    key: "ArrowUp".into(),
                    text: String::new(),
                    modifiers: 0
                }))
                .unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            emulator.encode(Input::Paste("a\nb".into())).unwrap(),
            b"\x1b[200~a\nb\x1b[201~"
        );
        emulator.terminal.vt_write(b"\x1b[?1049hALT");
        assert!(text(&emulator.frame().unwrap()).starts_with("ALT"));
        emulator.terminal.resize(35, 6, 0, 0).unwrap();
        let frame = emulator.frame().unwrap();
        assert_eq!((frame.cols, frame.rows, frame.cells.len()), (35, 6, 210));
        emulator.terminal.vt_write(b"\x1b[?1049l");
        assert!(!text(&emulator.frame().unwrap()).contains("ALT"));
    }

    #[test]
    fn sustained_output_keeps_scrollback_bounded_and_the_latest_screen() {
        let mut emulator = Emulator::new(80, 24).unwrap();
        emulator
            .terminal
            .set_scrollback_max_lines(Some(100))
            .unwrap();
        for _ in 0..1000 {
            emulator
                .terminal
                .vt_write(b"A line of sustained output\r\n");
        }
        emulator.terminal.vt_write(b"LAST_FRAME");
        assert!(emulator.terminal.scrollback_rows().unwrap() <= 100);
        assert!(text(&emulator.frame().unwrap()).contains("LAST_FRAME"));
        emulator
            .terminal
            .scroll_viewport(terminal::ScrollViewport::Delta(-50));
        assert!(!text(&emulator.frame().unwrap()).contains("LAST_FRAME"));
        emulator
            .terminal
            .scroll_viewport(terminal::ScrollViewport::Bottom);
        assert!(text(&emulator.frame().unwrap()).contains("LAST_FRAME"));
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
