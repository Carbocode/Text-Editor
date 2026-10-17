//! The concrete filesystem boundary for local workspaces.

use native_workspace_domain::{DocumentId, Workspace};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

/// Larger files are listed but never searched.
pub const MAX_SEARCH_BYTES: u64 = 1024 * 1024;

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

    /// Consumes queued notifications, ignoring pure access events.
    pub fn drain_changes(&self) -> WatcherBatch {
        let mut batch = WatcherBatch::default();
        while let Ok(event) = self.events.try_recv() {
            let relevant = match event {
                Ok(event) => !matches!(event.kind, EventKind::Access(_)),
                Err(_) => {
                    batch.errors += 1;
                    true
                }
            };
            batch.events += u64::from(relevant);
        }
        batch
    }
}

/// Notifications collected since the last drain.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatcherBatch {
    /// Relevant events, including errors: any of them warrants a rescan.
    pub events: u64,
    pub errors: u64,
}

impl WatcherBatch {
    pub fn changed(&self) -> bool {
        self.events > 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error("invalid workspace directory: {0}")]
    InvalidWorkspace(PathBuf),
    #[error("document is not a regular file or has an unsafe path: {0}")]
    InvalidDocument(PathBuf),
    #[error("filesystem error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The files and folders of a workspace, each list sorted by path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceListing {
    pub workspace: Workspace,
    pub documents: Vec<DocumentId>,
    pub directories: Vec<String>,
}

/// A canonical local item supplied by the operating system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenTarget {
    File(PathBuf),
    Directory(PathBuf),
}

/// Every filesystem access of the application. Implementations decide
/// everything about the disk; callers never touch `std::fs` directly.
pub trait WorkspaceFilesystem: Send + Sync {
    /// Canonicalizes `path` and lists regular files and folders below it,
    /// skipping symlinks.
    fn open_workspace(&self, path: &Path) -> Result<WorkspaceListing, FilesystemError>;
    /// Resolve an absolute file or directory; reject symlink leaves and special files.
    fn resolve_open_target(&self, path: &Path) -> Result<OpenTarget, FilesystemError>;
    /// Resolve an absolute regular file selected by the user. Reject symlink leaves.
    fn resolve_file(&self, path: &Path) -> Result<PathBuf, FilesystemError>;
    /// Move a resolved regular file into the workspace without replacement.
    fn move_file(
        &self,
        source: &Path,
        root: &Path,
        target: &DocumentId,
    ) -> Result<(), FilesystemError>;
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError>;
    /// UTF-8 content of at most [`MAX_SEARCH_BYTES`], or nothing. Folders
    /// recorded in `verified` are not checked again, so one workspace search
    /// checks each folder once.
    fn read_document_for_search(
        &self,
        root: &Path,
        id: &DocumentId,
        verified: &mut HashSet<String>,
    ) -> Option<String>;
    /// Writes an existing regular file, or creates a missing one.
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError>;
    /// Renames atomically without ever replacing an existing entry.
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError>;
    /// Removes an entry recursively, unlinking child symlinks without
    /// following them.
    fn delete_entry(&self, root: &Path, id: &DocumentId) -> Result<(), FilesystemError>;
    /// Creates an empty file or folder without replacing anything.
    fn create_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        directory: bool,
    ) -> Result<(), FilesystemError>;
}

