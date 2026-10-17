//! SQLite adapter for application metadata. Document content remains on disk.

use directories::ProjectDirs;
use native_workspace_domain::Workspace;
use rusqlite::{Connection, params};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: i64 = 2;
pub const RECENT_LIMIT: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentWorkspace {
    pub root: PathBuf,
    pub name: String,
}

/// A confirmed editor layout. Document contents are never stored here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSession {
    pub root: PathBuf,
    pub documents: Vec<String>,
    pub active_document: Option<String>,
    pub preview_document: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not find an application data directory")]
    DataDirectoryUnavailable,
    #[error("metadata path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[error("unsupported metadata schema version: {0}")]
    UnsupportedSchema(i64),
    #[error("metadata I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite metadata error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub trait MetadataStorage: Send {
    fn session(&self) -> Result<Option<WorkspaceSession>, StorageError>;
    fn save_session(&mut self, session: &WorkspaceSession) -> Result<(), StorageError>;
    fn recent_workspaces(&self) -> Result<Vec<RecentWorkspace>, StorageError>;
    fn record_workspace(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Vec<RecentWorkspace>, StorageError>;
}

#[derive(Default)]
pub struct MemoryStorage {
    recent: Vec<RecentWorkspace>,
    session: Option<WorkspaceSession>,
}

impl MetadataStorage for MemoryStorage {
    fn session(&self) -> Result<Option<WorkspaceSession>, StorageError> {
        Ok(self.session.clone())
    }

    fn save_session(&mut self, session: &WorkspaceSession) -> Result<(), StorageError> {
        self.session = Some(session.clone());
        Ok(())
    }

    fn recent_workspaces(&self) -> Result<Vec<RecentWorkspace>, StorageError> {
        Ok(self.recent.clone())
    }

    fn record_workspace(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Vec<RecentWorkspace>, StorageError> {
        self.recent.retain(|item| item.root != workspace.root);
        self.recent.insert(
            0,
            RecentWorkspace {
                root: workspace.root.clone(),
                name: workspace.name.clone(),
            },
        );
        self.recent.truncate(RECENT_LIMIT);
        Ok(self.recent.clone())
    }
}

pub struct SqliteStorage {
    connection: Connection,
}

impl SqliteStorage {
    pub fn open_default() -> Result<Self, StorageError> {
        let path = if let Some(path) = std::env::var_os("NATIVE_WORKSPACE_STATE_DB") {
            PathBuf::from(path)
        } else {
            ProjectDirs::from("org", "nativeworkspace", "Native Workspace")
                .ok_or(StorageError::DataDirectoryUnavailable)?
                .data_local_dir()
                .join("metadata.sqlite3")
        };
        Self::open(&path)
    }

    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(StorageError::UnsupportedSchema(version));
        }
        if version < SCHEMA_VERSION {
            let transaction = connection.transaction()?;
            if version < 1 {
                transaction.execute_batch(include_str!("migrations/0001_recent_workspaces.sql"))?;
            }
            transaction.execute_batch(include_str!("migrations/0002_workspace_session.sql"))?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        Ok(Self { connection })
    }
}

impl MetadataStorage for SqliteStorage {
    fn session(&self) -> Result<Option<WorkspaceSession>, StorageError> {
        // One statement holds a consistent read snapshot even if another instance
        // replaces the layout between reading its header and document rows.
        let mut statement = self.connection.prepare(
            "SELECT s.root, s.active_document, s.preview_document, d.path
             FROM workspace_session s LEFT JOIN session_documents d ON s.id = 1
             WHERE s.id = 1 ORDER BY d.position",
        )?;
        let mut rows = statement.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let mut session = WorkspaceSession {
            root: PathBuf::from(row.get::<_, String>(0)?),
            active_document: row.get(1)?,
            preview_document: row.get(2)?,
            documents: Vec::new(),
        };
        if let Some(path) = row.get::<_, Option<String>>(3)? {
            session.documents.push(path);
        }
        while let Some(row) = rows.next()? {
            session.documents.push(row.get(3)?);
        }
        Ok(Some(session))
    }

