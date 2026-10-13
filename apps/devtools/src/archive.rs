//! Bounded, versioned ZIP/Parquet archives. Only known files are extracted;
//! executable SQL and database/WAL files are never part of the interchange.
use crate::store::{Result, Store};
use duckdb::params;
use native_workspace_devtools_protocol::{MAX_ARCHIVE_BYTES, Session, Uuid, VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    session: Session,
    committed_sequence: String,
    dropped: String,
    status: String,
    files: BTreeMap<String, FileInfo>,
    redactions: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct FileInfo {
    bytes: u64,
    sha256: String,
}
const FILES: [&str; 2] = ["events.parquet", "resource_samples.parquet"];
fn sql_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}
impl Store {
    pub fn export(&mut self, id: Uuid) -> Result<Vec<u8>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(FILES[0]);
        let tx = self.connection.transaction()?;
        let (metadata, committed, dropped, status): (String, u64, u64, String) = tx.query_row(
            "SELECT metadata,committed,dropped,status FROM sessions WHERE id=?",
            params![id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        // UUIDs are parsed before reaching SQL. Paths are generated locally.
        tx.execute_batch(&format!("COPY (SELECT * FROM events WHERE session='{}' AND sequence<={committed} ORDER BY sequence) TO '{}' (FORMAT PARQUET)",id,sql_path(&path)))?;
        let session: Session = serde_json::from_str(&metadata)?;
        let resource_path = dir.path().join(FILES[1]);
        let now = native_workspace_devtools_protocol::unix_ms();
        tx.execute_batch(&format!("COPY (SELECT * FROM resource_samples WHERE session='{}' OR (session='{}' AND time_ms>={} AND time_ms<={now} AND {}) ORDER BY time_ms,pid) TO '{}' (FORMAT PARQUET)",id,Uuid::nil(),session.started_unix_ms.0,session.process_id!=0 && status!="offline",sql_path(&resource_path)))?;
        let resource_data = std::fs::read(resource_path)?;
        let size = std::fs::metadata(&path)?.len();
        if size.saturating_add(resource_data.len() as u64) > MAX_ARCHIVE_BYTES {
            return Err("archive_budget_exceeded".into());
        }
        let data = std::fs::read(path)?;
        let mut files = BTreeMap::new();
        files.insert(
            FILES[0].into(),
            FileInfo {
                bytes: size,
                sha256: format!("{:x}", Sha256::digest(&data)),
            },
        );
        files.insert(
            FILES[1].into(),
            FileInfo {
                bytes: resource_data.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&resource_data)),
            },
        );
        let manifest = Manifest {
            version: VERSION,
            session,
            committed_sequence: committed.to_string(),
            dropped: dropped.to_string(),
            status,
            files,
            redactions: vec![
                "document_content".into(),
                "query_text".into(),
                "paths".into(),
                "free_error_text".into(),
            ],
        };
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", options)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        zip.start_file(FILES[0], options)?;
        zip.write_all(&data)?;
        zip.start_file(FILES[1], options)?;
        zip.write_all(&resource_data)?;
        let bytes = zip.finish()?.into_inner();
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err("archive_budget_exceeded".into());
        }
        tx.commit()?;
        Ok(bytes)
    }
    pub fn import(&mut self, bytes: &[u8]) -> Result<Uuid> {
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err("archive_budget_exceeded".into());
        }
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        if !(2..=FILES.len() + 1).contains(&zip.len()) {
            return Err("unexpected_archive_entries".into());
        }
        let mut names = std::collections::HashSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let file = zip.by_index(i)?;
            if file.name() != "manifest.json" && !FILES.contains(&file.name()) {
                return Err("unsafe_archive_path".into());
            }
            if !names.insert(file.name().to_owned()) {
                return Err("duplicate_archive_entry".into());
            }
            total = total
                .checked_add(file.size())
                .ok_or("archive_budget_exceeded")?;
            if total > MAX_ARCHIVE_BYTES
                || file.is_dir()
                || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err("archive_budget_exceeded".into());
            }
        }
        let manifest: Manifest = {
            let mut file = zip.by_name("manifest.json")?;
            if file.size() > 32 * 1024 {
                return Err("manifest_too_large".into());
            }
            let mut data = Vec::new();
            file.by_ref().take(32 * 1024 + 1).read_to_end(&mut data)?;
            serde_json::from_slice(&data)?
        };
        if manifest.version != VERSION
            || !manifest.session.validate()
            || manifest.files.is_empty()
            || manifest.files.len() > FILES.len()
            || zip.len() != manifest.files.len() + 1
            || manifest
                .files
                .keys()
                .any(|name| !FILES.contains(&name.as_str()))
        {
            return Err("incompatible_archive".into());
        }
        let info = manifest
            .files
            .get(FILES[0])
            .ok_or("missing_archive_table")?;
        let mut data = Vec::new();
        zip.by_name(FILES[0])?
            .take(MAX_ARCHIVE_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 != info.bytes || info.sha256 != format!("{:x}", Sha256::digest(&data))
        {
            return Err("checksum_mismatch".into());
        }
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(FILES[0]);
        std::fs::write(&path, &data)?;
        let resource_path = if let Some(info) = manifest.files.get(FILES[1]) {
            let mut data = Vec::new();
            zip.by_name(FILES[1])?
                .take(MAX_ARCHIVE_BYTES + 1)
                .read_to_end(&mut data)?;
            if data.len() as u64 != info.bytes
                || info.sha256 != format!("{:x}", Sha256::digest(&data))
            {
                return Err("checksum_mismatch".into());
            }
            let path = dir.path().join(FILES[1]);
            std::fs::write(&path, data)?;
            Some(path)
        } else {
            None
        };
        let committed: u64 = manifest.committed_sequence.parse()?;
        let dropped: u64 = manifest.dropped.parse()?;
        let id = Uuid::new_v4();
        let original_id = manifest.session.id;
        let mut session = manifest.session;
        session.id = id;
        let provenance = session
            .capabilities
            .iter()
            .find(|s| s.starts_with("offline.source."))
            .cloned()
            .unwrap_or_else(|| format!("offline.source.{original_id}"));
        session
            .capabilities
            .retain(|s| !s.starts_with("offline.source."));
        if session.capabilities.len() >= 32 {
            return Err("archive_capability_budget_exceeded".into());
        }
        session.capabilities.push(provenance);
        let tx = self.connection.transaction()?;
        let rows: u64 = tx.query_row(
            &format!("SELECT count(*) FROM read_parquet('{}')", sql_path(&path)),
            [],
            |r| r.get(0),
        )?;
        if rows > 1_000_000 {
            return Err("archive_row_budget_exceeded".into());
        }
        tx.execute_batch(&format!(
            "CREATE TEMP TABLE import_events AS SELECT * FROM read_parquet('{}')",
            sql_path(&path)
        ))?;
        let columns: Vec<String> = tx
            .prepare("DESCRIBE import_events")?
            .query_map([], |r| r.get(0))?
            .collect::<duckdb::Result<_>>()?;
        let expected = [
            "session",
            "producer",
            "sequence",
            "time_ns",
            "component",
            "lane",
            "name",
            "kind",
            "outcome",
            "operation",
            "span",
            "parent_span",
            "generation",
            "revision",
            "value",
        ];
        if columns != expected {
            return Err("invalid_archive_schema".into());
        }
        let bad:u64=tx.query_row("SELECT count(*) FROM import_events WHERE session IS NULL OR session<>? OR producer IS NULL OR producer<>? OR sequence IS NULL OR sequence=0 OR sequence>? OR time_ns IS NULL OR component IS NULL OR lane IS NULL OR name IS NULL OR kind IS NULL OR outcome IS NULL OR operation IS NULL OR span IS NULL OR parent_span IS NULL OR generation IS NULL OR revision IS NULL OR value IS NULL OR length(component)>96 OR length(lane)>96 OR length(name)>96 OR NOT regexp_full_match(component,'[A-Za-z0-9_.-]+') OR NOT regexp_full_match(lane,'[A-Za-z0-9_.-]+') OR NOT regexp_full_match(name,'[A-Za-z0-9_.-]+') OR kind NOT IN ('\"event\"','\"span_start\"','\"span_end\"','\"gap\"','\"marker\"','\"session_end\"') OR outcome NOT IN ('\"ok\"','\"error\"','\"noop\"','\"cancelled\"','\"discarded\"','\"incomplete\"')",params![original_id.to_string(),session.producer.to_string(),committed],|r|r.get(0))?;
        let (count, distinct, max): (u64, u64, u64) = tx.query_row(
            "SELECT count(*),count(DISTINCT sequence),coalesce(max(sequence),0) FROM import_events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if bad != 0 || count != distinct || max != committed {
            return Err("invalid_archive_records".into());
        }
        tx.execute(
            "INSERT INTO sessions VALUES (?,?,'offline',?,?,?,false)",
            params![
                id.to_string(),
                serde_json::to_string(&session)?,
                committed,
                count,
                dropped
            ],
        )?;
        tx.execute("INSERT INTO events SELECT ?,producer,sequence,time_ns,component,lane,name,kind,outcome,operation,span,parent_span,generation,revision,value FROM import_events",params![id.to_string()])?;
        if let Some(path) = resource_path {
            let count: u64 = tx.query_row(
                &format!("SELECT count(*) FROM read_parquet('{}')", sql_path(&path)),
                [],
                |r| r.get(0),
            )?;
            if count > 1_000_000 {
                return Err("archive_row_budget_exceeded".into());
            }
            tx.execute_batch(&format!(
                "CREATE TEMP TABLE import_resources AS SELECT * FROM read_parquet('{}')",
                sql_path(&path)
            ))?;
            let columns: Vec<String> = tx
                .prepare("DESCRIBE import_resources")?
                .query_map([], |r| r.get(0))?
                .collect::<duckdb::Result<_>>()?;
            if columns
                != [
                    "session",
                    "time_ms",
                    "pid",
                    "role",
                    "cpu",
                    "memory",
                    "virtual_memory",
                    "read_bytes",
                    "written_bytes",
                    "alive",
                ]
            {
                return Err("invalid_resource_schema".into());
            }
            let invalid:u64=tx.query_row("SELECT count(*) FROM import_resources WHERE session IS NULL OR session NOT IN (?,?) OR time_ms IS NULL OR pid IS NULL OR role IS NULL OR role NOT IN ('editor','collector','console') OR cpu IS NULL OR NOT isfinite(cpu) OR cpu<0 OR memory IS NULL OR virtual_memory IS NULL OR read_bytes IS NULL OR written_bytes IS NULL OR alive IS NULL",params![original_id.to_string(),Uuid::nil().to_string()],|r|r.get(0))?;
            if invalid != 0 {
                return Err("invalid_resource_records".into());
            }
            tx.execute("INSERT INTO resource_samples SELECT ?,time_ms,pid,role,cpu,memory,virtual_memory,read_bytes,written_bytes,alive FROM import_resources",params![id.to_string()])?;
            tx.execute_batch("DROP TABLE import_resources")?;
        }
        tx.execute_batch("DROP TABLE import_events")?;
        tx.commit()?;
        Ok(id)
    }
}
