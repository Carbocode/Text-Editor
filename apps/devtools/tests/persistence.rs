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
    store
        .persist_samples(std::slice::from_ref(&sample))
        .unwrap();
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

#[test]
fn operation_query_recovers_span_across_unrelated_pages_and_preserves_cursor_scope() {
    use native_workspace_devtools_protocol::{Kind, MAX_PAGE};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("analytics.duckdb")).unwrap();
    let mut batch = synthetic();
    batch.session.id = Uuid::new_v4();
    let operation = (1_u64 << 63) + 29;
    let mut records = Vec::new();
    for sequence in 1..=1203 {
        let mut record = batch.records[0].clone();
        record.sequence = Counter(sequence);
        record.monotonic_ns = Counter(sequence * 1000);
        record.operation = Counter(if sequence == 1 || sequence == 1203 {
            operation
        } else {
            17
        });
        record.span = Counter(if sequence == 1 || sequence == 1203 {
            7
        } else {
            0
        });
        record.name = if sequence == 1 || sequence == 1203 {
            "search.execute"
        } else {
            "document.changed"
        }
        .into();
        record.kind = if sequence == 1 {
            Kind::SpanStart
        } else if sequence == 1203 {
            Kind::SpanEnd
        } else {
            Kind::Event
        };
        records.push(record);
    }
    for chunk in records.chunks(256) {
        batch.id = Uuid::new_v4();
        batch.records = chunk.to_vec();
        store.ingest(&batch).unwrap();
    }
    let id = batch.session.id;
    assert!(
        !store
            .tail(id, MAX_PAGE)
            .unwrap()
            .records
            .iter()
            .any(|r| r.sequence == Counter(1))
    );
    let trace = store
        .query_page(id, 0, MAX_PAGE, false, Some(operation))
        .unwrap();
    assert_eq!(
        trace
            .records
            .iter()
            .map(|r| r.sequence.0)
            .collect::<Vec<_>>(),
        [1, 1203]
    );
    assert_eq!(trace.records[0].kind, Kind::SpanStart);
    assert_eq!(trace.records[1].kind, Kind::SpanEnd);
    assert_eq!(trace.records[0].span, trace.records[1].span);
    assert!(trace.next.is_none());
    let first = store.query_page(id, 0, 1, false, Some(operation)).unwrap();
    assert_eq!(first.next, Some(Counter(1)));
    let next = store
        .query_page(id, first.next.unwrap().0, 1, false, Some(operation))
        .unwrap();
    assert_eq!(next.records[0].sequence, Counter(1203));
    assert!(next.next.is_none());
    let tail = store.query_tail(id, 1, false, Some(operation)).unwrap();
    assert_eq!(tail.records[0].sequence, Counter(1203));
    assert!(
        store
            .query_page(id, 0, MAX_PAGE, true, Some(operation + 1))
            .unwrap()
            .records
            .is_empty()
    );
    // Filtering cannot leak another session's operation with the same numeric ID.
    assert!(
        store
            .query_page(Uuid::new_v4(), 0, MAX_PAGE, true, Some(operation))
            .unwrap()
            .records
            .is_empty()
    );
    let first = store.query_page(id, 0, MAX_PAGE, false, Some(17)).unwrap();
    assert_eq!(first.records.len(), MAX_PAGE);
    assert_eq!(first.next, Some(Counter(501)));
    assert!(first.records.iter().all(|r| r.operation == Counter(17)));
}

#[test]
fn pins_persist_and_stay_visible_beyond_the_session_header_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytics.duckdb");
    let mut store = Store::open(&path).unwrap();
    let mut ids = Vec::new();
    for index in 0..205 {
        let mut batch = synthetic();
        batch.id = Uuid::new_v4();
        batch.session.id = Uuid::new_v4();
        batch.session.started_unix_ms = Counter(1000 + index);
        store.ingest(&batch).unwrap();
        ids.push(batch.session.id);
    }
    for id in &ids[..64] {
        store.set_pin(*id, true).unwrap();
    }
    store.set_pin(ids[0], true).unwrap(); // Idempotent even at the limit.
    assert_eq!(
        store.set_pin(ids[64], true).unwrap_err().to_string(),
        "pin_limit_reached"
    );
    assert_eq!(
        store.set_pin(Uuid::new_v4(), true).unwrap_err().to_string(),
        "session_not_found"
    );
    let sessions = store.sessions().unwrap();
    assert_eq!(sessions.len(), 200);
    assert_eq!(sessions.iter().filter(|session| session.pinned).count(), 64);
    assert_eq!(sessions[0].session.id, ids[63]);
    assert!(
        sessions
            .iter()
            .any(|session| session.session.id == ids[0] && session.pinned)
    );
    assert!(
        sessions
            .iter()
            .any(|session| session.session.id == ids[204] && !session.pinned)
    );
    let bytes = store.export(ids[0]).unwrap();
    let imported = store.import(&bytes).unwrap();
    assert!(!store.session(imported).unwrap().unwrap().pinned);
    assert!(store.session(Uuid::new_v4()).unwrap().is_none());
    drop(store);
    let mut reopened = Store::open(&path).unwrap();
    assert!(
        reopened
            .sessions()
            .unwrap()
            .iter()
            .find(|session| session.session.id == ids[0])
            .unwrap()
            .pinned
    );
    reopened.set_pin(ids[0], false).unwrap();
    reopened.set_pin(ids[0], false).unwrap();
    reopened.set_pin(ids[64], true).unwrap();
    assert!(
        reopened
            .sessions()
            .unwrap()
            .iter()
            .find(|session| session.session.id == ids[64])
            .unwrap()
            .pinned
    );
}

#[test]
fn drop_measurements_survive_persistence_and_operation_filtering() {
    use native_workspace_devtools_protocol::{Kind, Outcome};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("drop.duckdb")).unwrap();
    let mut batch = synthetic();
    let mut start = batch.records[0].clone();
    start.name = "drop.confirmation".into();
    start.kind = Kind::SpanStart;
    start.operation = Counter(73);
    start.span = Counter(74);
    start.monotonic_ns = Counter(10);
    let mut end = start.clone();
    end.kind = Kind::SpanEnd;
    end.outcome = Outcome::Error;
    end.monotonic_ns = Counter(1_000_000_010);
    end.value = Counter(1_000_000_000);
    end.sequence = Counter(2);
    let mut unrelated = end.clone();
    unrelated.operation = Counter(99);
    unrelated.span = Counter(0);
    unrelated.kind = Kind::Event;
    unrelated.name = "snapshot.apply".into();
    unrelated.sequence = Counter(3);
    batch.records = vec![start, end, unrelated];
    store.ingest(&batch).unwrap();
    let stats = store.analysis(batch.session.id).unwrap();
    let confirmation = stats
        .operations
        .iter()
        .find(|s| s.name == "drop.confirmation")
        .unwrap();
    assert_eq!(confirmation.errors.0, 1);
    assert_eq!(confirmation.max_ms, 1000.0);
    let records = store
        .query_tail(batch.session.id, 500, false, Some(73))
        .unwrap()
        .records;
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.operation.0 == 73));
    assert_eq!(records[1].value.0, 1_000_000_000);
}
