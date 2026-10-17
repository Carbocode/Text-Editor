//! Name filtering for the Files sidebar. Matching every path of a large
//! workspace on each keystroke froze the UI, so the shared debounced worker
//! lists matches off the UI thread and the session publishes the result.
use crate::{
    DocumentSummary,
    worker::{Cancellation, DebouncedWorker},
};
use native_workspace_domain::path;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
};

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

impl PathFilterResult {
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

struct Job {
    query: String,
    documents: Arc<Vec<DocumentSummary>>,
    directories: Arc<Vec<String>>,
}

pub(crate) struct PathFilter {
    worker: DebouncedWorker<Job>,
    results: Receiver<(u64, PathFilterResult)>,
}

impl PathFilter {
    pub fn new() -> Self {
        let (sender, results) = mpsc::channel();
        let worker = DebouncedWorker::spawn(
            |_superseded, newer| newer,
            move |job: Job, cancellation: &Cancellation| {
                if let Some(result) = filter_paths(&job, || cancellation.is_cancelled()) {
                    let _ = sender.send((cancellation.generation(), result));
                }
            },
        );
        Self { worker, results }
    }

    /// Replaces any pending query. An empty query needs no work, so its
    /// result returns immediately; others arrive through `poll`.
    pub fn queue(
        &mut self,
        query: &str,
        documents: Arc<Vec<DocumentSummary>>,
        directories: Arc<Vec<String>>,
    ) -> Option<PathFilterResult> {
        if query.trim().is_empty() {
            self.worker.cancel();
            return Some(PathFilterResult {
                query: query.to_owned(),
                ..Default::default()
            });
        }
        self.worker.submit(Job {
            query: query.to_owned(),
            documents,
            directories,
        });
        None
    }

    /// The result of the latest query, once and only if it is current.
    pub fn poll(&mut self) -> Option<PathFilterResult> {
        let current = self.worker.current();
        let mut latest = None;
        while let Ok((generation, result)) = self.results.try_recv() {
            if generation == current {
                latest = Some(result);
            }
        }
        latest
    }
}

fn filter_paths(job: &Job, cancelled: impl Fn() -> bool) -> Option<PathFilterResult> {
    let needle = job.query.trim().to_lowercase();
    let name_matches = |entry: &str| path::name(entry).to_lowercase().contains(&needle);
    let mut files = Vec::new();
    let mut directories = BTreeSet::new();
    let add_ancestors = |entry: &str, directories: &mut BTreeSet<String>| {
        for ancestor in path::ancestors(entry) {
            // Ancestors of a listed folder are already listed.
            if !directories.insert(ancestor.to_owned()) {
                break;
            }
        }
    };
    for (index, document) in job.documents.iter().enumerate() {
        if index % 1024 == 0 && cancelled() {
            return None;
        }
        if document.id.is_external() {
            continue;
        }
        let entry = document.id.as_str();
        if name_matches(entry) {
            files.push(entry.to_owned());
            add_ancestors(entry, &mut directories);
        }
    }
    for (index, entry) in job.directories.iter().enumerate() {
        if index % 1024 == 0 && cancelled() {
            return None;
        }
        if name_matches(entry) && directories.insert(entry.clone()) {
            add_ancestors(entry, &mut directories);
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
    use crate::worker::DEBOUNCE;
    use native_workspace_domain::DocumentId;
    use std::{
        thread,
        time::{Duration, Instant},
    };

    fn documents(paths: &[&str]) -> Arc<Vec<DocumentSummary>> {
        Arc::new(
            paths
                .iter()
                .map(|path| DocumentSummary {
                    id: DocumentId::parse(*path).unwrap(),
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
        let result = wait(&mut filter);
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
        thread::sleep(DEBOUNCE * 2);
        assert_eq!(filter.poll(), None);
    }
}
