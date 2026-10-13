//! Versioned local protocol. No document content, paths, queries or free error text.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub use uuid::Uuid;

pub const VERSION: u32 = 1;
pub const OPERATION_QUERY: &str = "events.operation";
pub const SESSION_PIN: &str = "sessions.pin";
pub const SESSION_LOOKUP: &str = "sessions.lookup";
pub const MAX_RECORDS: usize = 256;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;
pub const MAX_PAGE: usize = 500;
pub const QUEUE_RECORDS: usize = 4096;
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Decimal strings preserve the complete 64-bit range in JSON clients.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Counter(#[serde(with = "decimal")] pub u64);
mod decimal {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Event,
    SpanStart,
    SpanEnd,
    Gap,
    Marker,
    SessionEnd,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Error,
    Noop,
    Cancelled,
    Discarded,
    Incomplete,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub sequence: Counter,
    pub monotonic_ns: Counter,
    pub component: String,
    pub thread: String,
    pub name: String,
    pub kind: Kind,
    pub outcome: Outcome,
    pub operation: Counter,
    pub span: Counter,
    pub parent_span: Counter,
    pub generation: Counter,
    pub revision: Counter,
    pub value: Counter,
}
impl Record {
    pub fn validate(&self) -> bool {
        [&self.component, &self.thread, &self.name].iter().all(|s| {
            !s.is_empty()
                && s.len() <= 96
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub producer: Uuid,
    pub process_id: u32,
    pub frontend: String,
    pub platform: String,
    pub build: String,
    pub started_unix_ms: Counter,
    pub clock_uncertainty_ns: Counter,
    pub capabilities: Vec<String>,
}
impl Session {
    pub fn new(frontend: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            producer: Uuid::new_v4(),
            process_id: std::process::id(),
            frontend: frontend.into(),
            platform: std::env::consts::OS.into(),
            build: env!("CARGO_PKG_VERSION").into(),
            started_unix_ms: Counter(unix_ms()),
            clock_uncertainty_ns: Counter(1_000_000),
            capabilities: vec!["dispatch".into(), "search".into(), "snapshot_apply".into()],
        }
    }
    pub fn validate(&self) -> bool {
        [
            self.frontend.as_str(),
            self.platform.as_str(),
            self.build.as_str(),
        ]
        .iter()
        .all(|s| s.len() <= 96 && !s.is_empty())
            && self.capabilities.len() <= 32
            && self.capabilities.iter().all(|s| s.len() <= 96)
    }
}
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub version: u32,
    pub id: Uuid,
    pub session: Session,
    pub records: Vec<Record>,
    pub dropped_total: Counter,
}
impl Batch {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != VERSION {
            return Err("incompatible_version");
        }
        if !self.session.validate() || self.records.is_empty() || self.records.len() > MAX_RECORDS {
            return Err("invalid_batch");
        }
        if !self.records.iter().all(Record::validate)
            || self
                .records
                .windows(2)
                .any(|w| w[0].sequence >= w[1].sequence)
            || self.records[0].sequence.0 == 0
        {
            return Err("invalid_records");
        }
        if serde_json::to_vec(self).map_err(|_| "invalid_batch")?.len() > MAX_BATCH_BYTES {
            return Err("batch_too_large");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {
    #[serde(default)]
    pub deduplicated: bool,
    pub version: u32,
    pub batch: Uuid,
    pub committed_sequence: Counter,
    pub recording: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session: Session,
    pub status: String,
    pub committed_sequence: Counter,
    pub records: Counter,
    pub dropped: Counter,
    pub recording: bool,
    #[serde(default)]
    pub pinned: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub records: Vec<Record>,
    pub next: Option<Counter>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pin {
    pub pinned: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Control {
    pub recording: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Health {
    pub version: u32,
    pub service: Uuid,
    #[serde(default)]
    pub capabilities: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Descriptor {
    pub version: u32,
    pub service: Uuid,
    pub port: u16,
    pub token: String,
}
impl Descriptor {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() || meta.len() > 4096 {
            return Err(std::io::Error::other("invalid descriptor"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(std::io::Error::other("descriptor must be private (0600)"));
            }
        }
        let value: Self = serde_json::from_slice(&fs::read(path)?)?;
        if value.version != VERSION
            || value.port == 0
            || value.token.len() != 64
            || !value.token.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(std::io::Error::other("incompatible descriptor"));
        }
        Ok(value)
    }
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(path)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()
    }
}
pub fn diagnostics_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NATIVE_WORKSPACE_DIAGNOSTICS_DIR") {
        return path.into();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/NativeWorkspace/diagnostics")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("native-workspace/diagnostics")
    }
}

/// Deterministic fixture: cancelled search, successful search, aggregated watcher,
/// explicit loss, and an unfinished span in an interrupted session.
pub fn synthetic() -> Batch {
    let mut session = Session::new("synthetic");
    session.id = Uuid::from_u128(1);
    session.producer = Uuid::from_u128(2);
    session.process_id = 0;
    session.platform = "fixture".into();
    session.started_unix_ms = Counter(1_700_000_000_000);
    session.capabilities.push("watcher.synthetic".into());
    let mut records = Vec::new();
    for (name, kind, outcome, operation, span, time, value) in [
        ("search.queued", Kind::Event, Outcome::Ok, 1, 0, 0, 0),
        (
            "search.debounce",
            Kind::SpanStart,
            Outcome::Ok,
            1,
            1,
            1_000,
            0,
        ),
        (
            "search.superseded",
            Kind::SpanEnd,
            Outcome::Cancelled,
            1,
            1,
            50_000_000,
            0,
        ),
        (
            "search.queued",
            Kind::Event,
            Outcome::Ok,
            2,
            0,
            50_000_001,
            0,
        ),
        (
            "search.debounce",
            Kind::SpanStart,
            Outcome::Ok,
            2,
            2,
            50_000_002,
            0,
        ),
        (
            "search.debounce",
            Kind::SpanEnd,
            Outcome::Ok,
            2,
            2,
            200_000_002,
            0,
        ),
        (
            "search.execute",
            Kind::SpanStart,
            Outcome::Ok,
            2,
            3,
            200_000_003,
            0,
        ),
        (
            "search.execute",
            Kind::SpanEnd,
            Outcome::Ok,
            2,
            3,
            210_000_003,
            12,
        ),
        (
            "search.published",
            Kind::Event,
            Outcome::Ok,
            2,
            0,
            210_000_004,
            12,
        ),
        (
            "snapshot.apply",
            Kind::SpanStart,
            Outcome::Ok,
            2,
            4,
            250_000_000,
            0,
        ),
        (
            "snapshot.apply",
            Kind::SpanEnd,
            Outcome::Ok,
            2,
            4,
            251_000_000,
            0,
        ),
        (
            "watcher.aggregated",
            Kind::Event,
            Outcome::Ok,
            3,
            0,
            260_000_000,
            24,
        ),
        (
            "producer.queue_full",
            Kind::Gap,
            Outcome::Incomplete,
            0,
            0,
            270_000_000,
            8,
        ),
        (
            "search.execute",
            Kind::SpanStart,
            Outcome::Ok,
            4,
            5,
            300_000_000,
            0,
        ),
    ] {
        records.push(Record {
            sequence: Counter(records.len() as u64 + 1),
            monotonic_ns: Counter(time),
            component: if name.starts_with("snapshot") {
                "frontend"
            } else {
                "core"
            }
            .into(),
            thread: if name.starts_with("search") {
                "search-worker"
            } else {
                "main"
            }
            .into(),
            name: name.into(),
            kind,
            outcome,
            operation: Counter(operation),
            span: Counter(span),
            parent_span: Counter(0),
            generation: Counter(operation),
            revision: Counter(2),
            value: Counter(value),
        });
    }
    Batch {
        version: VERSION,
        id: Uuid::from_u128(3),
        session,
        records,
        dropped_total: Counter(8),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_round_trip_and_u64() {
        let mut b = synthetic();
        b.records[0].monotonic_ns = Counter(u64::MAX);
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"18446744073709551615\""));
        assert_eq!(b, serde_json::from_str(&json).unwrap());
        b.validate().unwrap();
    }
    #[test]
    fn rejects_versions_order_and_payloads() {
        let mut b = synthetic();
        b.version = 99;
        assert_eq!(b.validate(), Err("incompatible_version"));
        b.version = VERSION;
        b.records[1].sequence = b.records[0].sequence;
        assert!(b.validate().is_err());
        b = synthetic();
        b.records[0].name = "secret/path".into();
        assert!(b.validate().is_err());
    }
}

/// Resource measurements are made by the collector, never on the editor thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceSample {
    pub session: Uuid,
    pub unix_ms: Counter,
    pub pid: u32,
    pub role: String,
    pub cpu_percent: f32,
    pub memory_bytes: Counter,
    pub virtual_bytes: Counter,
    pub read_bytes: Counter,
    pub written_bytes: Counter,
    pub alive: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectorStatus {
    pub sampled_unix_ms: Counter,
    pub cpu_count: u32,
    pub system_memory_bytes: Counter,
    pub system_used_memory_bytes: Counter,
    pub writer_queue: Counter,
    pub received_records: Counter,
    pub committed_records: Counter,
    pub rejected_batches: Counter,
    pub sampling_gaps: Counter,
    pub database_bytes: Counter,
    pub last_commit_unix_ms: Counter,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourcePage {
    pub samples: Vec<ResourceSample>,
    pub collector: CollectorStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateValue {
    pub name: String,
    pub value: Counter,
    pub revision: Counter,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationStats {
    pub name: String,
    pub count: Counter,
    pub errors: Counter,
    pub cancelled: Counter,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub state: Vec<StateValue>,
    pub operations: Vec<OperationStats>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsoleHello {
    pub pid: u32,
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn legacy_headers_decode_without_claiming_new_capabilities_or_pins() {
        let health: Health = serde_json::from_value(serde_json::json!({
            "version": VERSION, "service": Uuid::from_u128(1)
        }))
        .unwrap();
        assert!(health.capabilities.is_empty());
        let summary = SessionSummary {
            session: synthetic().session,
            status: "recording".into(),
            committed_sequence: Counter(14),
            records: Counter(14),
            dropped: Counter(0),
            recording: true,
            pinned: true,
        };
        let mut value = serde_json::to_value(summary).unwrap();
        value.as_object_mut().unwrap().remove("pinned");
        let legacy: SessionSummary = serde_json::from_value(value).unwrap();
        assert!(!legacy.pinned);
        assert_eq!(legacy.records.0, 14);
    }
}
