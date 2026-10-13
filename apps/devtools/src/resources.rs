//! One low-frequency collector thread samples a bounded set of registered PIDs.
use native_workspace_devtools_protocol::{
    CollectorStatus, Counter, ResourceSample, Session, Uuid, unix_ms,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone)]
struct Target {
    session: Uuid,
    role: String,
    enabled: bool,
}
#[derive(Default)]
pub struct ResourceState {
    targets: Mutex<HashMap<u32, Target>>,
    pub status: Mutex<CollectorStatus>,
    pub received: AtomicU64,
    pub committed: AtomicU64,
    pub rejected: AtomicU64,
    pub gaps: AtomicU64,
    pub queue: AtomicU64,
    pub last_commit: AtomicU64,
}
impl ResourceState {
    pub fn register_editor(&self, s: &Session, recording: bool) {
        if s.process_id == 0 || s.frontend == "synthetic" {
            return;
        }
        if let Ok(mut targets) = self.targets.lock()
            && (targets.len() < 24 || targets.contains_key(&s.process_id))
        {
            targets.insert(
                s.process_id,
                Target {
                    session: s.id,
                    role: "editor".into(),
                    enabled: recording,
                },
            );
        }
    }
    pub fn register_console(&self, pid: u32) {
        if pid == 0 {
            return;
        }
        if let Ok(mut targets) = self.targets.lock()
            && (targets.len() < 24 || targets.contains_key(&pid))
        {
            targets.insert(
                pid,
                Target {
                    session: Uuid::nil(),
                    role: "console".into(),
                    enabled: true,
                },
            );
        }
    }
    pub fn set_recording(&self, id: Uuid, recording: bool) {
        if let Ok(mut targets) = self.targets.lock() {
            for target in targets.values_mut().filter(|target| target.session == id) {
                target.enabled = recording;
            }
        }
    }
    pub fn snapshot(&self) -> CollectorStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn start(
        self: Arc<Self>,
        path: PathBuf,
        mut submit: impl FnMut(Vec<ResourceSample>) -> bool + Send + 'static,
    ) -> std::io::Result<()> {
        if let Ok(mut targets) = self.targets.lock() {
            targets.insert(
                std::process::id(),
                Target {
                    session: Uuid::nil(),
                    role: "collector".into(),
                    enabled: true,
                },
            );
        }
        thread::Builder::new()
            .name("devtools-resources".into())
            .spawn(move || {
                let mut system = System::new();
                let kind = ProcessRefreshKind::nothing()
                    .with_cpu()
                    .with_memory()
                    .with_disk_usage()
                    .without_tasks();
                let mut previous = HashMap::<u32, (u64, u64)>::new();
                loop {
                    let targets = self.targets.lock().map(|t| t.clone()).unwrap_or_default();
                    let pids: Vec<Pid> = targets
                        .iter()
                        .filter(|(_, target)| target.enabled)
                        .map(|(pid, _)| Pid::from_u32(*pid))
                        .collect();
                    system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
                    system.refresh_memory();
                    let now = unix_ms();
                    let mut samples = Vec::new();
                    for (pid, target) in &targets {
                        if !target.enabled {
                            previous.remove(pid);
                            continue;
                        }
                        if let Some(process) = system.process(Pid::from_u32(*pid)) {
                            let disk = process.disk_usage();
                            let last = previous
                                .insert(*pid, (disk.total_read_bytes, disk.total_written_bytes));
                            samples.push(ResourceSample {
                                session: target.session,
                                unix_ms: Counter(now),
                                pid: *pid,
                                role: target.role.clone(),
                                cpu_percent: process.cpu_usage(),
                                memory_bytes: Counter(process.memory()),
                                virtual_bytes: Counter(process.virtual_memory()),
                                read_bytes: Counter(last.map_or(0, |(read, _)| {
                                    disk.total_read_bytes.saturating_sub(read)
                                })),
                                written_bytes: Counter(last.map_or(0, |(_, written)| {
                                    disk.total_written_bytes.saturating_sub(written)
                                })),
                                alive: true,
                            });
                        } else {
                            samples.push(ResourceSample {
                                session: target.session,
                                unix_ms: Counter(now),
                                pid: *pid,
                                role: target.role.clone(),
                                cpu_percent: 0.0,
                                memory_bytes: Counter(0),
                                virtual_bytes: Counter(0),
                                read_bytes: Counter(0),
                                written_bytes: Counter(0),
                                alive: false,
                            });
                            if target.role != "collector" {
                                if let Ok(mut current) = self.targets.lock() {
                                    current.remove(pid);
                                }
                                previous.remove(pid);
                            }
                        }
                    }
                    if !submit(samples) {
                        self.gaps.fetch_add(1, Ordering::Relaxed);
                    }
                    let size = std::fs::metadata(&path).map_or(0, |m| m.len())
                        + std::fs::metadata(path.with_extension("duckdb.wal"))
                            .map_or(0, |m| m.len());
                    if let Ok(mut status) = self.status.lock() {
                        *status = CollectorStatus {
                            sampled_unix_ms: Counter(now),
                            cpu_count: System::physical_core_count().unwrap_or(1) as u32,
                            system_memory_bytes: Counter(system.total_memory()),
                            system_used_memory_bytes: Counter(system.used_memory()),
                            writer_queue: Counter(self.queue.load(Ordering::Relaxed)),
                            received_records: Counter(self.received.load(Ordering::Relaxed)),
                            committed_records: Counter(self.committed.load(Ordering::Relaxed)),
                            rejected_batches: Counter(self.rejected.load(Ordering::Relaxed)),
                            sampling_gaps: Counter(self.gaps.load(Ordering::Relaxed)),
                            database_bytes: Counter(size),
                            last_commit_unix_ms: Counter(self.last_commit.load(Ordering::Relaxed)),
                        };
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            })?;
        Ok(())
    }
}
