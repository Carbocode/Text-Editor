# Development

This repository pins Rust 1.99.0 via `rust-toolchain.toml`. `Cargo.lock` pins crate resolution. Core checks use `cargo fmt --check`, `cargo clippy`, and `cargo test`; default workspace members avoid GTK, Qt, and Xcode dependencies. Run `cargo xtask doctor` to inspect local tools. Strict checks are available as `cargo xtask doctor --strict macos|gnome|kde`.

The GUI packages are separate commands: `cargo xtask build-macos`, `cargo xtask build-gnome`, and `cargo xtask build-kde`. The diagnostic is the entire Phase 0 API; Phase 1 is intentionally not started.
