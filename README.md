# native-workspace

Phase 0 tests one Rust application core through three native frontends: SwiftUI on macOS, GTK4/libadwaita on GNOME, and CXX-Qt/QML on KDE. Each window displays a counter and sends Increment to the same `Application` implementation.

Run `cargo test`, `cargo clippy`, and `cargo xtask doctor`. The core is the default Cargo workspace member, so these commands do not require GUI libraries. See [architecture](docs/architecture.md), [macOS](docs/macos.md), [GNOME](docs/gnome.md), and [KDE](docs/kde.md) setup notes.
