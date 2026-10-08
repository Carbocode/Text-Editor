# native-workspace

Three native frontends share one Rust application core: SwiftUI on macOS, GTK4/libadwaita on GNOME, and CXX-Qt/QML with a Kirigami variant on KDE.

Phase 1 opens a local directory, displays its full directory tree and regular files, opens and edits UTF-8 documents, shows their modified/saved state, and saves content to disk. The sidebar starts with folders closed and expands them on demand. File and folder icons use the bundled [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) associations; see [asset attribution](assets/material-icon-theme/ATTRIBUTION.md).

Phase 2 separates frontend commands, successful application events, authoritative state, and snapshots. The three native frontends still consume the same snapshot API.

Phase 3 adds a versioned SQLite metadata store for recent workspaces. Each frontend shows the same recent-workspace list and can reopen a directory from it. Document contents remain in their original files. The database location follows the platform application-data directory; `NATIVE_WORKSPACE_STATE_DB` overrides the path for development.

Phase 4 adds a search field inside each sidebar. The Rust application core matches relative paths and every occurrence in UTF-8 file contents without case sensitivity. Ripgrep's search crates filter unopened files; the core computes exact previews and editor positions, and searches unsaved buffers directly. A worker debounces queries for 150 ms and discards obsolete results. SQLite indexing is not used.

Phase 5 watches the full directory tree with `notify` for files added, removed, or changed outside the app. Frontends collect events every 250 ms; the core also reconciles every 30 seconds, or every two seconds if a native watcher cannot start. Clean open documents reload automatically, while local edits are preserved when the disk version changes. A conflict notice offers an explicit choice to reload from disk or overwrite it. Normal Save refuses to replace a changed disk version, including changes detected before the next check.

Run cargo test, cargo clippy, and cargo xtask doctor for the shared core. Build or start a frontend with cargo xtask build-macos, build-gnome, build-kde or their run-* counterparts. Set NATIVE_WORKSPACE_KIRIGAMI=1 when running the KDE variant on a host with Kirigami.

Measure listing and search latency with `cargo run -p native-workspace-application --release --example search_benchmark -- [workspace] [query]`. Without arguments it creates a temporary nested workspace with 1,000 files. Search timing includes the 150 ms debounce.

See [architecture](docs/architecture.md), [development](docs/development.md), [macOS](docs/macos.md), [GNOME](docs/gnome.md), and [KDE](docs/kde.md).
