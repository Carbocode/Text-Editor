//! Opt-in instrumentation. The editor path only timestamps and pushes fixed-size
//! records into a bounded lock-free queue. All I/O happens on the transport thread.
use crossbeam_queue::ArrayQueue;
use native_workspace_devtools_protocol::{
    Ack, Batch, Counter, Descriptor, Kind, MAX_RECORDS, Outcome, QUEUE_RECORDS, Record, Session,
    Uuid, VERSION, diagnostics_dir,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Producer(Arc<Inner>);
struct Inner {
    queue: ArrayQueue<Pending>,
    enabled: AtomicBool,
    stopped: AtomicBool,
    dropped: AtomicU64,
    ids: AtomicU64,
    clock: Instant,
    session: Session,
    ui_tick: AtomicU64,
    ui_gap: AtomicU64,
    ui_gap_time: AtomicU64,
}
#[derive(Clone, Copy)]
struct Pending {
    time: u64,
    component: &'static str,
    thread: &'static str,
    name: &'static str,
    kind: Kind,
    outcome: Outcome,
    operation: u64,
    span: u64,
    revision: u64,
    value: u64,
    generation: u64,
}
impl Producer {
    /// Returns immediately; descriptor discovery and connections are off-thread.
    pub fn from_env(frontend: &'static str) -> Option<Self> {
        if std::env::var("NATIVE_WORKSPACE_DEVTOOLS").as_deref() != Ok("1") {
            return None;
        }
        Self::start(frontend)
    }
    pub fn start(frontend: &'static str) -> Option<Self> {
        let producer = Self::buffered(Session::new(frontend), QUEUE_RECORDS);
        producer.emit(
            "producer",
            "main",
            "session.start",
            Kind::Event,
            Outcome::Ok,
            0,
            0,
            0,
            0,
            0,
        );
        let controls = producer.clone();
        let _ = thread::Builder::new()
            .name("devtools-controls".into())
            .spawn(move || controls.controls());
        let transport = producer.clone();
        if thread::Builder::new()
            .name("devtools-transport".into())
            .spawn(move || transport.run())
            .is_err()
        {
            producer.stop();
            return None;
        }
        Some(producer)
    }
    fn buffered(session: Session, capacity: usize) -> Self {
        Self(Arc::new(Inner {
            queue: ArrayQueue::new(capacity),
            enabled: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            ids: AtomicU64::new(1),
            clock: Instant::now(),
            session,
            ui_tick: AtomicU64::new(0),
            ui_gap: AtomicU64::new(0),
            ui_gap_time: AtomicU64::new(0),
        }))
    }
    pub fn enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Relaxed) && !self.0.stopped.load(Ordering::Relaxed)
    }
    pub fn id(&self) -> u64 {
        self.0.ids.fetch_add(1, Ordering::Relaxed)
    }
    pub fn now(&self) -> u64 {
        self.0.clock.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }
    pub fn ui_heartbeat(&self, interval_ns: u64) {
        if !self.enabled() {
            return;
        }
        let now = self.now();
        let previous = self.0.ui_tick.swap(now, Ordering::Relaxed);
        if previous == 0 {
            return;
        }
        let delay = now.saturating_sub(previous).saturating_sub(interval_ns);
        if delay >= 50_000_000 {
            // Keep the recent stall across normal catch-up ticks, until the
            // drop hook has had a chance to associate it with its operation.
            self.0.ui_gap.store(delay, Ordering::Relaxed);
            self.0.ui_gap_time.store(now, Ordering::Relaxed);
            self.emit(
                "frontend",
                "ui",
                "ui.event_loop_delay",
                Kind::Event,
                Outcome::Ok,
                0,
                0,
                0,
                delay,
                0,
            );
        }
    }
    pub fn recent_ui_delay(&self, interval_ns: u64) -> u64 {
        let now = self.now();
        let previous = self.0.ui_tick.load(Ordering::Relaxed);
        if previous == 0 {
            return 0;
        }
        let current = now.saturating_sub(previous).saturating_sub(interval_ns);
        let recent =
            if now.saturating_sub(self.0.ui_gap_time.load(Ordering::Relaxed)) <= interval_ns * 2 {
                self.0.ui_gap.load(Ordering::Relaxed)
            } else {
                0
            };
        current.max(recent)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &self,
        component: &'static str,
        lane: &'static str,
        name: &'static str,
        kind: Kind,
        outcome: Outcome,
        operation: u64,
        span: u64,
        revision: u64,
        value: u64,
        generation: u64,
    ) {
        if !self.enabled() {
            return;
        }
        let event = Pending {
            time: self.now(),
            component,
            thread: lane,
            name,
            kind,
            outcome,
            operation,
            span,
            revision,
            value,
            generation,
        };
        if self.0.queue.push(event).is_err() {
            self.0.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Never joins the transport thread or waits for a network flush.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Relaxed);
    }
    fn set_recording(&self, recording: bool) {
        if self.0.enabled.swap(recording, Ordering::Relaxed) == recording {
            return;
        }
        // A pause in collection must not become a UI stall after resuming.
        self.0.ui_tick.store(0, Ordering::Relaxed);
        self.0.ui_gap.store(0, Ordering::Relaxed);
        self.0.ui_gap_time.store(0, Ordering::Relaxed);
        // Collection transitions bypass the event filter so stopped intervals
        // remain observable. They run on diagnostic threads, never the editor.
        let event = Pending {
            time: self.now(),
            component: "producer",
            thread: "devtools-controls",
            name: if recording {
                "collection.started"
            } else {
                "collection.stopped"
            },
            kind: Kind::Event,
            outcome: Outcome::Ok,
            operation: 0,
            span: 0,
            revision: 0,
            value: u64::from(recording),
            generation: 0,
        };
        if self.0.queue.push(event).is_err() {
            self.0.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn controls(self) {
        use tungstenite::client::IntoClientRequest;
        while !self.0.stopped.load(Ordering::Relaxed) {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let descriptor = Descriptor::read(&diagnostics_dir().join("service.json"))?;
                let mut request =
                    format!("ws://127.0.0.1:{}/v1/live", descriptor.port).into_client_request()?;
                request.headers_mut().insert(
                    "authorization",
                    format!("Bearer {}", descriptor.token).parse()?,
                );
                let address = std::net::SocketAddr::from(([127, 0, 0, 1], descriptor.port));
                let stream =
                    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200))?;
                stream.set_read_timeout(Some(Duration::from_millis(750)))?;
                stream.set_write_timeout(Some(Duration::from_millis(750)))?;
                let (mut socket, _) = tungstenite::client(request, stream)?;
                while !self.0.stopped.load(Ordering::Relaxed) {
                    match socket.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                                && value.get("session").and_then(|v| v.as_str())
                                    == Some(self.0.session.id.to_string().as_str())
                                && let Some(recording) =
                                    value.get("recording").and_then(|v| v.as_bool())
                            {
                                self.set_recording(recording);
                            }
                        }
                        Err(tungstenite::Error::Io(e))
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(e) => return Err(e.into()),
                        _ => {}
                    }
                }
                let _ = socket.close(None);
                Ok(())
            })();
            let _ = result;
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn run(self) {
        let Ok(client) = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_millis(200))
            .timeout(Duration::from_millis(750))
            .build()
        else {
            return;
        };
        let mut sequence = 0;
        let mut pending: Option<Batch> = None;
        let mut failures = 0;
        let mut announced_drops = 0;
        let mut expired_total = 0u64;
        let mut expired_since = 0u64;
        let mut ended = false;
        loop {
            let stopping = self.0.stopped.load(Ordering::Relaxed);
            let mut records = Vec::with_capacity(MAX_RECORDS);
            if pending.is_none() {
                while records.len() < MAX_RECORDS - 3 {
                    let Some(event) = self.0.queue.pop() else {
                        break;
                    };
                    sequence += 1;
                    records.push(Record {
                        sequence: Counter(sequence),
                        monotonic_ns: Counter(event.time),
                        component: event.component.into(),
                        thread: event.thread.into(),
                        name: event.name.into(),
                        kind: event.kind,
                        outcome: event.outcome,
                        operation: Counter(event.operation),
                        span: Counter(event.span),
                        parent_span: Counter(0),
                        generation: Counter(event.generation),
                        revision: Counter(event.revision),
                        value: Counter(event.value),
                    });
                }
                let dropped = self.0.dropped.load(Ordering::Relaxed);
                if dropped > announced_drops {
                    sequence += 1;
                    records.push(self.special(
                        sequence,
                        "producer.queue_full",
                        Kind::Gap,
                        dropped - announced_drops,
                    ));
                    announced_drops = dropped;
                }
                if expired_since != 0 {
                    sequence += 1;
                    records.push(self.special(
                        sequence,
                        "producer.unconfirmed_expired",
                        Kind::Gap,
                        expired_since,
                    ));
                    expired_since = 0;
                }
                if stopping && self.0.queue.is_empty() && !ended {
                    sequence += 1;
                    records.push(self.special(sequence, "session.end", Kind::SessionEnd, 0));
                    ended = true;
                }
                if !records.is_empty() {
                    pending = Some(Batch {
                        version: VERSION,
                        id: Uuid::new_v4(),
                        session: self.0.session.clone(),
                        records,
                        dropped_total: Counter(dropped.saturating_add(expired_total)),
                    });
                }
            }
            // Also poll control while recording is stopped; editor paths never poll.
            if let Ok(descriptor) = Descriptor::read(&diagnostics_dir().join("service.json")) {
                if let Some(batch) = &pending {
                    let response = client
                        .post(format!("{}/v1/batches", descriptor.base_url()))
                        .bearer_auth(&descriptor.token)
                        .json(batch)
                        .send()
                        .and_then(|r| r.error_for_status())
                        .and_then(|r| r.json::<Ack>());
                    if let Ok(ack) = response
                        && ack.version == VERSION
                        && ack.batch == batch.id
                        && ack.committed_sequence >= batch.records.last().unwrap().sequence
                    {
                        self.set_recording(ack.recording);
                        pending = None;
                        failures = 0;
                    } else {
                        failures += 1;
                    }
                } else if let Ok(response) = client
                    .get(format!(
                        "{}/v1/sessions/{}/control",
                        descriptor.base_url(),
                        self.0.session.id
                    ))
                    .bearer_auth(&descriptor.token)
                    .send()
                    && let Ok(control) =
                        response.json::<native_workspace_devtools_protocol::Control>()
                {
                    self.set_recording(control.recording);
                }
            } else {
                failures += 1;
            }
            // Bound retention of unacknowledged data during outages. Sequence holes
            // plus the next durable dropped counter make the loss explicit.
            if failures >= 8 {
                if let Some(batch) = pending.take() {
                    let lost = batch
                        .records
                        .iter()
                        .filter(|r| r.kind != Kind::Gap && r.kind != Kind::SessionEnd)
                        .count() as u64;
                    expired_total = expired_total.saturating_add(lost);
                    expired_since = expired_since.saturating_add(lost);
                }
                failures = 0;
                if stopping {
                    return;
                }
            }
            if stopping && ended && pending.is_none() {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
    fn special(&self, sequence: u64, name: &str, kind: Kind, value: u64) -> Record {
        Record {
            sequence: Counter(sequence),
            monotonic_ns: Counter(self.now()),
            component: "producer".into(),
            thread: "devtools-transport".into(),
            name: name.into(),
            kind,
            outcome: if kind == Kind::Gap {
                Outcome::Incomplete
            } else {
                Outcome::Ok
            },
            operation: Counter(0),
            span: Counter(0),
            parent_span: Counter(0),
            generation: Counter(0),
            revision: Counter(0),
            value: Counter(value),
        }
    }
}
pub use native_workspace_devtools_protocol::{Kind as EventKind, Outcome as EventOutcome};

/// Launch a separately packaged console without running a build on the UI thread.
/// The developer launcher starts the service independently.
pub fn open_console() {
    open_tools();
}

/// Explicit editor action. Bootstrap and process launch happen off the UI thread.
pub fn open_tools() {
    let _ = thread::Builder::new()
        .name("devtools-bootstrap".into())
        .spawn(|| {
            static LAUNCH: std::sync::Mutex<Option<std::process::Child>> =
                std::sync::Mutex::new(None);
            let Ok(mut running) = LAUNCH.lock() else {
                return;
            };
            let Ok(client) = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_millis(500))
                .build()
            else {
                return;
            };
            let ready = || {
                Descriptor::read(&diagnostics_dir().join("service.json"))
                    .ok()
                    .filter(|d| {
                        client
                            .get(format!("{}/v1/health", d.base_url()))
                            .bearer_auth(&d.token)
                            .send()
                            .and_then(|r| r.error_for_status())
                            .and_then(|r| r.json::<native_workspace_devtools_protocol::Health>())
                            .is_ok_and(|h| h.version == VERSION && h.service == d.service)
                    })
            };
            let binary = |name: &str, key: &str| {
                std::env::var_os(key)
                    .map(std::path::PathBuf::from)
                    .or_else(|| {
                        std::env::current_exe().ok().and_then(|p| {
                            p.parent().map(|d| {
                                if cfg!(target_os = "macos") && name == "native-workspace-devtools-ui" {
                                    let helper = d.join("../Helpers/NativeWorkspaceDevTools.app/Contents/MacOS/native-workspace-devtools-ui");
                                    if helper.is_file() { return helper; }
                                }
                                d.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
                            })
                        })
                    })
                    .filter(|p| p.is_file())
                    .or_else(|| {
                        let p = std::path::PathBuf::from("target/debug")
                            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
                        p.is_file().then_some(p)
                    })
            };
            if ready().is_none() {
                let Some(path) = binary(
                    "native-workspace-devtools",
                    "NATIVE_WORKSPACE_DEVTOOLS_SERVICE",
                ) else {
                    return;
                };
                if std::process::Command::new(path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .is_err()
                {
                    return;
                }
                let deadline = Instant::now() + Duration::from_secs(10);
                while ready().is_none() {
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            if running
                .as_mut()
                .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
            {
                if let Some(d) = ready() {
                    let _ = client
                        .post(format!("{}/v1/console/focus", d.base_url()))
                        .bearer_auth(&d.token)
                        .send();
                }
                return;
            }
            if let Some(path) = binary(
                "native-workspace-devtools-ui",
                "NATIVE_WORKSPACE_DEVTOOLS_UI",
            ) {
                *running = std::process::Command::new(path).spawn().ok();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_stall_is_observable_and_collection_pause_is_not_a_stall() {
        let mut p = Producer::buffered(Session::new("test"), 8);
        Arc::get_mut(&mut p.0).unwrap().clock = Instant::now() - Duration::from_secs(2);
        p.0.ui_tick
            .store(p.now() - 1_000_000_000, Ordering::Relaxed);
        p.ui_heartbeat(50_000_000);
        let stall = p.0.queue.pop().unwrap();
        assert_eq!(stall.name, "ui.event_loop_delay");
        assert!(stall.value >= 950_000_000);
        assert!(p.recent_ui_delay(50_000_000) >= 950_000_000);
        p.ui_heartbeat(50_000_000);
        assert!(p.recent_ui_delay(50_000_000) >= 950_000_000);
        p.set_recording(false);
        p.ui_heartbeat(50_000_000);
        assert_eq!(p.0.ui_tick.load(Ordering::Relaxed), 0);
        p.set_recording(true);
        p.ui_heartbeat(50_000_000);
        assert!(p.recent_ui_delay(50_000_000) < 50_000_000);
        assert!(!std::iter::from_fn(|| p.0.queue.pop()).any(|r| r.name == "ui.event_loop_delay"));
    }
    #[test]
    fn saturation_is_bounded_and_never_waits() {
        let p = Producer::buffered(Session::new("test"), 2);
        for _ in 0..10_000 {
            p.emit(
                "core",
                "main",
                "test",
                Kind::Event,
                Outcome::Ok,
                0,
                0,
                0,
                0,
                0,
            );
        }
        assert_eq!(p.0.queue.len(), 2);
        assert_eq!(p.0.dropped.load(Ordering::Relaxed), 9998);
    }
    #[test]
    fn paused_intervals_have_control_records_but_no_editor_events() {
        let p = Producer::buffered(Session::new("test"), 4);
        p.set_recording(false);
        p.emit(
            "core",
            "main",
            "test",
            Kind::Event,
            Outcome::Ok,
            0,
            0,
            0,
            0,
            0,
        );
        p.set_recording(false);
        p.set_recording(true);
        assert_eq!(p.0.queue.len(), 2);
        assert_eq!(p.0.queue.pop().unwrap().name, "collection.stopped");
        assert_eq!(p.0.queue.pop().unwrap().name, "collection.started");
    }

    #[test]
    fn disabled_does_not_timestamp_or_enqueue() {
        let p = Producer::buffered(Session::new("test"), 2);
        p.0.enabled.store(false, Ordering::Relaxed);
        p.emit(
            "core",
            "main",
            "test",
            Kind::Event,
            Outcome::Ok,
            0,
            0,
            0,
            0,
            0,
        );
        assert!(p.0.queue.is_empty());
        assert_eq!(p.0.dropped.load(Ordering::Relaxed), 0);
    }
}
