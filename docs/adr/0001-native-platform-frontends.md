# ADR 0001: Native platform frontends

Status: accepted for the Phase 0 experiment.

Use one Rust domain and application layer. SwiftUI is the macOS interface and crosses UniFFI in a dedicated adapter. GNOME calls the application crate directly from Rust GTK4/libadwaita code. KDE calls it through a Rust CXX-Qt QObject adapter exposed to QML. Platform UI conventions remain local to each frontend. There is no common cross-platform widget layer.

This keeps application commands, state changes, and snapshots identical while allowing different navigation and presentation on each platform. The cost is maintaining three UI implementations and their distinct build dependencies.
