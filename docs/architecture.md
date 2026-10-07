# Architecture

```text
domain (document IDs and workspace values)
   ↓
filesystem (local directory and UTF-8 file I/O)
   ↓
application (commands, document state, snapshots)
   ├── ffi-swift (UniFFI) → SwiftUI
   ├── gnome (direct Rust) → GTK4/libadwaita
   └── kde (direct Rust) → CXX-Qt/QML and Kirigami
```

Application::dispatch(AppCommand) owns the authoritative document content, dirty state, save operation, and revision. Each frontend renders AppSnapshot and translates native UI events into commands. The Rust core has no toolkit dependency. Frontends choose their own file icons and controls.

The initial file tree includes regular .md and .txt files in the workspace root. Nested directories and symlinks are skipped. External file changes are not watched yet; reopening a workspace refreshes its file list when there are no unsaved edits. Dirty documents block replacing the workspace. See [ADR 0001](adr/0001-native-platform-frontends.md).
