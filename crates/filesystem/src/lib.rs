//! The concrete filesystem boundary for local workspaces.

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{SearcherBuilder, sinks::UTF8};
use native_workspace_domain::{DocumentId, Workspace};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

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
        watcher.watch(root, RecursiveMode::NonRecursive)?;
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

pub trait WorkspaceFilesystem: Send {
    fn open_workspace(&self, path: &Path) -> Result<(Workspace, Vec<DocumentId>), FilesystemError>;
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
}

#[derive(Debug, Default)]
pub struct RealFilesystem;

impl RealFilesystem {
    fn document_path(root: &Path, id: &DocumentId) -> Result<PathBuf, FilesystemError> {
        let path = root.join(id.as_str());
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
    fn open_workspace(&self, path: &Path) -> Result<(Workspace, Vec<DocumentId>), FilesystemError> {
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
        for entry in fs::read_dir(&root).map_err(|source| FilesystemError::Io {
            path: root.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| FilesystemError::Io {
                path: root.clone(),
                source,
            })?;
            if !entry
                .file_type()
                .map_err(|source| FilesystemError::Io {
                    path: entry.path(),
                    source,
                })?
                .is_file()
            {
                continue;
            }
            if let Ok(name) = entry.file_name().into_string()
                && let Ok(id) = DocumentId::parse(name)
            {
                documents.push(id);
            }
        }
        documents.sort();
        Ok((Workspace { root, name }, documents))
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
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(true)
            // Rust's lowercase expansion can match text that Unicode regex case
            // folding does not (for example, `i` in `İ`). Inspect such files
            // with the exact matcher in the application core.
            .build(&format!("(?:{}|[^\\x00-\\x7F])", regex::escape(needle)))
            .ok()?;
        let mut found = false;
        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_path(
                &matcher,
                &path,
                UTF8(|_, _| {
                    found = true;
                    Ok::<bool, io::Error>(false)
                }),
            )
            .ok()?;
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
        let path = root.join(id.as_str());
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
    fn lists_only_flat_text_files_and_round_trips_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "before").unwrap();
        fs::write(dir.path().join("a.md"), "# A").unwrap();
        fs::write(dir.path().join("ignored.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("nested/note.md"), "hidden").unwrap();
        let filesystem = RealFilesystem;
        let (workspace, ids) = filesystem.open_workspace(dir.path()).unwrap();
        assert_eq!(
            ids.iter().map(DocumentId::as_str).collect::<Vec<_>>(),
            ["a.md", "b.txt"]
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
}
