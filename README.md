# native-workspace

Three native frontends share one Rust application core: SwiftUI on macOS, GTK4/libadwaita on GNOME, and CXX-Qt/QML with a Kirigami variant on KDE.

Phase 1 opens a local directory, lists its root-level Markdown and text files, opens and edits a document, shows its modified/saved state, and saves UTF-8 content to disk. README.md, other .md files, and .txt files use icons from [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme); see [asset attribution](assets/material-icon-theme/ATTRIBUTION.md).

Phase 2 separates frontend commands, successful application events, authoritative state, and snapshots. The three native frontends still consume the same snapshot API.

Phase 3 adds a versioned SQLite metadata store for recent workspaces. Each frontend shows the same recent-workspace list and can reopen a directory from it. Document contents remain in their original files. The database location follows the platform application-data directory; `NATIVE_WORKSPACE_STATE_DB` overrides the path for development.

Run cargo test, cargo clippy, and cargo xtask doctor for the shared core. Build or start a frontend with cargo xtask build-macos, build-gnome, build-kde or their run-* counterparts. Set NATIVE_WORKSPACE_KIRIGAMI=1 when running the KDE variant on a host with Kirigami.

See [architecture](docs/architecture.md), [development](docs/development.md), [macOS](docs/macos.md), [GNOME](docs/gnome.md), and [KDE](docs/kde.md).
