# ADR 0003: SQLite for application metadata

Use a dedicated `storage` adapter for SQLite. The domain defines workspace values and document IDs without a database dependency. The filesystem remains authoritative for document contents.

The first migration creates `recent_workspaces`, keyed by canonical path. Opening a workspace moves it to the front of the ten displayed entries. Reopening the application loads the list without reopening a document or advancing the application revision. SQLite `PRAGMA user_version` tracks migrations; a database from a newer application version is rejected instead of changed.

The database is created only when persistence is explicitly enabled during a normal frontend launch. Tests and smoke modes can use in-memory metadata or a temporary database. Future metadata, such as bookmarks and tags, can use later migrations without changing document storage.
