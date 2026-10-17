# native-workspace

Three native frontends share one Rust application core: SwiftUI on macOS, GTK4/libadwaita on GNOME, and Kirigami with CXX-Qt/QML on KDE.

Phase 1 opens a local directory, displays its full directory tree and regular files, opens and edits UTF-8 documents, shows their modified/saved state, and saves content to disk. The sidebar starts with folders closed and expands them on demand. All frontends' file/folder icons use the bundled [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) associations; see [asset attribution](assets/material-icon-theme/ATTRIBUTION.md). KDE uses Qt's standard tree controls with Kirigami icon/title content and the same Material associations. Its Breeze colors follow the OS light/dark preference and update without a restart.

Phase 2 separates frontend commands, successful application events, authoritative state, and snapshots. Each native window now owns one shared `Session`; editor behavior, scheduling, path projection and error publication are defined once in Rust. See [ADR 0004](docs/adr/0004-shared-session-and-editor-rules.md).

Home and Workspace use separate native windows in all three frontends. Home has its own window controls, folder chooser, recent-workspace list, and Resume Workspace action. Returning Home preserves the editor and sidebar state; KDE keeps window-manager decorations outside the content.

Phase 3 adds a versioned SQLite metadata store for recent workspaces. Each frontend shows the same recent-workspace list and can reopen a directory from it. Document contents remain in their original files. The database location follows the platform application-data directory; `NATIVE_WORKSPACE_STATE_DB` overrides the path for development.

Phase 4 adds a search field inside each sidebar. The Rust application core matches every occurrence in UTF-8 file contents without case sensitivity, searching open buffers first and then files from the workspace root downward on several threads. Matches appear as they are found; a newer query or an emptied field cancels the search. Files excluded by `.gitignore` and `.git` itself are skipped, and a search stops after 20,000 matches with a notice. A worker debounces queries for 150 ms; typing never searches on the UI thread. The Files name filter also runs on a debounced worker. SQLite indexing is not used.

Phase 5 watches the full directory tree with `notify` for files added, removed, or changed outside the app. Frontends collect events every 250 ms; the core reconciles on a filesystem worker every 30 seconds, or every two seconds if a native watcher cannot start. Directory scans and buffer checks never run in the UI polling handler; results superseded by edits or moves are discarded. Clean open documents reload automatically, while local edits are preserved when the disk version changes. A conflict notice offers an explicit choice to reload from disk or overwrite it. Normal Save refuses to replace a changed disk version, including changes detected before the next check.

The editors use a monospaced font, line numbers, and a clickable and draggable minimap. The minimap renders 1×2 character glyphs with indentation and syntax colors, samples long documents to fit, and overlays the visible viewport. The glyph atlas comes from VS Code; see [attribution](assets/MINIMAP_ATTRIBUTION.md). A shared Rust syntax crate identifies token ranges for common code formats and Markdown; the native frontends color those ranges without changing document content. On macOS, text styling is limited to visible lines and the minimap raster is prepared in the background so large files stay responsive.

Files and folders support inline renaming (F2 or the sidebar context menu, Return to confirm, Escape to cancel) and drag-and-drop moves in all three frontends. Rename cancellation follows the native editor's focus events; no frontend intercepts clicks across the window to cancel a draft. A drag preview follows the pointer, the destination is highlighted, and hovering inside a closed folder for 450 ms expands it without requiring the pointer to stop. Drag-opened folders close when hovering a sibling branch or cancelling the drag; a successful drop keeps the destination ancestors open. Dropping on a file uses its parent folder; the workspace drop area moves entries to the root. Renaming or moving a folder updates every open document beneath it while preserving unsaved buffers and external-change conflicts. Existing destinations are never replaced, and folders cannot be moved inside themselves. Releasing a drag removes its preview immediately, without a native landing or return animation. Drops update the tree in the release handler in all three frontends while the filesystem move runs in the background. A progress indicator appears only if confirmation takes longer than 300 ms. A failed confirmation restores the original paths and reports the error; edits made during the move are retained and saves wait for the confirmed path. On macOS, drag feedback updates independently of the workspace view, drop validation uses the shared Rust rules, and filesystem polling and relocation confirmations run in the background. Inline rename displays optimistically, restores the original name if confirmation fails, preserves editor selection, and previews the new icon after a 150 ms typing debounce.

Phase 6 completes file management with Delete… in the Files and Search context menus on all three frontends. A native confirmation explains that deletion is permanent, including folder contents. The core refuses to delete any file or folder containing unsaved documents, even when those documents are inactive; save them first. Deletion runs on a worker while editing is paused. On success, affected open documents close and the tree and search results update; errors preserve buffers and are shown in the frontend. Workspace-relative paths are validated, symlinked ancestors are rejected, and symlinks inside a deleted directory are unlinked without following their targets.

Phase 7 shows open documents as ordered, horizontally scrollable tabs in all three frontends. Tabs mark unsaved changes and preserve buffers when switching files. Closing a modified tab offers Save, Cancel, or Discard Changes; a failed save leaves the tab open. Closing the active tab selects its neighbor. Save All writes modified open documents in tab order and stops on the first error or external-change conflict, preserving completed saves and all remaining buffers. Use Command-Shift-S on macOS or Ctrl-Shift-S on GNOME and KDE. Renames, moves and deletions update the open tabs. Switching documents clears native undo history; per-tab undo history is not retained. Renaming the active document preserves its undo history and selection. Side-by-side editing remains a future extension.

