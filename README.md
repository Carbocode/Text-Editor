# native-workspace

Three native frontends share one Rust application core: SwiftUI on macOS, GTK4/libadwaita on GNOME, and CXX-Qt/QML with a Kirigami variant on KDE.

Phase 1 opens a local directory, displays its full directory tree and regular files, opens and edits UTF-8 documents, shows their modified/saved state, and saves content to disk. The sidebar starts with folders closed and expands them on demand. File and folder icons use the bundled [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) associations; see [asset attribution](assets/material-icon-theme/ATTRIBUTION.md).

Phase 2 separates frontend commands, successful application events, authoritative state, and snapshots. The three native frontends still consume the same snapshot API.

Phase 3 adds a versioned SQLite metadata store for recent workspaces. Each frontend shows the same recent-workspace list and can reopen a directory from it. Document contents remain in their original files. The database location follows the platform application-data directory; `NATIVE_WORKSPACE_STATE_DB` overrides the path for development.

Phase 4 adds a search field inside each sidebar. The Rust application core matches relative paths and every occurrence in UTF-8 file contents without case sensitivity. Ripgrep's search crates filter unopened files; the core computes exact previews and editor positions, and searches unsaved buffers directly. A worker debounces queries for 150 ms and discards obsolete results. SQLite indexing is not used.

Phase 5 watches the full directory tree with `notify` for files added, removed, or changed outside the app. Frontends collect events every 250 ms; the core also reconciles every 30 seconds, or every two seconds if a native watcher cannot start. Clean open documents reload automatically, while local edits are preserved when the disk version changes. A conflict notice offers an explicit choice to reload from disk or overwrite it. Normal Save refuses to replace a changed disk version, including changes detected before the next check.

The editors use a monospaced font, line numbers, and a clickable and draggable minimap. The minimap renders 1×2 character glyphs with indentation and syntax colors, samples long documents to fit, and overlays the visible viewport. The glyph atlas comes from VS Code; see [attribution](assets/MINIMAP_ATTRIBUTION.md). A shared Rust syntax crate identifies token ranges for common code formats and Markdown; the native frontends color those ranges without changing document content. On macOS, text styling is limited to visible lines and the minimap raster is prepared in the background so large files stay responsive.

## Planned phases

The following phases are proposed and have not been implemented yet:

6. **Complete file management:** rename, move, and delete files and folders, including safe handling of open documents and unsaved changes.
7. **Edit multiple files:** show open documents as tabs in all three frontends, mark unsaved changes, switch and close tabs safely, and add Save All. The core already preserves unsaved content when switching documents; this phase makes open documents explicit. Side-by-side editing can follow as a separate extension.
8. **Find and replace:** replace matches in the current document and across the workspace, with a preview before workspace-wide changes.
9. **Restore sessions:** reopen the last workspace and its open documents after restarting the app.
10. **Integrated terminal:** run commands in the current workspace from each native frontend.
11. **Basic Git integration:** show changed files and diffs before committing.
12. **Language Server Protocol support:** add completion, diagnostics, and navigation to definitions through language servers.

Run cargo test, cargo clippy, and cargo xtask doctor for the shared core. Build or start a frontend with cargo xtask build-macos, build-gnome, build-kde or their run-* counterparts. Set NATIVE_WORKSPACE_KIRIGAMI=1 when running the KDE variant on a host with Kirigami.

Measure listing and search latency with `cargo run -p native-workspace-application --release --example search_benchmark -- [workspace] [query]`. Without arguments it creates a temporary nested workspace with 1,000 files. Search timing includes the 150 ms debounce.

See [architecture](docs/architecture.md), [development](docs/development.md), [macOS](docs/macos.md), [GNOME](docs/gnome.md), and [KDE](docs/kde.md).
