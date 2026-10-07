# Architecture

```text
domain (Rust values)
   ↓
application (commands, state, owned snapshots)
   ├── ffi-swift (UniFFI) → SwiftUI
   ├── gnome (direct Rust) → GTK4/libadwaita
   └── kde (direct Rust) → CXX-Qt/QML
```

`Application::dispatch(AppCommand)` owns the diagnostic state and returns a copyable `AppSnapshot`. Every frontend translates its own button click into `IncrementDiagnostic` and renders the returned counter. Revision advances once per successful command. The core has no UI, toolkit, FFI, or platform dependency. The UI layers own their platform objects and display errors; the core owns command semantics. Phase 0 deliberately contains no workspace files, storage, search, or shared widget abstraction.

See [ADR 0001](adr/0001-native-platform-frontends.md).
