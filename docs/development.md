# Development

Rust 1.99.0 is pinned in rust-toolchain.toml, and Cargo.lock pins crate resolution. Core checks are cargo fmt --check, cargo clippy -- -D warnings, and cargo test. Default workspace members avoid GUI dependencies.

Use cargo xtask doctor to inspect local tools, or cargo xtask doctor --strict macos|gnome|kde for one frontend. Build with cargo xtask build-macos, cargo xtask build-gnome, or cargo xtask build-kde; run with the corresponding run-* command.

The Phase 1 vertical slice covers opening a directory, listing root-level .md/.txt files, opening, editing, and saving UTF-8 documents. There is no watcher or conflict resolution for external edits yet. The diagnostic counter remains in the core and smoke tests, but is hidden from normal windows.
