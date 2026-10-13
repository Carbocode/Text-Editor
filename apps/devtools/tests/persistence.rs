use native_workspace_devtools::store::Store;
use native_workspace_devtools_protocol::{Counter, Uuid, synthetic};

#[test]
fn commit_retry_recovery_and_page_cursors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytics.duckdb");
    let batch = synthetic();
    {
        let mut store = Store::open(&path).unwrap();
        let ack = store.ingest(&batch).unwrap();
        assert_eq!(ack.committed_sequence, Counter(14));
        assert_eq!(
            store.ingest(&batch).unwrap().committed_sequence,
            ack.committed_sequence
        );
        let sessions = store.sessions().unwrap();
        assert_eq!(sessions[0].records, Counter(14));
        let first = store.page(batch.session.id, 0, 3).unwrap();
        assert_eq!(first.next, Some(Counter(3)));
        let second = store
            .page(batch.session.id, first.next.unwrap().0, 3)
            .unwrap();
        assert_eq!(second.records[0].sequence, Counter(4));
        let mut collision = batch.clone();
        collision.records[0].name = "different".into();
        assert!(store.ingest(&collision).is_err());
        let mut overlap = batch.clone();
        overlap.id = Uuid::new_v4();
        assert!(store.ingest(&overlap).is_err());
        assert_eq!(store.sessions().unwrap()[0].records, Counter(14));
    }
    let store = Store::open(&path).unwrap();
    let session = &store.sessions().unwrap()[0];
    assert_eq!(session.status, "interrupted");
    assert_eq!(session.records, Counter(14));
    assert_eq!(
        store.page(batch.session.id, 0, 500).unwrap().records,
        batch.records
    );
}
#[test]
fn parquet_round_trip_during_live_recording_preserves_correlations() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db")).unwrap();
    let batch = synthetic();
    store.ingest(&batch).unwrap();
    let bytes = store.export(batch.session.id).unwrap();
    let imported = store.import(&bytes).unwrap();
    assert_ne!(imported, batch.session.id);
    assert_eq!(store.page(imported, 0, 500).unwrap().records, batch.records);
    let sessions = store.sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.session.id == imported)
            .unwrap()
            .status,
        "offline"
    );
    assert!(store.set_control(imported, true).is_err());
    let another = store.import(&bytes).unwrap();
    assert_ne!(another, imported);
    assert_eq!(store.sessions().unwrap().len(), 3);
}
#[test]
fn rejects_checksum_and_zip_traversal_without_changing_data() {
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db")).unwrap();
    let batch = synthetic();
    store.ingest(&batch).unwrap();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("../escape", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"x").unwrap();
    zip.start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    assert!(store.import(&zip.finish().unwrap().into_inner()).is_err());
    let bytes = store.export(batch.session.id).unwrap();
    let mut original = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut changed = ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["manifest.json", "events.parquet"] {
        let mut file = original.by_name(name).unwrap();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut data).unwrap();
        if name == "events.parquet" {
            data[0] ^= 1;
        }
        changed
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        changed.write_all(&data).unwrap();
    }
    assert!(
        store
            .import(&changed.finish().unwrap().into_inner())
            .is_err()
    );
    assert_eq!(store.sessions().unwrap().len(), 1);
    // A failed import must leave the writer usable for the next valid archive.
    let bytes = store.export(batch.session.id).unwrap();
    assert!(store.import(&bytes).is_ok());
}
#[test]
fn rejects_invalid_records_and_identity_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db")).unwrap();
    let mut batch = synthetic();
    batch.version = 99;
    assert!(store.ingest(&batch).is_err());
    assert!(store.sessions().unwrap().is_empty());
    batch = synthetic();
    store.ingest(&batch).unwrap();
    batch.id = Uuid::new_v4();
    batch.session.producer = Uuid::new_v4();
    for r in &mut batch.records {
        r.sequence.0 += 14;
    }
    assert!(store.ingest(&batch).is_err());
    assert_eq!(store.sessions().unwrap()[0].records, Counter(14));
}

#[test]
fn tail_follows_new_events_and_keeps_user_activity_past_timer_noise() {
    use native_workspace_devtools_protocol::{Kind, Outcome};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db")).unwrap();
    let mut batch = synthetic();
    store.ingest(&batch).unwrap();
    let base = batch.records[0].clone();
    batch.id = Uuid::new_v4();
    batch.records = (15..=214)
        .map(|sequence| {
            let mut r = base.clone();
            r.sequence = Counter(sequence);
            r.name = "command.search_poll".into();
            r.kind = Kind::SpanEnd;
            r.outcome = Outcome::Noop;
            r
        })
        .collect();
    store.ingest(&batch).unwrap();
    let page = store.filtered_tail(batch.session.id, 10, false).unwrap();
    assert_eq!(page.records.last().unwrap().sequence.0, 14);
    assert!(page.records.iter().all(|r| r.name != "command.search_poll"));
    batch.id = Uuid::new_v4();
    batch.records = vec![base];
    batch.records[0].sequence = Counter(215);
    batch.records[0].name = "document.saved".into();
    store.ingest(&batch).unwrap();
    let page = store.filtered_tail(batch.session.id, 10, false).unwrap();
    assert_eq!(page.records.last().unwrap().name, "document.saved");
    assert_eq!(page.records.len(), 10);
}
#[test]
fn resources_analysis_and_offline_namespace_round_trip() {
    use native_workspace_devtools_protocol::ResourceSample;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db")).unwrap();
    let batch = synthetic();
    store.ingest(&batch).unwrap();
    let sample = ResourceSample {
        session: batch.session.id,
        unix_ms: Counter(1_700_000_000_001),
        pid: 123,
        role: "editor".into(),
        cpu_percent: 12.5,
        memory_bytes: Counter(10_000_000),
        virtual_bytes: Counter(20_000_000),
        read_bytes: Counter(1024),
        written_bytes: Counter(2048),
        alive: true,
    };
    store.persist_samples(&[sample.clone()]).unwrap();
    let read = store.resource_samples(batch.session.id, 0).unwrap();
    assert_eq!(read, vec![sample.clone()]);
    let stats = store.analysis(batch.session.id).unwrap();
    let debounce = stats
        .operations
        .iter()
        .find(|s| s.name == "search.debounce")
        .unwrap();
    assert_eq!(debounce.count.0, 2);
    assert_eq!(debounce.cancelled.0, 1);
    assert!((debounce.p50_ms - 100.0).abs() < 0.01);
    let bytes = store.export(batch.session.id).unwrap();
    let imported = store.import(&bytes).unwrap();
    let read = store.resource_samples(imported, 0).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].session, imported);
    assert_eq!(read[0].cpu_percent, 12.5);
    let global = ResourceSample {
        session: Uuid::nil(),
        pid: 777,
        role: "collector".into(),
        ..sample
    };
    store.persist_samples(&[global]).unwrap();
    assert_eq!(
        store.resource_samples(imported, 0).unwrap().len(),
        1,
        "offline resources must not include the current collector"
    );
}
