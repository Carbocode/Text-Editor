# macOS development

Verified host: Apple Silicon macOS 27, Xcode 27.0, Swift 6.4, Rust 1.99.0. The SwiftUI app uses generated UniFFI Swift bindings and a Rust static library.

Run `cargo xtask build-macos`. This builds the Rust adapter, generates Swift bindings, compiles the app, and runs a Swift bridge smoke executable. Run `cargo xtask run-macos` to open the counter window. Generated files stay under `apps/macos/generated` and build output under `apps/macos/.build`.

The UI uses `WorkspaceStore` on the main actor. Its only business operation calls `WorkspaceApplication.increment()` through UniFFI.
