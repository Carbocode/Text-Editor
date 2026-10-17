//! Behavior tests of the application core. Shared fixtures live here so
//! every test drives the same real filesystem through the same hooks.

mod core;
mod delete;
mod document_tabs;
mod external_files;
mod search;
mod session;
mod system_open;

use crate::*;
use native_workspace_domain::DocumentId;
use native_workspace_filesystem::{
    FilesystemError, RealFilesystem, WorkspaceFilesystem, WorkspaceListing,
};
use std::{
    collections::HashSet,
    fs, io,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

pub(super) fn id(path: &str) -> DocumentId {
    DocumentId::parse(path).unwrap()
}

/// A temporary workspace with `files` (path, content); parent folders are created.
pub(super) fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let path = directory.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    directory
}

/// An application with `directory` open.
pub(super) fn open(directory: &Path, filesystem: TestFilesystem) -> Application {
    let mut app = Application::with_filesystem(Box::new(filesystem));
    app.dispatch(AppCommand::OpenWorkspace(directory.into()))
        .unwrap();
    app
}

/// Polls `poll` until `done` holds, failing after five seconds.
pub(super) fn wait_until<T>(mut poll: impl FnMut() -> T, done: impl Fn(&T) -> bool) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value = poll();
        if done(&value) {
            return value;
        }
        assert!(Instant::now() < deadline, "condition not reached in time");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Queues `query` and waits for the search worker to finish.
pub(super) fn search(app: &mut Application, query: &str) -> AppSnapshot {
    app.dispatch(AppCommand::QueueSearchQuery(query.into()))
        .unwrap();
    wait_until(
        || app.dispatch(AppCommand::PollSearchResults).unwrap(),
        |snapshot| !snapshot.search_in_progress,
    )
}

/// Executes a command and returns its events with the resulting snapshot.
pub(super) fn outcome(
    app: &mut Application,
    command: AppCommand,
) -> Result<(Vec<AppEvent>, AppSnapshot), AppError> {
    let events = app.execute(command)?;
    Ok((events, app.snapshot()))
}

/// Runs `session` until its worker returns.
pub(super) fn settle(session: &mut Session) -> AppSnapshot {
    wait_until(
        || {
            session.poll();
            session.snapshot()
        },
        |snapshot| !snapshot.activity.busy,
    )
}

/// A filesystem operation, observed by [`TestFilesystem`] hooks.
#[allow(dead_code, reason = "hooks match the operations they need")]
pub(super) enum Operation<'a> {
    Open,
    Resolve,
    Read(&'a DocumentId),
    SearchRead(&'a DocumentId),
    Write(&'a DocumentId),
    Rename(&'a DocumentId, &'a DocumentId),
    Delete(&'a DocumentId),
    Create(&'a DocumentId),
}

type Hook = dyn Fn(&Operation<'_>) -> io::Result<()> + Send + Sync;

/// The real filesystem behind a hook that may block or fail any operation.
pub(super) struct TestFilesystem(Arc<Hook>);

impl TestFilesystem {
    pub fn real() -> Self {
        Self::with(|_| Ok(()))
    }

    pub fn with(hook: impl Fn(&Operation<'_>) -> io::Result<()> + Send + Sync + 'static) -> Self {
        Self(Arc::new(hook))
    }

    fn check(&self, operation: Operation<'_>, root: &Path) -> Result<(), FilesystemError> {
        (self.0)(&operation).map_err(|source| FilesystemError::Io {
            path: root.to_path_buf(),
            source,
        })
    }
}

impl WorkspaceFilesystem for TestFilesystem {
    fn open_workspace(&self, path: &Path) -> Result<WorkspaceListing, FilesystemError> {
        self.check(Operation::Open, path)?;
        RealFilesystem.open_workspace(path)
    }
    fn resolve_open_target(
        &self,
        path: &Path,
    ) -> Result<native_workspace_filesystem::OpenTarget, FilesystemError> {
        self.check(Operation::Resolve, path)?;
        RealFilesystem.resolve_open_target(path)
    }
    fn resolve_file(&self, path: &Path) -> Result<std::path::PathBuf, FilesystemError> {
        self.check(Operation::Resolve, path)?;
        RealFilesystem.resolve_file(path)
    }
    fn move_file(
        &self,
        source: &Path,
        root: &Path,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        let id = DocumentId::from_document_path(source.to_str().unwrap()).unwrap();
        self.check(Operation::Rename(&id, target), root)?;
        RealFilesystem.move_file(source, root, target)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        self.check(Operation::Read(id), root)?;
        RealFilesystem.read_document(root, id)
    }
    fn read_document_for_search(
        &self,
        root: &Path,
        id: &DocumentId,
        verified: &mut HashSet<String>,
    ) -> Option<String> {
        self.check(Operation::SearchRead(id), root).ok()?;
        RealFilesystem.read_document_for_search(root, id, verified)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        self.check(Operation::Write(id), root)?;
        RealFilesystem.write_document(root, id, content)
    }
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        self.check(Operation::Rename(id, target), root)?;
        RealFilesystem.rename_entry(root, id, target)
    }
    fn delete_entry(&self, root: &Path, id: &DocumentId) -> Result<(), FilesystemError> {
        self.check(Operation::Delete(id), root)?;
        RealFilesystem.delete_entry(root, id)
    }
    fn create_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        directory: bool,
    ) -> Result<(), FilesystemError> {
        self.check(Operation::Create(id), root)?;
        RealFilesystem.create_entry(root, id, directory)
    }
}

/// A gate a hook waits on until the test opens it.
#[derive(Clone, Default)]
pub(super) struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    pub fn wait(&self) {
        let (open, condition) = &*self.0;
        let mut released = open.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
    }

    pub fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}