    fn save_session(&mut self, session: &WorkspaceSession) -> Result<(), StorageError> {
        let root = session
            .root
            .to_str()
            .ok_or_else(|| StorageError::NonUtf8Path(session.root.clone()))?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO workspace_session(id, root, active_document, preview_document)
             VALUES (1, ?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET
             root=excluded.root, active_document=excluded.active_document,
             preview_document=excluded.preview_document",
            params![root, session.active_document, session.preview_document],
        )?;
        transaction.execute("DELETE FROM session_documents", [])?;
        for (position, path) in session.documents.iter().enumerate() {
            transaction.execute(
                "INSERT INTO session_documents(position, path) VALUES (?1, ?2)",
                params![position as i64, path],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn recent_workspaces(&self) -> Result<Vec<RecentWorkspace>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT path, name FROM recent_workspaces ORDER BY opened_order DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([RECENT_LIMIT as i64], |row| {
            Ok(RecentWorkspace {
                root: PathBuf::from(row.get::<_, String>(0)?),
                name: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn record_workspace(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Vec<RecentWorkspace>, StorageError> {
        let path = workspace
            .root
            .to_str()
            .ok_or_else(|| StorageError::NonUtf8Path(workspace.root.clone()))?;
        let transaction = self.connection.transaction()?;
        let next_order: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(opened_order), 0) + 1 FROM recent_workspaces",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO recent_workspaces(path, name, opened_order) VALUES (?1, ?2, ?3) \
             ON CONFLICT(path) DO UPDATE SET name=excluded.name, opened_order=excluded.opened_order",
            params![path, workspace.name, next_order],
        )?;
        transaction.execute(
            "DELETE FROM recent_workspaces WHERE path NOT IN (
                SELECT path FROM recent_workspaces ORDER BY opened_order DESC LIMIT ?1
            )",
            [RECENT_LIMIT as i64],
        )?;
        transaction.commit()?;
        self.recent_workspaces()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_and_persists_recent_workspaces_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let mut storage = SqliteStorage::open(&path).unwrap();
        let first = Workspace {
            root: dir.path().join("first"),
            name: "First".into(),
        };
        let second = Workspace {
            root: dir.path().join("second"),
            name: "Second".into(),
        };
        storage.record_workspace(&first).unwrap();
        storage.record_workspace(&second).unwrap();
        assert_eq!(
            storage
                .record_workspace(&first)
                .unwrap()
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        drop(storage);
        let reopened = SqliteStorage::open(&path).unwrap();
        assert_eq!(reopened.recent_workspaces().unwrap()[0].root, first.root);
        let version: i64 = reopened
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn rejects_future_schema_without_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 42).unwrap();
        drop(connection);
        assert!(matches!(
            SqliteStorage::open(&path),
            Err(StorageError::UnsupportedSchema(42))
        ));
    }

    #[test]
    fn retains_only_ten_recent_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = SqliteStorage::open(&dir.path().join("state.sqlite3")).unwrap();
        for index in 0..12 {
            storage
                .record_workspace(&Workspace {
                    root: dir.path().join(format!("workspace-{index}")),
                    name: format!("Workspace {index}"),
                })
                .unwrap();
        }
        let recent = storage.recent_workspaces().unwrap();
        assert_eq!(recent.len(), RECENT_LIMIT);
        assert_eq!(recent[0].name, "Workspace 11");
        assert_eq!(recent.last().unwrap().name, "Workspace 2");
    }
    #[test]
    fn migrates_v1_without_losing_recent_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("migrations/0001_recent_workspaces.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO recent_workspaces VALUES ('/old/workspace', 'Old', 7)",
                [],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        drop(connection);
        let storage = SqliteStorage::open(&path).unwrap();
        assert_eq!(
            storage.recent_workspaces().unwrap()[0].root,
            PathBuf::from("/old/workspace")
        );
        assert!(storage.session().unwrap().is_none());
        assert_eq!(
            storage
                .connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }

    #[test]
    fn session_layout_is_atomic_and_survives_reopening_and_empty_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let mut storage = SqliteStorage::open(&path).unwrap();
        let saved = WorkspaceSession {
            root: dir.path().into(),
            documents: vec![
                "second.txt".into(),
                "/outside/ü.txt".into(),
                "first.txt".into(),
            ],
            active_document: Some("first.txt".into()),
            preview_document: Some("second.txt".into()),
        };
        storage.save_session(&saved).unwrap();
        let mut invalid = saved.clone();
        invalid.active_document = Some("second.txt".into());
        invalid.documents.push("second.txt".into());
        assert!(storage.save_session(&invalid).is_err());
        drop(storage);
        let mut storage = SqliteStorage::open(&path).unwrap();
        assert_eq!(storage.session().unwrap(), Some(saved.clone()));
        let empty = WorkspaceSession {
            documents: Vec::new(),
            active_document: None,
            preview_document: None,
            ..saved
        };
        storage.save_session(&empty).unwrap();
        drop(storage);
        assert_eq!(
            SqliteStorage::open(&path).unwrap().session().unwrap(),
            Some(empty)
        );
    }
    #[test]
    fn concurrent_instances_never_read_a_mixed_session_layout() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let layout = |name: &str| WorkspaceSession {
            root: dir.path().join(name),
            documents: vec![name.into()],
            active_document: Some(name.into()),
            preview_document: None,
        };
        let first = layout("first.txt");
        let second = layout("second.txt");
        let mut writer = SqliteStorage::open(&path).unwrap();
        writer.save_session(&first).unwrap();
        let reader = SqliteStorage::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let writer_barrier = barrier.clone();
        let write = std::thread::spawn(move || {
            writer_barrier.wait();
            for index in 0..200 {
                writer
                    .save_session(if index % 2 == 0 { &first } else { &second })
                    .unwrap();
            }
        });
        barrier.wait();
        for _ in 0..200 {
            let saved = reader.session().unwrap().unwrap();
            assert_eq!(
                saved.root.file_name().unwrap().to_str().unwrap(),
                saved.documents[0]
            );
            assert_eq!(saved.active_document.as_ref(), saved.documents.first());
        }
        write.join().unwrap();
    }
}