/// Attaches the path to an I/O error.
fn at(path: &Path) -> impl FnOnce(io::Error) -> FilesystemError + '_ {
    move |source| FilesystemError::Io {
        path: path.to_path_buf(),
        source,
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

/// UTF-8 content of at most `MAX_SEARCH_BYTES`; otherwise nothing.
fn read_bounded_utf8(file: fs::File) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(MAX_SEARCH_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SEARCH_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[derive(Debug, Default)]
pub struct RealFilesystem;

impl RealFilesystem {
    fn safe_document_location(root: &Path, id: &DocumentId) -> Result<PathBuf, FilesystemError> {
        let path = root.join(id.as_str());
        let mut parent = if id.is_external() {
            PathBuf::from("/")
        } else {
            root.to_path_buf()
        };
        for component in Path::new(id.as_str())
            .parent()
            .into_iter()
            .flat_map(Path::components)
        {
            parent.push(component);
            if !fs::symlink_metadata(&parent)
                .map_err(at(&parent))?
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
        let metadata = fs::symlink_metadata(&path).map_err(at(&path))?;
        if !metadata.file_type().is_file() {
            return Err(FilesystemError::InvalidDocument(path));
        }
        Ok(path)
    }
}

impl WorkspaceFilesystem for RealFilesystem {
    fn open_workspace(&self, path: &Path) -> Result<WorkspaceListing, FilesystemError> {
        let root = fs::canonicalize(path).map_err(at(path))?;
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
            for entry in fs::read_dir(&directory).map_err(at(&directory))? {
                let entry = entry.map_err(at(&directory))?;
                let file_type = entry.file_type().map_err(at(&entry.path()))?;
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
        Ok(WorkspaceListing {
            workspace: Workspace { root, name },
            documents,
            directories,
        })
    }

    fn resolve_open_target(&self, path: &Path) -> Result<OpenTarget, FilesystemError> {
        if !path.is_absolute() {
            return Err(FilesystemError::InvalidDocument(path.to_path_buf()));
        }
        let kind = fs::symlink_metadata(path).map_err(at(path))?.file_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(FilesystemError::InvalidDocument(path.to_path_buf()));
        }
        let resolved = fs::canonicalize(path).map_err(at(path))?;
        Ok(if kind.is_dir() {
            OpenTarget::Directory(resolved)
        } else {
            OpenTarget::File(resolved)
        })
    }

    fn resolve_file(&self, path: &Path) -> Result<PathBuf, FilesystemError> {
        match self.resolve_open_target(path)? {
            OpenTarget::File(path) => Ok(path),
            OpenTarget::Directory(_) => Err(FilesystemError::InvalidDocument(path.to_path_buf())),
        }
    }

    fn move_file(
        &self,
        source: &Path,
        root: &Path,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        let source = self.resolve_file(source)?;
        if target.is_external() {
            return Err(FilesystemError::InvalidDocument(root.join(target.as_str())));
        }
        let target = Self::safe_document_location(root, target)?;
        rename_without_replacing(&source, &target).map_err(at(&target))
    }

    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        let path = Self::document_path(root, id)?;
        fs::read_to_string(&path).map_err(at(&path))
    }

    #[cfg(not(unix))]
    fn read_document_for_search(
        &self,
        root: &Path,
        id: &DocumentId,
        _verified: &mut HashSet<String>,
    ) -> Option<String> {
        let path = Self::document_path(root, id).ok()?;
        read_bounded_utf8(fs::File::open(path).ok()?)
    }

    #[cfg(unix)]
    fn read_document_for_search(
        &self,
        root: &Path,
        id: &DocumentId,
        verified: &mut HashSet<String>,
    ) -> Option<String> {
        use std::os::unix::fs::OpenOptionsExt;
        // Same guarantees as `document_path`: no symlinked ancestor folders
        // and a regular final file, with one check per folder per search.
        let relative = id.as_str();
        let mut end = 0;
        while let Some(separator) = relative[end..].find('/') {
            end += separator;
            if !verified.contains(&relative[..end]) {
                if !fs::symlink_metadata(root.join(&relative[..end]))
                    .ok()?
                    .file_type()
                    .is_dir()
                {
                    return None;
                }
                verified.insert(relative[..end].to_owned());
            }
            end += 1;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join(relative))
            .ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_SEARCH_BYTES {
            return None;
        }
        read_bounded_utf8(file)
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
                fs::write(&path, content).map_err(at(&path))
            }
            Ok(_) => Err(FilesystemError::InvalidDocument(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| file.write_all(content.as_bytes()))
                .map_err(at(&path)),
            Err(source) => Err(FilesystemError::Io { path, source }),
        }
    }

    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        if id.is_external() || target.is_external() {
            return Err(FilesystemError::InvalidDocument(root.join(id.as_str())));
        }
        let source_path = Self::safe_document_location(root, id)?;
        let target_path = Self::safe_document_location(root, target)?;
        let metadata = fs::symlink_metadata(&source_path).map_err(at(&source_path))?;
        if !(metadata.is_file() || metadata.is_dir()) {
            return Err(FilesystemError::InvalidDocument(source_path));
        }
        rename_without_replacing(&source_path, &target_path).map_err(at(&target_path))
    }

    fn delete_entry(&self, root: &Path, id: &DocumentId) -> Result<(), FilesystemError> {
        if id.is_external() {
            return Err(FilesystemError::InvalidDocument(root.join(id.as_str())));
        }
        let path = Self::safe_document_location(root, id)?;
        let metadata = fs::symlink_metadata(&path).map_err(at(&path))?;
        let result = if metadata.is_dir() {
            fs::remove_dir_all(&path)
        } else if metadata.is_file() {
            fs::remove_file(&path)
        } else {
            return Err(FilesystemError::InvalidDocument(path));
        };
        result.map_err(at(&path))
    }

    fn create_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        directory: bool,
    ) -> Result<(), FilesystemError> {
        if id.is_external() {
            return Err(FilesystemError::InvalidDocument(root.join(id.as_str())));
        }
        let path = Self::safe_document_location(root, id)?;
        let result = if directory {
            fs::create_dir(&path)
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map(drop)
        };
        result.map_err(at(&path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn deleting_folders_unlinks_child_symlinks_and_rejects_symlinked_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "keep").unwrap();
        fs::create_dir_all(dir.path().join("folder/nested/empty")).unwrap();
        fs::write(dir.path().join("folder/nested/file"), "remove").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("folder/link")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("alias")).unwrap();
        for path in ["alias", "alias/secret"] {
            assert!(
                RealFilesystem
                    .delete_entry(dir.path(), &DocumentId::parse(path).unwrap())
                    .is_err()
            );
        }
        RealFilesystem
            .delete_entry(dir.path(), &DocumentId::parse("folder").unwrap())
            .unwrap();
        assert!(!dir.path().join("folder").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "keep"
        );
        assert!(
            RealFilesystem
                .delete_entry(dir.path(), &DocumentId::parse("missing").unwrap())
                .is_err()
        );
    }

    #[test]
    fn moving_os_files_never_replaces_even_an_unlisted_destination() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("note");
        fs::write(&source, "source").unwrap();
        fs::write(root.path().join("note"), "destination").unwrap();
        let target = DocumentId::parse("note").unwrap();
        assert!(
            RealFilesystem
                .move_file(&source, root.path(), &target)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(
            fs::read_to_string(root.path().join("note")).unwrap(),
            "destination"
        );
        fs::remove_file(root.path().join("note")).unwrap();
        RealFilesystem
            .move_file(&source, root.path(), &target)
            .unwrap();
        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(root.path().join("note")).unwrap(),
            "source"
        );
    }

    #[cfg(unix)]
    #[test]
    fn os_file_drops_reject_symlink_leaves_and_destination_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("source");
        fs::write(&source, "keep").unwrap();
        std::os::unix::fs::symlink(&source, outside.path().join("alias")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        assert!(
            RealFilesystem
                .resolve_file(&outside.path().join("alias"))
                .is_err()
        );
        assert!(RealFilesystem.resolve_file(outside.path()).is_err());
        assert!(
            RealFilesystem
                .move_file(
                    &source,
                    root.path(),
                    &DocumentId::parse("linked/new").unwrap()
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), "keep");
        assert!(!outside.path().join("new").exists());
    }

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
        let WorkspaceListing {
            workspace,
            documents: ids,
            directories,
        } = filesystem.open_workspace(dir.path()).unwrap();
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
            filesystem.open_workspace(dir.path()).unwrap().documents,
            std::slice::from_ref(&id)
        );
        assert!(
            filesystem
                .read_document_for_search(dir.path(), &id, &mut HashSet::new())
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_reads_check_each_folder_and_never_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("src/link.txt"),
        )
        .unwrap();
        let filesystem = RealFilesystem;
        let mut verified = HashSet::new();
        let mut read = |name: &str| {
            filesystem.read_document_for_search(
                dir.path(),
                &DocumentId::parse(name).unwrap(),
                &mut verified,
            )
        };
        assert_eq!(read("src/main.rs").as_deref(), Some("fn main() {}"));
        assert_eq!(read("src/link.txt"), None);
        assert_eq!(read("linked/secret.txt"), None);
        assert!(verified.contains("src") && !verified.contains("linked"));
    }

    #[cfg(unix)]
    #[test]
    fn recursive_listing_does_not_follow_symlinked_directories() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        let filesystem = RealFilesystem;
        let WorkspaceListing {
            documents: files,
            directories,
            ..
        } = filesystem.open_workspace(dir.path()).unwrap();
        assert!(files.is_empty());
        assert!(directories.is_empty());
        assert!(
            filesystem
                .read_document(dir.path(), &DocumentId::parse("linked/secret.txt").unwrap())
                .is_err()
        );
    }
}
