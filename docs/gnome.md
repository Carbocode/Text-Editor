# GNOME development

The GNOME app uses GTK4 and libadwaita. `cargo xtask doctor --strict gnome` checks for Rust, Cargo, `pkg-config`, `gtk4`, and `libadwaita-1`. Once these system libraries are discoverable, run `cargo xtask build-gnome` and `cargo xtask run-gnome`.

On the macOS 27 bootstrap host, `brew install pkgconf gtk4 libadwaita` provided `pkgconf` 3.0.7, GTK4 4.24.1, and libadwaita 1.10.0. `cargo xtask build-gnome` and `NATIVE_WORKSPACE_SMOKE=1 target/debug/native-workspace-gnome` both passed. The latter emits a button click and checks that the GTK label shows `Counter: 1`. Linux CI repeats the smoke check under Xvfb.
