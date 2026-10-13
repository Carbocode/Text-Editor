use duckdb::{Connection, params};
use native_workspace_devtools_protocol::{
    Ack, Batch, Counter, Kind, MAX_PAGE, Page, Record, Session, SessionSummary, Uuid, VERSION,
};
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct Store {
    pub(crate) connection: Connection,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch("SET threads=2; SET memory_limit='128MB'; SET max_temp_directory_size='256MB'; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false;")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS schema_version(version INTEGER); INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);")?;
        let version: i32 =
            connection.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
        if version != VERSION as i32 {
            return Err("incompatible database version".into());
        }
        connection.execute_batch("CREATE TABLE IF NOT EXISTS sessions(id VARCHAR PRIMARY KEY, metadata VARCHAR NOT NULL, status VARCHAR NOT NULL, committed UBIGINT NOT NULL, records UBIGINT NOT NULL, dropped UBIGINT NOT NULL, recording BOOLEAN NOT NULL);
            CREATE TABLE IF NOT EXISTS events(session VARCHAR NOT NULL, producer VARCHAR NOT NULL, sequence UBIGINT NOT NULL, time_ns UBIGINT NOT NULL, component VARCHAR NOT NULL, lane VARCHAR NOT NULL, name VARCHAR NOT NULL, kind VARCHAR NOT NULL, outcome VARCHAR NOT NULL, operation UBIGINT NOT NULL, span UBIGINT NOT NULL, parent_span UBIGINT NOT NULL, generation UBIGINT NOT NULL, revision UBIGINT NOT NULL, value UBIGINT NOT NULL);
            CREATE TABLE IF NOT EXISTS ingestion_batches(id VARCHAR PRIMARY KEY, session VARCHAR NOT NULL, producer VARCHAR NOT NULL, first_sequence UBIGINT NOT NULL, last_sequence UBIGINT NOT NULL, digest VARCHAR NOT NULL);
            UPDATE sessions SET status='interrupted',recording=false WHERE status='recording';")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS resource_samples(session VARCHAR, time_ms UBIGINT, pid UINTEGER, role VARCHAR, cpu FLOAT, memory UBIGINT, virtual_memory UBIGINT, read_bytes UBIGINT, written_bytes UBIGINT, alive BOOLEAN);
            CREATE TABLE IF NOT EXISTS session_pins(session VARCHAR PRIMARY KEY);")?;
        Ok(Self { connection })
    }
    pub fn reader(&self) -> Result<Self> {
        Ok(Self {
            connection: self.connection.try_clone()?,
        })
    }
    pub fn ingest(&mut self, batch: &Batch) -> Result<Ack> {
        use sha2::{Digest, Sha256};
        batch.validate().map_err(std::io::Error::other)?;
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(batch)?));
        let tx = self.connection.transaction()?;
        let duplicate: Option<(String, u64)> = tx
            .query_row(
                "SELECT digest,last_sequence FROM ingestion_batches WHERE id=?",
                params![batch.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let id = batch.session.id.to_string();
        if let Some((stored, last)) = duplicate {
            if stored != digest {
                return Err("batch_id_collision".into());
            }
            let recording: bool = tx.query_row(
                "SELECT recording FROM sessions WHERE id=?",
                params![id],
                |r| r.get(0),
            )?;
            return Ok(Ack {
                deduplicated: true,
                version: VERSION,
                batch: batch.id,
                committed_sequence: Counter(last),
                recording,
            });
        }
        let existing: Option<(String, u64)> = tx
            .query_row(
                "SELECT metadata,committed FROM sessions WHERE id=?",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let committed = if let Some((metadata, committed)) = existing {
            let original: Session = serde_json::from_str(&metadata)?;
            if original != batch.session {
                return Err("session_identity_mismatch".into());
            }
            committed
        } else {
            tx.execute(
                "INSERT INTO sessions VALUES (?,?,'recording',0,0,0,true)",
                params![id, serde_json::to_string(&batch.session)?],
            )?;
            0
        };
        let first = batch.records.first().unwrap().sequence.0;
        let last = batch.records.last().unwrap().sequence.0;
        if first <= committed {
            return Err("sequence_overlap".into());
        }
        {
            let mut appender = tx.appender("events")?;
            for record in &batch.records {
                appender.append_row(params![
                    id,
                    batch.session.producer.to_string(),
                    record.sequence.0,
                    record.monotonic_ns.0,
                    record.component,
                    record.thread,
                    record.name,
                    serde_json::to_string(&record.kind)?,
                    serde_json::to_string(&record.outcome)?,
                    record.operation.0,
                    record.span.0,
                    record.parent_span.0,
                    record.generation.0,
                    record.revision.0,
                    record.value.0
                ])?;
            }
            appender.flush()?;
        }
        let ended = batch.records.iter().any(|r| r.kind == Kind::SessionEnd);
        tx.execute(
            "INSERT INTO ingestion_batches VALUES (?,?,?,?,?,?)",
            params![
                batch.id.to_string(),
                id,
                batch.session.producer.to_string(),
                first,
                last,
                digest
            ],
        )?;
        tx.execute("UPDATE sessions SET committed=?,records=records+?,dropped=greatest(dropped,?),status=?,recording=CASE WHEN ? THEN false ELSE recording END WHERE id=?",params![last,batch.records.len() as u64,batch.dropped_total.0,if ended {"completed"} else {"recording"},ended,id])?;
        let recording: bool = tx.query_row(
            "SELECT recording FROM sessions WHERE id=?",
            params![id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(Ack {
            deduplicated: false,
            version: VERSION,
            batch: batch.id,
            committed_sequence: Counter(last),
            recording,
        })
    }
    pub fn sessions(&self) -> Result<Vec<SessionSummary>> {
        let mut stmt=self.connection.prepare("SELECT metadata,status,committed,records,dropped,recording,EXISTS(SELECT 1 FROM session_pins WHERE session=sessions.id) AS pinned FROM sessions ORDER BY pinned DESC, CAST(regexp_extract(metadata, '\"started_unix_ms\":\"([0-9]+)\"', 1) AS UBIGINT) DESC, id LIMIT 200")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?;
        let mut sessions: Vec<SessionSummary> = rows
            .map(|r| {
                let (metadata, status, committed, records, dropped, recording, pinned) = r?;
                Ok(SessionSummary {
                    session: serde_json::from_str(&metadata)?,
                    status,
                    committed_sequence: Counter(committed),
                    records: Counter(records),
                    dropped: Counter(dropped),
                    recording,
                    pinned,
                })
            })
            .collect::<Result<_>>()?;
        sessions.sort_by_key(|session| {
            std::cmp::Reverse((session.pinned, session.session.started_unix_ms))
        });
        Ok(sessions)
    }
    pub fn session(&self, id: Uuid) -> Result<Option<SessionSummary>> {
        let row = self.connection.query_row("SELECT metadata,status,committed,records,dropped,recording,EXISTS(SELECT 1 FROM session_pins WHERE session=sessions.id) FROM sessions WHERE id=?", params![id.to_string()], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u64>(2)?,r.get::<_,u64>(3)?,r.get::<_,u64>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?))).optional()?;
        row.map(
            |(metadata, status, committed, records, dropped, recording, pinned)| {
                Ok(SessionSummary {
                    session: serde_json::from_str(&metadata)?,
                    status,
                    committed_sequence: Counter(committed),
                    records: Counter(records),
                    dropped: Counter(dropped),
                    recording,
                    pinned,
                })
            },
        )
        .transpose()
    }
    pub fn page(&self, id: Uuid, after: u64, limit: usize) -> Result<Page> {
        self.filtered_page(id, after, limit, true)
    }
    pub fn filtered_page(
        &self,
        id: Uuid,
        after: u64,
        limit: usize,
        internal: bool,
    ) -> Result<Page> {
        self.query_page(id, after, limit, internal, None)
    }
    /// Filter before pagination so traces are not restricted to the live page.
    pub fn query_page(
        &self,
        id: Uuid,
        after: u64,
        limit: usize,
        internal: bool,
        operation: Option<u64>,
    ) -> Result<Page> {
        let limit = limit.clamp(1, MAX_PAGE);
        let mut stmt=self.connection.prepare("SELECT sequence,time_ns,component,lane,name,kind,outcome,operation,span,parent_span,generation,revision,value FROM events WHERE session=? AND sequence>? AND (?::UBIGINT IS NULL OR operation=?::UBIGINT) AND (? OR (name NOT IN ('command.search_poll','command.workspace_poll') AND name NOT LIKE 'state.%')) ORDER BY sequence LIMIT ?")?;
        let rows = stmt.query_map(
            params![
                id.to_string(),
                after,
                operation,
                operation,
                internal,
                (limit + 1) as u64
            ],
            |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, u64>(7)?,
                    r.get::<_, u64>(8)?,
                    r.get::<_, u64>(9)?,
                    r.get::<_, u64>(10)?,
                    r.get::<_, u64>(11)?,
                    r.get::<_, u64>(12)?,
                ))
            },
        )?;
        let mut records: Vec<Record> = rows
            .map(|r| {
                let (
                    seq,
                    time,
                    component,
                    thread,
                    name,
                    kind,
                    outcome,
                    operation,
                    span,
                    parent,
                    generation,
                    revision,
                    value,
                ) = r?;
                Ok(Record {
                    sequence: Counter(seq),
                    monotonic_ns: Counter(time),
                    component,
                    thread,
                    name,
                    kind: serde_json::from_str(&kind)?,
                    outcome: serde_json::from_str(&outcome)?,
                    operation: Counter(operation),
                    span: Counter(span),
                    parent_span: Counter(parent),
                    generation: Counter(generation),
                    revision: Counter(revision),
                    value: Counter(value),
                })
            })
            .collect::<Result<_>>()?;
        let next = if records.len() > limit {
            records.pop();
            records.last().map(|r| r.sequence)
        } else {
            None
        };
        Ok(Page { records, next })
    }
    pub fn tail(&self, id: Uuid, limit: usize) -> Result<Page> {
        self.filtered_tail(id, limit, true)
    }
    pub fn filtered_tail(&self, id: Uuid, limit: usize, internal: bool) -> Result<Page> {
        self.query_tail(id, limit, internal, None)
    }
    pub fn query_tail(
        &self,
        id: Uuid,
        limit: usize,
        internal: bool,
        operation: Option<u64>,
    ) -> Result<Page> {
        let first: Option<u64> = self.connection.query_row("SELECT min(sequence) FROM (SELECT sequence FROM events WHERE session=? AND (?::UBIGINT IS NULL OR operation=?::UBIGINT) AND (? OR (name NOT IN ('command.search_poll','command.workspace_poll') AND name NOT LIKE 'state.%')) ORDER BY sequence DESC LIMIT ?)", params![id.to_string(),operation,operation,internal,limit.clamp(1,MAX_PAGE) as u64], |r|r.get(0))?;
        self.query_page(
            id,
            first.unwrap_or(1).saturating_sub(1),
            limit,
            internal,
            operation,
        )
    }
    pub fn resource_samples(
        &self,
        id: Uuid,
        after_ms: u64,
    ) -> Result<Vec<native_workspace_devtools_protocol::ResourceSample>> {
        use native_workspace_devtools_protocol::ResourceSample;
        let mut stmt=self.connection.prepare("SELECT session,time_ms,pid,role,cpu,memory,virtual_memory,read_bytes,written_bytes,alive FROM resource_samples WHERE (session=? OR (session=? AND NOT EXISTS(SELECT 1 FROM sessions WHERE id=? AND status='offline'))) AND time_ms>? ORDER BY time_ms DESC LIMIT 1800")?;
        let rows = stmt.query_map(
            params![
                id.to_string(),
                Uuid::nil().to_string(),
                id.to_string(),
                after_ms
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, f32>(4)?,
                    r.get::<_, u64>(5)?,
                    r.get::<_, u64>(6)?,
                    r.get::<_, u64>(7)?,
                    r.get::<_, u64>(8)?,
                    r.get::<_, bool>(9)?,
                ))
            },
        )?;
        let mut samples: Vec<ResourceSample> = rows
            .map(|r| {
                let (session, time, pid, role, cpu, memory, virtual_memory, read, written, alive) =
                    r?;
                Ok(ResourceSample {
                    session: session.parse()?,
                    unix_ms: Counter(time),
                    pid,
                    role,
                    cpu_percent: cpu,
                    memory_bytes: Counter(memory),
                    virtual_bytes: Counter(virtual_memory),
                    read_bytes: Counter(read),
                    written_bytes: Counter(written),
                    alive,
                })
            })
            .collect::<Result<_>>()?;
        samples.reverse();
        Ok(samples)
    }
    pub fn persist_samples(
        &mut self,
        samples: &[native_workspace_devtools_protocol::ResourceSample],
    ) -> Result<()> {
        let tx = self.connection.transaction()?;
        {
            let mut appender = tx.appender("resource_samples")?;
            for sample in samples {
                appender.append_row(params![
                    sample.session.to_string(),
                    sample.unix_ms.0,
                    sample.pid,
                    sample.role,
                    sample.cpu_percent,
                    sample.memory_bytes.0,
                    sample.virtual_bytes.0,
                    sample.read_bytes.0,
                    sample.written_bytes.0,
                    sample.alive
                ])?;
            }
            appender.flush()?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn analysis(&self, id: Uuid) -> Result<native_workspace_devtools_protocol::Analysis> {
        use native_workspace_devtools_protocol::{Analysis, OperationStats, StateValue};
        let mut state_stmt=self.connection.prepare("SELECT name,arg_max(value,sequence),arg_max(revision,sequence) FROM events WHERE session=? AND (name LIKE 'state.%' OR name='document.buffer_bytes') GROUP BY name ORDER BY name")?;
        let state = state_stmt
            .query_map(params![id.to_string()], |r| {
                Ok(StateValue {
                    name: r.get(0)?,
                    value: Counter(r.get(1)?),
                    revision: Counter(r.get(2)?),
                })
            })?
            .collect::<duckdb::Result<_>>()?;
        let mut stats_stmt=self.connection.prepare("SELECT starts.name,count(*),count(*) FILTER(WHERE ends.outcome='\"error\"'),count(*) FILTER(WHERE ends.outcome IN ('\"cancelled\"','\"discarded\"')),quantile_cont((ends.time_ns-starts.time_ns)/1000000.0,0.5),quantile_cont((ends.time_ns-starts.time_ns)/1000000.0,0.95),quantile_cont((ends.time_ns-starts.time_ns)/1000000.0,0.99),max((ends.time_ns-starts.time_ns)/1000000.0) FROM events starts JOIN events ends ON starts.session=ends.session AND starts.producer=ends.producer AND starts.span=ends.span AND ends.kind='\"span_end\"' WHERE starts.session=? AND starts.kind='\"span_start\"' AND ends.time_ns>=starts.time_ns AND ends.outcome<>'\"noop\"' GROUP BY starts.name ORDER BY count(*) DESC LIMIT 60")?;
        let operations = stats_stmt
            .query_map(params![id.to_string()], |r| {
                Ok(OperationStats {
                    name: r.get(0)?,
                    count: Counter(r.get(1)?),
                    errors: Counter(r.get(2)?),
                    cancelled: Counter(r.get(3)?),
                    p50_ms: r.get(4)?,
                    p95_ms: r.get(5)?,
                    p99_ms: r.get(6)?,
                    max_ms: r.get(7)?,
                })
            })?
            .collect::<duckdb::Result<_>>()?;
        Ok(Analysis { state, operations })
    }

    pub fn control(&self, id: Uuid) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT recording FROM sessions WHERE id=?",
            params![id.to_string()],
            |r| r.get(0),
        )?)
    }
    pub fn set_pin(&mut self, id: Uuid, pinned: bool) -> Result<()> {
        let tx = self.connection.transaction()?;
        let id = id.to_string();
        let exists: u64 = tx.query_row(
            "SELECT count(*) FROM sessions WHERE id=?",
            params![id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err("session_not_found".into());
        }
        if pinned {
            let existing: u64 = tx.query_row(
                "SELECT count(*) FROM session_pins WHERE session=?",
                params![id],
                |r| r.get(0),
            )?;
            let count: u64 = tx.query_row("SELECT count(*) FROM session_pins", [], |r| r.get(0))?;
            if existing == 0 {
                if count >= 64 {
                    return Err("pin_limit_reached".into());
                }
                tx.execute("INSERT INTO session_pins VALUES (?)", params![id])?;
            }
        } else {
            tx.execute("DELETE FROM session_pins WHERE session=?", params![id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_control(&self, id: Uuid, recording: bool) -> Result<()> {
        if self.connection.execute(
            "UPDATE sessions SET recording=? WHERE id=? AND status IN ('recording','interrupted')",
            params![recording, id.to_string()],
        )? != 1
        {
            return Err("session_not_controllable".into());
        }
        Ok(())
    }
}
// Avoid depending on SQLite's OptionalExtension: DuckDB has its own query errors.
trait Optional<T> {
    fn optional(self) -> duckdb::Result<Option<T>>;
}
impl<T> Optional<T> for duckdb::Result<T> {
    fn optional(self) -> duckdb::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
