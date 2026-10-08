# Architecture

```text
domain (document IDs and workspace values)
   ├── filesystem (local directory and UTF-8 file I/O)
   └── storage (SQLite metadata with versioned migrations)
          ↓
application (commands → events → state → snapshots)
   ├── ffi-swift (UniFFI) → SwiftUI
   ├── gnome (direct Rust) → GTK4/libadwaita
   └── kde (direct Rust) → CXX-Qt/QML and Kirigami
```

Application::dispatch_with_events(AppCommand) validates intent, performs any required filesystem I/O, and returns the resulting AppEvent values plus an AppSnapshot. AppState applies only successful events and owns the authoritative document content, dirty state, and revision. A command that has no effect produces no events or revision change. Application::dispatch(AppCommand) keeps the snapshot-only API used by all three frontends. These transient events are not a persisted event log. The Rust core has no toolkit dependency. Frontends choose their own file icons and controls.

The initial file tree includes regular .md and .txt files in the workspace root. Nested directories and symlinks are skipped. External file changes are not watched yet; reopening a workspace refreshes its file list when there are no unsaved edits. Dirty documents block replacing the workspace. See [ADR 0001](adr/0001-native-platform-frontends.md) and [ADR 0002](adr/0002-transient-application-events.md).

The storage adapter records only recent workspace paths, names, and order. `Application::enable_persistence_default` loads them before frontend interaction; normal workspace opening updates the database and publishes the new list in the same snapshot. `Application::new` uses in-memory metadata for isolated tests and smoke runs. Document bytes are never stored in SQLite. See [ADR 0003](adr/0003-sqlite-metadata.md).

Search is an application command and event. The application reads unopened files through the filesystem adapter when the query changes and publishes a filename result plus one `SearchResult` for each content occurrence. Each content result carries line, column, Unicode scalar offsets for GTK, and UTF-16 offsets for AppKit and Qt. Loaded document buffers take precedence over disk content, so unsaved edits update the results. `AppState` keeps the query and current results; frontends render and navigate to individual occurrences in their sidebars. Opening a different workspace clears the query. SQLite full-text indexing is not used.
