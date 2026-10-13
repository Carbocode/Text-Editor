//! The concrete filesystem boundary for local workspaces.

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{SearcherBuilder, sinks::UTF8};
use native_workspace_domain::{DocumentId, Workspace};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    cell::RefCell,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

thread_local! {
    /// Search jobs stay on one worker thread, so the compiled matcher is reused
    /// across every file in a query without locking the UI thread.
    static SEARCH_MATCHER: RefCell<Option<(String, RegexMatcher)>> = const { RefCell::new(None) };
}

/// Delivers relevant filesystem events without rescanning the workspace on each UI tick.
pub struct WorkspaceWatcher {
    _watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
}

impl WorkspaceWatcher {
    pub fn new(root: &Path) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            events,
        })
    }

    pub fn has_changes(&self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            changed |= match event {
                Ok(event) => !matches!(event.kind, EventKind::Access(_)),
                Err(_) => true,
            };
        }
        changed
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error("invalid workspace directory: {0}")]
    InvalidWorkspace(PathBuf),
    #[error("document is not a regular file inside the workspace: {0}")]
    InvalidDocument(PathBuf),
    #[error("filesystem error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub trait WorkspaceFilesystem: Send + Sync {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError>;
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError>;
    fn read_document_for_search(&self, root: &Path, id: &DocumentId) -> Option<String> {
        self.read_document(root, id).ok()
    }
    fn read_document_if_matching(
        &self,
        root: &Path,
        id: &DocumentId,
        needle: &str,
    ) -> Option<String> {
        let content = self.read_document_for_search(root, id)?;
        content.to_lowercase().contains(needle).then_some(content)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError>;
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        let source_path = RealFilesystem::safe_document_location(root, id)?;
        let target_path = RealFilesystem::safe_document_location(root, target)?;
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source| FilesystemError::Io {
                path: source_path.clone(),
                source,
            })?;
        if !(metadata.is_file() || metadata.is_dir()) {
            return Err(FilesystemError::InvalidDocument(source_path));
        }
        rename_without_replacing(&source_path, &target_path).map_err(|source| FilesystemError::Io {
            path: target_path,
            source,
        })
    }

    fn create_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        directory: bool,
    ) -> Result<(), FilesystemError> {
        let path = RealFilesystem::safe_document_location(root, id)?;
        if directory {
            fs::create_dir(&path).map_err(|source| FilesystemError::Io { path, source })
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map(|_| ())
                .map_err(|source| FilesystemError::Io { path, source })
        }
    }
}

// Both supported Unix platforms offer an atomic no-replace rename, including folders.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_without_replacing(from: &Path, to: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both pointers refer to live, NUL-terminated path strings.
    let result = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL)
        }
        #[cfg(target_os = "linux")]
        {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_without_replacing(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "safe rename is unavailable on this platform",
    ))
}

#[derive(Debug, Default)]
pub struct RealFilesystem;

impl RealFilesystem {
    fn safe_document_location(root: &Path, id: &DocumentId) -> Result<PathBuf, FilesystemError> {
        let path = root.join(id.as_str());
        let mut parent = root.to_path_buf();
        for component in Path::new(id.as_str())
            .parent()
            .into_iter()
            .flat_map(Path::components)
        {
            parent.push(component);
            if !fs::symlink_metadata(&parent)
                .map_err(|source| FilesystemError::Io {
                    path: parent.clone(),
                    source,
                })?
                .file_type()
                .is_dir()
            {
                return Err(FilesystemError::InvalidDocument(path));
            }
        }
        Ok(path)
    }

