# macOS development

Verified locally on Apple Silicon macOS 27, Xcode 27.0, Swift 6.4, and Rust 1.99.0. The SwiftUI frontend uses UniFFI generated bindings and the Rust static library.

Run `cargo xtask build macos` to build `apps/macos/.build/NativeWorkspace.app` without running tests. `cargo xtask start macos` opens an existing build; `cargo xtask run macos` builds and opens it. Use `cargo xtask test macos` for Rust bridge tests and all seven Swift smoke tests, or `cargo xtask verify macos` for formatting, Rust check/Clippy and those tests. The previous `build-macos` and `run-macos` spellings remain available. Generated bindings are in `apps/macos/generated`; build output is in `apps/macos/.build`. See [terminal commands](development.md#terminal-commands) for command composition and dry runs.

The app uses a native NavigationSplitView, a directory picker, an editor, Save, and a bottom document status bar. The workspace navigation title supplies the native window title, including when a document is open. The editor uses an AppKit NSTextView inside NSScrollView so long lines stay on one line and scroll horizontally. The initial Open Workspace action uses Liquid Glass styling. WorkspaceStore holds view state on the main actor and sends operations through UniFFI.

Inline renaming uses an AppKit NSTextField and its native field editor. Return submits, Escape cancels, and ending editing cancels the draft through the delegate. There is no application-wide mouse-event monitor for rename cancellation; clicks reach their intended views.
