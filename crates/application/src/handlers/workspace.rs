//! Opening, scanning and reconciling the workspace tree.

use crate::{AppError, AppEvent, Application, DocumentEntry, diagnostics::Outcome};
use native_workspace_filesystem::{
    FilesystemError, WorkspaceFilesystem, WorkspaceListing, WorkspaceWatcher,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

/// Full scans catch events the watcher missed.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
/// Without a native watcher, scans are the only change detection.
const RECONCILE_INTERVAL_WITHOUT_WATCHER: Duration = Duration::from_secs(2);

/// A background scan and the revision it started from.
pub(crate) struct PendingReconcile {
    revision: u64,
    result: Receiver<Result<Vec<AppEvent>, AppError>>,
}

impl Application {
    pub(super) fn open_workspace(&mut self, path: &Path) -> Result<Vec<AppEvent>, AppError> {
        if self
            .state()
            .documents
            .entries()
            .iter()
            .any(DocumentEntry::dirty)
        {
            return Err(AppError::UnsavedChanges);
        }
        let WorkspaceListing {
            workspace,
            documents,
            directories,
        } = self.filesystem.open_workspace(path)?;
        let recent_workspaces = self.storage.record_workspace(&workspace)?;
        self.pending_reconcile = None;
        self.watcher = WorkspaceWatcher::new(&workspace.root).ok();
        self.last_reconcile = Instant::now();
        Ok(vec![AppEvent::WorkspaceOpened {
            workspace,
            documents,
            directories,
            recent_workspaces,
        }])
    }

    pub(super) fn refresh_workspace(&mut self) -> Result<Vec<AppEvent>, AppError> {
        let root = self.workspace_root()?.to_path_buf();
        self.pending_reconcile = None;
        reconcile(
            self.filesystem.as_ref(),
            &root,
            self.state().documents.entries(),
            &self.state().directories,
        )
    }

    /// Collects a finished background scan, or starts one after a watcher
    /// event or when the reconciliation interval elapsed.
    pub(super) fn poll_workspace_changes(&mut self) -> Result<Vec<AppEvent>, AppError> {
        let Some(workspace) = &self.state().workspace else {
            return Ok(Vec::new());
        };
        let root = workspace.root.clone();
        if let Some(pending) = &self.pending_reconcile {
            let result = match pending.result.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return Ok(Vec::new()),
                Err(TryRecvError::Disconnected) => Err(AppError::Filesystem(FilesystemError::Io {
                    path: root,
                    source: std::io::Error::other("Workspace scan worker stopped"),
                })),
            };
            let started_at = pending.revision;
            self.pending_reconcile = None;
            // A scan cannot replace edits or paths changed since it started.
            if started_at != self.state().revision {
                self.last_reconcile = Instant::now() - RECONCILE_INTERVAL;
                return Ok(Vec::new());
            }
            return result;
        }
        let interval = if self.watcher.is_some() {
            RECONCILE_INTERVAL
        } else {
            RECONCILE_INTERVAL_WITHOUT_WATCHER
        };
        let batch = self
            .watcher
            .as_ref()
            .map(WorkspaceWatcher::drain_changes)
            .unwrap_or_default();
        if batch.events > 0 {
            self.command_context.event(
                "watcher.batch",
                "main",
                self.state().revision,
                batch.events,
            );
        }
        if batch.errors > 0 {
            self.command_context.event(
                "watcher.errors",
                "main",
                self.state().revision,
                batch.errors,
            );
        }
        if batch.changed() || self.last_reconcile.elapsed() >= interval {
            self.last_reconcile = Instant::now();
            self.start_background_reconcile(root);
        }
        Ok(Vec::new())
    }

    fn start_background_reconcile(&mut self, root: std::path::PathBuf) {
        let documents = self.state().documents.entries().to_vec();
        let directories = Arc::clone(&self.state().directories);
        let filesystem = Arc::clone(&self.filesystem);
        let context = self.command_context.clone();
        let revision = self.state().revision;
        let (sender, result) = mpsc::channel();
        self.pending_reconcile = Some(PendingReconcile { revision, result });
        thread::spawn(move || {
            let span = context.span("reconcile.background", "filesystem-worker");
            let outcome = reconcile(filesystem.as_ref(), &root, &documents, &directories);
            span.end(
                if outcome.is_ok() {
                    Outcome::Ok
                } else {
                    Outcome::Error
                },
                revision,
                0,
            );
            let _ = sender.send(outcome);
        });
    }
}

/// Rescans the tree and reconciles loaded buffers with the disk: clean
/// buffers reload, modified ones are marked as externally changed, and
/// loaded files that disappeared stay listed as conflicts.
fn reconcile(
    filesystem: &dyn WorkspaceFilesystem,
    root: &Path,
    previous_documents: &[DocumentEntry],
    previous_directories: &[String],
) -> Result<Vec<AppEvent>, AppError> {
    let WorkspaceListing {
        documents: ids,
        directories,
        ..
    } = filesystem.open_workspace(root)?;
    let mut ids = ids;
    ids.extend(
        previous_documents
            .iter()
            .filter(|entry| entry.id().is_external())
            .map(|entry| entry.id().clone()),
    );
    let listed: HashSet<_> = ids.iter().collect();
    let previous: HashMap<_, _> = previous_documents
        .iter()
        .map(|entry| (entry.id(), entry))
        .collect();
    let mut documents = Vec::with_capacity(ids.len());
    for id in &ids {
        let mut entry = previous
            .get(id)
            .map(|entry| (*entry).clone())
            .unwrap_or_else(|| DocumentEntry::new(id.clone()));
        if entry.is_loaded() {
            match filesystem.read_document(root, id) {
                Ok(disk) if entry.saved_content() == Some(disk.as_str()) => {
                    entry.set_external_change(false);
                }
                Ok(disk) if !entry.dirty() => entry.load(disk.into()),
                _ => entry.set_external_change(true),
            }
        }
        documents.push(entry);
    }
    for entry in previous_documents {
        if !listed.contains(entry.id()) && entry.is_loaded() {
            let mut missing = entry.clone();
            missing.set_external_change(true);
            documents.push(missing);
        }
    }
    documents.sort_by(|left, right| left.id().cmp(right.id()));
    if documents == previous_documents && directories == previous_directories {
        Ok(Vec::new())
    } else {
        Ok(vec![AppEvent::WorkspaceRefreshed {
            documents,
            directories,
        }])
    }
}
