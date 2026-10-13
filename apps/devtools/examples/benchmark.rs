//! Reproducible storage probe; use --release for representative results.
use native_workspace_devtools::store::{Result, Store};
use native_workspace_devtools_protocol::{Counter, MAX_RECORDS, Uuid, synthetic};
use std::time::Instant;
fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() - 1) as f64 * p) as usize]
}
fn main() -> Result<()> {
    let batches: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "200".into())
        .parse()?;
    if batches == 0 || batches > 10000 {
        return Err("batch count must be 1..=10000".into());
    }
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("analytics.duckdb");
    let mut store = Store::open(&path)?;
    let mut batch = synthetic();
    batch.session.id = Uuid::new_v4();
    batch.session.producer = Uuid::new_v4();
    let template = batch.records.clone();
    batch.records = (0..MAX_RECORDS)
        .map(|i| template[i % template.len()].clone())
        .collect();
    batch.dropped_total = Counter(0);
    let start = Instant::now();
    let mut commits = Vec::new();
    for index in 0..batches {
        batch.id = Uuid::new_v4();
        for (i, record) in batch.records.iter_mut().enumerate() {
            record.sequence = Counter((index * MAX_RECORDS + i + 1) as u64);
            record.monotonic_ns = Counter(record.sequence.0 * 100000);
            record.span = Counter(record.span.0 + index as u64 * 100);
        }
        let t = Instant::now();
        store.ingest(&batch)?;
        commits.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let mut reads = Vec::new();
    for i in 0..100 {
        let t = Instant::now();
        store.page(batch.session.id, (i * MAX_RECORDS) as u64, 500)?;
        reads.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let t = Instant::now();
    let bytes = store.export(batch.session.id)?;
    let export_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    store.import(&bytes)?;
    let import_ms = t.elapsed().as_secs_f64() * 1000.0;
    drop(store);
    let t = Instant::now();
    let recovered = Store::open(&path)?;
    let recovery_ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(recovered.sessions()?.len(), 2);
    println!(
        "{}",
        serde_json::json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"batches":batches,"records":batches*MAX_RECORDS,"records_per_second":batches as f64*MAX_RECORDS as f64/elapsed,"commit_p95_ms":percentile(&mut commits,0.95),"commit_p99_ms":percentile(&mut commits,0.99),"page_p95_ms":percentile(&mut reads,0.95),"page_p99_ms":percentile(&mut reads,0.99),"archive_bytes":bytes.len(),"export_ms":export_ms,"import_ms":import_ms,"recovery_ms":recovery_ms,"database_bytes":std::fs::metadata(path)?.len()})
    );
    Ok(())
}