    fn document_path(root: &Path, id: &DocumentId) -> Result<PathBuf, FilesystemError> {
        let path = Self::safe_document_location(root, id)?;
        let metadata = fs::symlink_metadata(&path).map_err(|source| FilesystemError::Io {
            path: path.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() {
            return Err(FilesystemError::InvalidDocument(path));
        }
        Ok(path)
    }
}

impl WorkspaceFilesystem for RealFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        let root = fs::canonicalize(path).map_err(|source| FilesystemError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if !root.is_dir() {
            return Err(FilesystemError::InvalidWorkspace(root));
        }
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace")
            .to_owned();
        let mut documents = Vec::new();
        let mut directories = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).map_err(|source| FilesystemError::Io {
                path: directory.clone(),
                source,
            })? {
                let entry = entry.map_err(|source| FilesystemError::Io {
                    path: directory.clone(),
                    source,
                })?;
                let file_type = entry.file_type().map_err(|source| FilesystemError::Io {
                    path: entry.path(),
                    source,
                })?;
                let relative = entry
                    .path()
                    .strip_prefix(&root)
                    .expect("entry is below root")
                    .to_path_buf();
                let Some(relative) = relative.to_str() else {
                    continue;
                };
                let relative = relative.replace(std::path::MAIN_SEPARATOR, "/");
                if file_type.is_dir() {
                    directories.push(relative);
                    pending.push(entry.path());
                } else if file_type.is_file()
                    && let Ok(id) = DocumentId::parse(relative)
                {
                    documents.push(id);
                }
            }
        }
        documents.sort();
        directories.sort();
        Ok((Workspace { root, name }, documents, directories))
    }

    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        let path = Self::document_path(root, id)?;
        fs::read_to_string(&path).map_err(|source| FilesystemError::Io { path, source })
    }

    fn read_document_for_search(&self, root: &Path, id: &DocumentId) -> Option<String> {
        const MAX_SEARCH_BYTES: u64 = 1024 * 1024;
        let path = Self::document_path(root, id).ok()?;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(MAX_SEARCH_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_SEARCH_BYTES {
            return None;
        }
        String::from_utf8(bytes).ok()
    }

    fn read_document_if_matching(
        &self,
        root: &Path,
        id: &DocumentId,
        needle: &str,
    ) -> Option<String> {
        if !needle.is_ascii() || needle.chars().any(char::is_control) {
            return self.read_document_for_search(root, id);
        }
        const MAX_SEARCH_BYTES: u64 = 1024 * 1024;
        let path = Self::document_path(root, id).ok()?;
        if fs::metadata(&path).ok()?.len() > MAX_SEARCH_BYTES {
            return None;
        }
        let found = SEARCH_MATCHER.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.as_ref().is_none_or(|(query, _)| query != needle) {
                // Rust's lowercase expansion can match text that Unicode regex
                // case folding does not (for example, `i` in `İ`). Such files
                // pass through to the exact matcher in the application core.
                let matcher = RegexMatcherBuilder::new()
                    .case_insensitive(true)
                    .build(&format!("(?:{}|[^\\x00-\\x7F])", regex::escape(needle)))
                    .ok()?;
                *cache = Some((needle.to_owned(), matcher));
            }
            let matcher = &cache.as_ref()?.1;
            let mut found = false;
            let mut searcher = SearcherBuilder::new().build();
            searcher
                .search_path(
                    matcher,
                    &path,
                    UTF8(|_, _| {
                        found = true;
                        Ok::<bool, io::Error>(false)
                    }),
                )
                .ok()?;
            Some(found)
        })?;
        if found {
            self.read_document_for_search(root, id)
        } else {
            None
        }
    }

    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        let path = Self::safe_document_location(root, id)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                fs::write(&path, content).map_err(|source| FilesystemError::Io { path, source })
            }
            Ok(_) => Err(FilesystemError::InvalidDocument(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|source| FilesystemError::Io {
                        path: path.clone(),
                        source,
                    })?;
                file.write_all(content.as_bytes())
                    .map_err(|source| FilesystemError::Io { path, source })
            }
            Err(source) => Err(FilesystemError::Io { path, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_never_replaces_existing_files_or_folders() {
        let dir = tempfile::tempdir().unwrap();
        let filesystem = RealFilesystem;
        for name in ["a", "b"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        fs::write(dir.path().join("a/note"), "keep").unwrap();
        assert!(
            filesystem
                .rename_entry(
                    dir.path(),
                    &DocumentId::parse("a").unwrap(),
                    &DocumentId::parse("b").unwrap()
                )
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("a/note")).unwrap(),
            "keep"
        );
        filesystem
            .rename_entry(
                dir.path(),
                &DocumentId::parse("a").unwrap(),
                &DocumentId::parse("renamed").unwrap(),
            )
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("renamed/note")).unwrap(),
            "keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rename_rejects_symlink_sources_destinations_and_parents() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("alias"))
            .unwrap();
        fs::write(dir.path().join("note"), "inside").unwrap();
        for (from, to) in [
            ("alias", "new"),
            ("linked/secret", "new"),
            ("note", "linked/new"),
            ("note", "alias"),
        ] {
            assert!(
                RealFilesystem
                    .rename_entry(
                        dir.path(),
                        &DocumentId::parse(from).unwrap(),
                        &DocumentId::parse(to).unwrap()
                    )
                    .is_err()
            );
        }
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "outside"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("note")).unwrap(),
            "inside"
        );
    }

    #[test]
    fn lists_nested_files_and_directories_and_round_trips_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "before").unwrap();
        fs::write(dir.path().join("a.md"), "# A").unwrap();
        fs::write(dir.path().join("ignored.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("nested/note.md"), "hidden").unwrap();
        let filesystem = RealFilesystem;
        let (workspace, ids, directories) = filesystem.open_workspace(dir.path()).unwrap();
        assert_eq!(directories, ["nested"]);
        assert_eq!(
            ids.iter().map(DocumentId::as_str).collect::<Vec<_>>(),
            ["a.md", "b.txt", "ignored.rs", "nested/note.md"]
        );
        assert_eq!(
            filesystem.read_document(&workspace.root, &ids[1]).unwrap(),
            "before"
        );
        filesystem
            .write_document(&workspace.root, &ids[1], "after")
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "after"
        );
    }

    #[test]
    fn content_search_skips_oversized_files_without_hiding_the_document() {
        let dir = tempfile::tempdir().unwrap();
        let id = DocumentId::parse("large.md").unwrap();
        fs::write(dir.path().join(id.as_str()), "x".repeat(1024 * 1024 + 1)).unwrap();
        let filesystem = RealFilesystem;
        assert_eq!(
            filesystem.open_workspace(dir.path()).unwrap().1,
            std::slice::from_ref(&id)
        );
        assert!(
            filesystem
                .read_document_for_search(dir.path(), &id)
                .is_none()
        );
    }

    #[test]
    fn ripgrep_filter_preserves_literal_and_unicode_case_matches() {
        let dir = tempfile::tempdir().unwrap();
        let id = DocumentId::parse("note.md").unwrap();
        fs::write(dir.path().join(id.as_str()), "a.b İ K").unwrap();
        let filesystem = RealFilesystem;
        for query in ["a.b", "i", "k"] {
            assert!(
                filesystem
                    .read_document_if_matching(dir.path(), &id, query)
                    .is_some(),
                "query {query:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn recursive_listing_does_not_follow_symlinked_directories() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        let filesystem = RealFilesystem;
        let (_, files, directories) = filesystem.open_workspace(dir.path()).unwrap();
        assert!(files.is_empty());
        assert!(directories.is_empty());
        assert!(
            filesystem
                .read_document(dir.path(), &DocumentId::parse("linked/secret.txt").unwrap())
                .is_err()
        );
    }
}
