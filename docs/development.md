# Development

Rust 1.99.0 is pinned in rust-toolchain.toml, and Cargo.lock pins crate resolution. Core checks are cargo fmt --check, cargo clippy -- -D warnings, and cargo test. Default workspace members avoid GUI dependencies.

Material Icon Theme SVGs, 64 px PNGs, and the association manifest are bundled in `assets/material-icon-theme`. To update them, prepare an upstream checkout as described in `tools/update_material_icons.py`, then run `python3 tools/update_material_icons.py /path/to/vscode-material-icon-theme`. The script requires `rsvg-convert` and regenerates the GNOME embedded icon lookup table. Review the pinned upstream revision in `ATTRIBUTION.md` when updating.

Use cargo xtask doctor to inspect local tools, or cargo xtask doctor --strict macos|gnome|kde for one frontend. Build with cargo xtask build-macos, cargo xtask build-gnome, or cargo xtask build-kde; run with the corresponding run-* command.

The initial vertical slice covers opening a directory, listing files, opening, editing, and saving UTF-8 documents. The frontends now collect queued `notify` events every 250 ms and reconcile periodically for external changes. Clean open documents reload automatically. If local edits and disk content diverge, normal Save refuses to overwrite the disk; the user can reload or explicitly overwrite it. The diagnostic counter remains in the core and smoke tests, but is hidden from normal windows.

Normal frontend launches open a SQLite metadata database in the platform application-data directory. Set `NATIVE_WORKSPACE_STATE_DB=/absolute/path/metadata.sqlite3` to use an isolated database. `cargo test` uses temporary files; GUI smoke modes use in-memory metadata. Migration files live in `crates/storage/src/migrations`, with schema version tracked by SQLite `PRAGMA user_version`.

The sidebar search matches relative paths and UTF-8 file contents throughout the directory tree, ignoring case and surrounding whitespace in the query. Every content occurrence has a separate result with line, column, and a short preview; selecting it opens the file and highlights that text. Unsaved document buffers take precedence over disk content. Files above 1 MiB and unreadable files are skipped for content search, while their paths remain searchable. An empty query shows every listed document. Search runs in a worker and does not use SQLite.
