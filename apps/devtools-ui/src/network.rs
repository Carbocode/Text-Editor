use eframe::egui;
use native_workspace_devtools_protocol::{
    Analysis, ConsoleHello, Control, Descriptor, Health, MAX_ARCHIVE_BYTES, Page, ResourcePage,
    SessionSummary, Uuid, VERSION, diagnostics_dir, unix_ms,
};
use std::{
    io::Read,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::Duration,
};

pub enum Command {
    Changed,
    Focus,
    Refresh,
    Select(Uuid),
    AutoSelect,
    Follow(bool),
    Internal(bool),
    Paused(bool),
    Page(u64),
    Control(Uuid, bool),
    Export(Uuid, PathBuf),
    Import(PathBuf),
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub sessions: Vec<SessionSummary>,
    pub selected: Option<Uuid>,
    pub page: Option<Page>,
    pub resources: ResourcePage,
    pub analysis: Analysis,
    pub message: String,
    pub connected: bool,
    pub follow: bool,
    pub received_ms: u64,
}
pub fn spawn(context: egui::Context) -> (SyncSender<Command>, Receiver<View>) {
    let (tx, rx) = mpsc::sync_channel(16);
    let (utx, urx) = mpsc::sync_channel(2);
    let events = tx.clone();
    let _ = thread::Builder::new()
        .name("devtools-ui-live".into())
        .spawn(move || listen(events));
    let _ = thread::Builder::new()
        .name("devtools-ui-network".into())
        .spawn(move || worker(rx, utx, context));
    (tx, urx)
}
fn listen(commands: SyncSender<Command>) {
    use tungstenite::client::IntoClientRequest;
    loop {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let d = Descriptor::read(&diagnostics_dir().join("service.json"))?;
            let mut request = format!("ws://127.0.0.1:{}/v1/live", d.port).into_client_request()?;
            request
                .headers_mut()
                .insert("authorization", format!("Bearer {}", d.token).parse()?);
            let stream = std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], d.port)),
                Duration::from_millis(200),
            )?;
            stream.set_read_timeout(Some(Duration::from_secs(1)))?;
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            let (mut socket, _) = tungstenite::client(request, stream)?;
            loop {
                match socket.read() {
                    Ok(tungstenite::Message::Text(text)) => {
                        let focus = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .is_some_and(|value| {
                                value.get("focus").and_then(|v| v.as_bool()) == Some(true)
                            });
                        if matches!(
                            commands.try_send(if focus {
                                Command::Focus
                            } else {
                                Command::Changed
                            }),
                            Err(mpsc::TrySendError::Disconnected(_))
                        ) {
                            return Err("closed".into());
                        }
                        thread::sleep(Duration::from_millis(200));
                    }
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) => {}
                    Err(e) => return Err(e.into()),
                    _ => {}
                }
            }
        })();
        let _ = result;
        thread::sleep(Duration::from_secs(1));
    }
}
fn worker(commands: Receiver<Command>, updates: SyncSender<View>, context: egui::Context) {
    let Ok(client) = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4))
        .build()
    else {
        return;
    };
    let mut selected = std::env::args()
        .skip_while(|arg| arg != "--session")
        .nth(1)
        .and_then(|value| Uuid::parse_str(&value).ok());
    let mut auto = selected.is_none();
    let mut follow = true;
    let mut internal = false;
    let mut after = 0;
    let mut paused = false;
    let mut registered = None;
    let mut previous = View::default();
    let mut message = String::new();
    loop {
        let command = match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(_) => return,
        };
        if matches!(command, Some(Command::Focus)) {
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
            context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            context.request_repaint();
            continue;
        }
        if paused && (command.is_none() || matches!(command, Some(Command::Changed))) {
            continue;
        }
        let result = (|| -> Result<View, Box<dyn std::error::Error>> {
            let d = Descriptor::read(&diagnostics_dir().join("service.json"))?;
            let base = d.base_url();
            let health: Health = client
                .get(format!("{base}/v1/health"))
                .bearer_auth(&d.token)
                .send()?
                .error_for_status()?
                .json()?;
            if health.version != VERSION || health.service != d.service {
                return Err("incompatible service version".into());
            }
            if registered != Some(d.service) {
                client
                    .post(format!("{base}/v1/console"))
                    .bearer_auth(&d.token)
                    .json(&ConsoleHello {
                        pid: std::process::id(),
                    })
                    .send()?
                    .error_for_status()?;
                registered = Some(d.service);
            }
            match command {
                Some(Command::Select(id)) => {
                    selected = Some(id);
                    auto = false;
                    after = 0;
                    follow = false;
                    message.clear();
                }
                Some(Command::AutoSelect) => {
                    auto = true;
                    follow = true;
                    after = 0;
                    message.clear();
                }
                Some(Command::Internal(value)) => {
                    internal = value;
                }
                Some(Command::Follow(value)) => {
                    follow = value;
                    after = 0;
                }
                Some(Command::Page(cursor)) => {
                    follow = false;
                    after = cursor;
                }
                Some(Command::Paused(value)) => paused = value,
                Some(Command::Control(id, value)) => {
                    client
                        .post(format!("{base}/v1/sessions/{id}/control"))
                        .bearer_auth(&d.token)
                        .json(&Control { recording: value })
                        .send()?
                        .error_for_status()?;
                    message = if value {
                        "Collection resumed."
                    } else {
                        "Collection stopped. Recorded data is preserved."
                    }
                    .into();
                }
                Some(Command::Export(id, path)) => {
                    let mut bytes = Vec::new();
                    client
                        .get(format!("{base}/v1/sessions/{id}/export"))
                        .timeout(Duration::from_secs(20))
                        .bearer_auth(&d.token)
                        .send()?
                        .error_for_status()?
                        .take(MAX_ARCHIVE_BYTES + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
                        return Err("archive is too large".into());
                    }
                    use std::io::Write;
                    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
                    let result = (|| -> std::io::Result<()> {
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&temp)?;
                        file.write_all(&bytes)?;
                        file.sync_all()?;
                        std::fs::rename(&temp, &path)
                    })();
                    if result.is_err() {
                        let _ = std::fs::remove_file(temp);
                    }
                    result?;
                    message = "Recording exported.".into();
                }
                Some(Command::Import(path)) => {
                    if std::fs::metadata(&path)?.len() > MAX_ARCHIVE_BYTES {
                        return Err("archive is too large".into());
                    }
                    let id: Uuid = client
                        .post(format!("{base}/v1/import"))
                        .timeout(Duration::from_secs(20))
                        .bearer_auth(&d.token)
                        .body(std::fs::read(path)?)
                        .send()?
                        .error_for_status()?
                        .json()?;
                    selected = Some(id);
                    auto = false;
                    follow = false;
                    after = 0;
                    message = "Archive opened for offline analysis.".into();
                }
                _ => {}
            }
            let sessions: Vec<SessionSummary> = client
                .get(format!("{base}/v1/sessions"))
                .bearer_auth(&d.token)
                .send()?
                .error_for_status()?
                .json()?;
            if auto {
                selected = sessions
                    .iter()
                    .find(|s| s.status == "recording" && s.session.frontend != "synthetic")
                    .map(|s| s.session.id);
            }
            let mut page = None;
            let mut analysis = Analysis::default();
            if let Some(id) = selected {
                page = Some(
                    client
                        .get(format!(
                            "{base}/v1/sessions/{id}/events?after={after}&limit=500&tail={follow}&internal={internal}"
                        ))
                        .bearer_auth(&d.token)
                        .send()?
                        .error_for_status()?
                        .json::<Page>()?,
                );
                analysis = client
                    .get(format!("{base}/v1/sessions/{id}/analysis"))
                    .bearer_auth(&d.token)
                    .send()?
                    .error_for_status()?
                    .json()?;
            }
            let id = selected.unwrap_or(Uuid::nil());
            let historical = sessions
                .iter()
                .find(|s| Some(s.session.id) == selected)
                .is_some_and(|s| matches!(s.status.as_str(), "offline" | "completed"));
            let start = if historical {
                0
            } else {
                unix_ms().saturating_sub(300_000)
            };
            let resources: ResourcePage = client
                .get(format!("{base}/v1/sessions/{id}/resources?after={start}"))
                .bearer_auth(&d.token)
                .send()?
                .error_for_status()?
                .json()?;
            Ok(View {
                sessions,
                selected,
                page,
                resources,
                analysis,
                message: message.clone(),
                connected: true,
                follow,
                received_ms: unix_ms(),
            })
        })();
        let mut view = match result {
            Ok(v) => v,
            Err(e) => {
                let mut view = previous.clone();
                view.connected = false;
                view.message = format!("Connection interrupted: {e}");
                view
            }
        };
        // Time alone is not a reason to repaint an idle view.
        view.received_ms = if view.connected {
            view.resources.collector.sampled_unix_ms.0
        } else {
            previous.received_ms
        };
        if view != previous && updates.try_send(view.clone()).is_ok() {
            previous = view;
            context.request_repaint();
        }
    }
}
