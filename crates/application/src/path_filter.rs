//! Name filtering for the Files sidebar. Matching every path of a large
//! workspace on each keystroke froze the UI, so a worker debounces queries
//! and lists matches off the UI thread; frontends poll for the result.
use crate::DocumentSummary;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

/// Typing within this interval replaces the pending query without filtering.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// The entries to list for one name query.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathFilterResult {
    /// The query as entered.
    pub query: String,
    /// Files whose name contains the query, in workspace order.
    pub files: Vec<String>,
    /// Folders whose name contains the query plus every folder containing a
    /// match, sorted.
    pub directories: Vec<String>,
}

struct Job {
    generation: u64,
    query: String,
    documents: Arc<Vec<DocumentSummary>>,
    directories: Arc<Vec<String>>,
}

pub struct PathFilter {
    jobs: Sender<Job>,
    results: Receiver<(u64, PathFilterResult)>,
    generation: Arc<AtomicU64>,
    pending: bool,
}

impl Default for PathFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl PathFilter {
    pub fn new() -> Self {
        let (jobs, jobs_rx) = mpsc::channel();
        let (results_tx, results) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&generation);
        thread::spawn(move || worker(jobs_rx, results_tx, worker_generation));
        Self {
            jobs,
            results,
            generation,
            pending: false,
        }
    }

    /// Replaces any pending query. An empty query needs no work, so its
    /// result returns immediately; others arrive through `poll`.
    pub fn queue(
        &mut self,
        query: &str,
        documents: Arc<Vec<DocumentSummary>>,
        directories: Arc<Vec<String>>,
    ) -> Option<PathFilterResult> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        if query.trim().is_empty() {
            self.pending = false;
            return Some(PathFilterResult {
                query: query.to_owned(),
                ..Default::default()
            });
        }
        self.pending = true;
        let _ = self.jobs.send(Job {
            generation,
            query: query.to_owned(),
            documents,
            directories,
        });
        None
    }

    /// Filters on the calling thread, discarding any pending query. Meant for
    /// smoke tests that need the result before returning.
    pub fn filter_now(
        &mut self,
        query: &str,
        documents: Arc<Vec<DocumentSummary>>,
        directories: Arc<Vec<String>>,
    ) -> PathFilterResult {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.pending = false;
        let job = Job {
            generation,
            query: query.to_owned(),
            documents,
            directories,
        };
        filter_paths(&job, || false).expect("an uncancellable filter completes")
    }

    /// Whether a queued query has not published its result yet.
    pub fn in_progress(&self) -> bool {
        self.pending
    }

    /// The result of the latest query, once and only if it is current.
    pub fn poll(&mut self) -> Option<PathFilterResult> {
        let current = self.generation.load(Ordering::Relaxed);
        let mut latest = None;
        while let Ok((generation, result)) = self.results.try_recv() {
            if generation == current {
                latest = Some(result);
            }
        }
        if latest.is_some() {
            self.pending = false;
        }
        latest
    }
}

fn worker(
    jobs: Receiver<Job>,
    results: Sender<(u64, PathFilterResult)>,
    generation: Arc<AtomicU64>,
) {
    while let Ok(mut job) = jobs.recv() {
        loop {
            match jobs.recv_timeout(DEBOUNCE) {
                Ok(newer) => job = newer,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let cancelled = || generation.load(Ordering::Relaxed) != job.generation;
        if cancelled() {
            continue;
        }
        let Some(result) = filter_paths(&job, cancelled) else {
            continue;
        };
        if results.send((job.generation, result)).is_err() {
            return;
        }
    }
}

fn filter_paths(job: &Job, cancelled: impl Fn() -> bool) -> Option<PathFilterResult> {
    let needle = job.query.trim().to_lowercase();
    if needle.is_empty() {
        return Some(PathFilterResult {
            query: job.query.clone(),
            ..Default::default()
        });
    }
    let name_matches = |path: &str| {
        path.rsplit('/')
            .next()
            .unwrap_or(path)
            .to_lowercase()
            .contains(&needle)
    };
    let mut files = Vec::new();
    let mut directories = BTreeSet::new();
    let add_ancestors = |path: &str, directories: &mut BTreeSet<String>| {
        let mut end = path.len();
        while let Some(separator) = path[..end].rfind('/') {
            // Ancestors of a listed folder are already listed.
            if !directories.insert(path[..separator].to_owned()) {
                break;
            }
            end = separator;
        }
    };
    for (index, document) in job.documents.iter().enumerate() {
        if index % 1024 == 0 && cancelled() {
            return None;
        }
        let path = document.id.as_str();
        if name_matches(path) {
            files.push(path.to_owned());
            add_ancestors(path, &mut directories);
        }
    }
    for (index, path) in job.directories.iter().enumerate() {
        if index % 1024 == 0 && cancelled() {
            return None;
        }
        if name_matches(path) && directories.insert(path.clone()) {
            add_ancestors(path, &mut directories);
        }
    }
    Some(PathFilterResult {
        query: job.query.clone(),
        files,
        directories: directories.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_workspace_domain::DocumentId;
    use std::time::Instant;

    fn documents(paths: &[&str]) -> Arc<Vec<DocumentSummary>> {
        Arc::new(
            paths
                .iter()
                .map(|path| DocumentSummary {
                    id: DocumentId::parse(*path).unwrap(),
                    title: path.to_string(),
                    dirty: false,
                    external_change: false,
                })
                .collect(),
        )
    }

    fn wait(filter: &mut PathFilter) -> PathFilterResult {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = filter.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "filter did not finish");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn lists_matching_names_with_their_folders_and_keeps_only_the_latest_query() {
        let mut filter = PathFilter::new();
        let documents = documents(&[
            "README.md",
            "src/app/main.rs",
            "src/lib.rs",
            "tests/main_test.rs",
        ]);
        let directories = Arc::new(vec![
            "assets".to_owned(),
            "src".to_owned(),
            "src/app".to_owned(),
            "tests".to_owned(),
            "tests/Mainline".to_owned(),
        ]);
        assert!(
            filter
                .queue("lib", documents.clone(), directories.clone())
                .is_none()
        );
        assert!(
            filter
                .queue("MAIN", documents.clone(), directories.clone())
                .is_none()
        );
        assert!(filter.in_progress());
        let result = wait(&mut filter);
        assert!(!filter.in_progress());
        assert_eq!(result.query, "MAIN");
        assert_eq!(result.files, ["src/app/main.rs", "tests/main_test.rs"]);
        assert_eq!(
            result.directories,
            ["src", "src/app", "tests", "tests/Mainline"]
        );
        thread::sleep(DEBOUNCE * 2);
        assert_eq!(filter.poll(), None, "the superseded query never publishes");
    }

    #[test]
    fn an_empty_query_returns_immediately_and_cancels_pending_work() {
        let mut filter = PathFilter::new();
        let documents = documents(&["main.rs"]);
        let directories = Arc::new(Vec::new());
        assert!(
            filter
                .queue("main", documents.clone(), directories.clone())
                .is_none()
        );
        let cleared = filter.queue(" ", documents, directories).unwrap();
        assert_eq!(cleared.query, " ");
        assert!(cleared.files.is_empty() && cleared.directories.is_empty());
        assert!(!filter.in_progress());
        thread::sleep(DEBOUNCE * 2);
        assert_eq!(filter.poll(), None);
    }
}