Phase 8 accepts operating-system file drops in all three frontends. Dropping local UTF-8 files onto the editor or tab area opens permanent tabs, including files outside the workspace. External tabs keep their own absolute identities and support editing, Save All, reload and conflict detection without adding external folders to the workspace tree or search. Dropping files onto the Files tree moves them into the indicated folder (a file row uses its parent; background uses the workspace root). Native previews, destination highlighting and delayed folder expansion accompany the drop. Existing destinations and retained unsaved buffers are protected; successful files in a batch remain committed if a later file fails. Moves run on the shared worker with editing paused, preserving open buffers and editor identity. Symlink leaves, folders and non-local URLs are rejected. Cross-filesystem moves report an error and retain their source. See [ADR 0005](docs/adr/0005-external-document-identities.md).

Phase 9 opens local files and folders through operating-system requests. macOS accepts drops onto the Dock or Finder application icon and Open With, including cold launch. GNOME handles file-manager/launcher requests through GApplication, forwarding later opens to the running instance. KDE accepts launch paths and Qt file-open events on platforms that deliver them; Linux launcher opens start a new KDE process. Desktop entry templates are provided for both Linux frontends. Each instance retains one workspace: folders select it in delivery order, and files open permanent tabs; without a workspace, the first file opens its parent as the workspace. Reopening the current folder preserves its tabs, and switching folders refuses unsaved changes. Ordered requests run on the shared worker and retain successful earlier steps if a later item fails. See [desktop integration and verification](docs/development.md#application-icon-and-open-with-verification).

Phase 10 adds find and replace to the **Search** sidebar in all three frontends.
The `Aa`, whole-word and `.*` controls configure case sensitivity, word boundaries
and regular expressions. Search details provide include/exclude globs, an ignore-file
toggle and an Open Editors scope. Searches can be refreshed, cleared or cancelled.
Replace supports preserving case, regex capture references and replacing one match,
one file or all results. Every replacement first shows a shared preview. Open buffers
remain unsaved; closed files are written on confirmation. A changed buffer or disk
file rejects a stale preview, and incomplete searches must be refreshed or narrowed.
See [search and replacement verification](docs/development.md#search-and-replacement-verification)
for supported syntax and checks.

Phase 11 restores the last workspace automatically on normal launch, including
ordered tabs, the active document, the preview tab and open external files. Files
are read from their current disk contents. Unavailable tabs are skipped with an
error while the rest reopen; an unavailable workspace returns to Home. Initial
OS file/folder requests take precedence over restoration. Normal quit offers
Save All, Cancel or Discard Changes for unsaved documents, and failed saves or
running filesystem operations keep the app open. SQLite stores only the confirmed
layout, never buffer contents; unsaved crash recovery, undo history, cursor/scroll
positions and expanded folders are not restored. See
[session restoration verification](docs/development.md#session-restoration-verification)
and [ADR 0007](docs/adr/0007-session-restoration.md).

## Planned phases

The following work is planned:

12. **Integrated terminal:** run commands in the current workspace from each native frontend.
13. **Basic Git integration:** show changed files and diffs before committing.
14. **Language Server Protocol support:** add completion, diagnostics, and navigation to definitions through language servers.

Use `cargo xtask verify core` for shared Rust checks and `cargo xtask doctor` to inspect local tools. For `macos`, `gnome` or `kde`, `cargo xtask build <frontend>` compiles, `cargo xtask start <frontend>` opens the existing build, and `cargo xtask run <frontend>` composes both without tests. Run tests explicitly with `cargo xtask test <target>`. The previous `build-*` and `run-*` commands remain aliases. See the [command reference](docs/development.md#terminal-commands) or `cargo xtask help`; add `--dry-run` to inspect a task. The KDE frontend uses Kirigami and Breeze on every host, including macOS; install Kirigami, qqc2-breeze-style and Breeze Icons for its Qt installation.

The macOS and GNOME development launchers start a new instance after building, so an already running process cannot silently receive the launch and keep executing an older binary. Existing editor sessions stay open.

Measure listing and search latency with `cargo run -p native-workspace-application --release --example search_benchmark -- [workspace] [query]`. Without arguments it creates a temporary nested workspace with 1,000 files. Timings for the first streamed result and the complete search include the 150 ms debounce.

See [architecture](docs/architecture.md), [development](docs/development.md), [macOS](docs/macos.md), [GNOME](docs/gnome.md), and [KDE](docs/kde.md).

The KDE native pointer smoke test uses `target/debug/native-workspace-kde --smoke-drag=/path/to/disposable-fixture` after `cargo xtask build-kde`. It requires QtTest and a fixture containing `README.md` and `src/main.rs`; it moves the README into `src` and back to the root. With `QT_QPA_PLATFORM=offscreen`, run it with the default Breeze style and an explicit `QT_QUICK_CONTROLS_STYLE=Fusion` to check drag targeting, hover expansion during movement, and drop persistence.

Developer Tools includes a shared native console, persistent recordings, resource monitoring and a **Timeline** with event type filters and drag/drop spans. Open it in the editor to compare preview removal, callback completion, destination readiness, filesystem work and confirmation, together with sampled UI event-loop delays. See [implementation and manual verification](docs/devtools/IMPLEMENTATION.md) for startup, measurement definitions and limits; the [architecture plan](docs/devtools/README.md) also lists remaining work.
