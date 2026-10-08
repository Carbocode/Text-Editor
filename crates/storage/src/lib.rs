//! SQLite adapter for application metadata. Document content remains on disk.

use directories::ProjectDirs;
use native_workspace_domain::Workspace;
use rusqlite::{Connection, params};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: i64 = 1;
pub const RECENT_LIMIT: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentWorkspace {
    pub root: PathBuf,
    pub name: String,
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
    fn recent_workspaces(&self) -> Result<Vec<RecentWorkspace>, StorageError>;
    fn record_workspace(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Vec<RecentWorkspace>, StorageError>;
}

#[derive(Default)]
pub struct MemoryStorage {
    recent: Vec<RecentWorkspace>,
}

impl MetadataStorage for MemoryStorage {
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
        match version {
            0 => {
                let transaction = connection.transaction()?;
                transaction.execute_batch(include_str!("migrations/0001_recent_workspaces.sql"))?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
            }
            SCHEMA_VERSION => {}
            other => return Err(StorageError::UnsupportedSchema(other)),
        }
        Ok(Self { connection })
    }
}

impl MetadataStorage for SqliteStorage {
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
}
