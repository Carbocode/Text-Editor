# macOS development

Verified locally on Apple Silicon macOS 27, Xcode 27.0, Swift 6.4, and Rust 1.99.0. The SwiftUI frontend uses UniFFI generated bindings and the Rust static library.

Run cargo xtask build-macos to build apps/macos/.build/NativeWorkspace.app and run the Swift bridge smoke test. Run cargo xtask run-macos to start the app. Generated bindings are in apps/macos/generated; build output is in apps/macos/.build.

The app uses a native NavigationSplitView, a directory picker, an editor, Save, and a bottom document status bar. The initial Open Workspace action uses Liquid Glass styling. WorkspaceStore holds view state on the main actor and sends operations through UniFFI.
