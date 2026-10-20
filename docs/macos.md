# macOS development

Verified locally on Apple Silicon macOS 27, Xcode 27.0, Swift 6.4, and Rust 1.99.0. The SwiftUI frontend uses UniFFI generated bindings and the Rust static library.

Run `cargo xtask build macos` to build `apps/macos/.build/NativeWorkspace.app` without running tests. `cargo xtask start macos` opens an existing build; `cargo xtask run macos` builds and opens it. Use `cargo xtask test macos` for Rust bridge tests and all eight Swift smoke tests, or `cargo xtask verify macos` for formatting, Rust check/Clippy and those tests. The previous `build-macos` and `run-macos` spellings remain available. Generated bindings are in `apps/macos/generated`; build output is in `apps/macos/.build`. See [terminal commands](development.md#terminal-commands) for command composition and dry runs.

The app uses a native NavigationSplitView, a directory picker, an editor, Save, and a bottom document status bar. The workspace navigation title supplies the native window title, including when a document is open. The editor uses an AppKit NSTextView inside NSScrollView so long lines stay on one line and scroll horizontally. The initial Open Workspace action uses Liquid Glass styling. WorkspaceStore holds view state on the main actor and sends operations through UniFFI.

Document tabs stay inside the editor window. The dedicated closable tab component is adapted from [Bonsplit](../apps/macos/ThirdParty/Bonsplit/README.md): each tab has the same Material file icon as the sidebar, a dirty dot in a fixed overlay on the left, and a close button. Inactive tabs have rounded floating Liquid Glass backgrounds; the selected tab uses a solid system accent fill. Tabs use the standard SwiftUI glass/glassProminent button styles, with the file image and title inside each button label. Close and dirty indicators overlay the control independently. The close control keeps its layout space when hidden. The dirty dot occupies the existing left padding and never changes tab width or disappears with a truncated title. File rows show an eye for documents open in the editor, independently of the dirty dot; closing a tab clears only the eye. Sidebar and document tab rows share a 50-point height and center their controls vertically. The editor toolbar has a separate pair of Decrease/Increase Text Size controls, with one-point steps from 8 to 40 points. A percentage between the controls uses the initial editor font as 100%. Font changes reuse the text view and preserve text, selection and undo. The workspace title occupies the toolbar’s centered principal slot. Save and Save All share a native toolbar group on the right, separated from the overflow menu by a fixed toolbar spacer, and retain Command-S and Command-Shift-S. The workspace and Home windows share an overflow menu with Home, Developer Tools (when available), Open Folder (Command-O) and Quit. The shared NSTextView stays outside the tab labels so renaming a tab preserves editor identity, selection and undo.

Inline renaming uses an AppKit NSTextField and its native field editor. Return submits, Escape cancels, and ending editing cancels the draft through the delegate. There is no application-wide mouse-event monitor for rename cancellation; clicks reach their intended views.

Single-clicking a file or search result opens a preview tab with an italic title. There is at most one preview per window; opening another file in preview replaces it, while selecting an existing tab preserves it. Editing, explicitly saving, double-clicking the file or tab, or choosing Keep Open makes the tab permanent. Undoing edits or saving afterwards does not turn it back into a preview. New files open permanently. F2 and the context menu handle renaming. These rules follow [VS Code preview mode](https://code.visualstudio.com/docs/editing/getting-started/userinterface#_preview-mode).

Application-icon drops and Open With go through `WorkspaceAppDelegate`, including
requests delivered before SwiftUI creates a window. The native delegate retains
only pending window presentation; shared `OpenPaths` owns validation, scheduling
and errors. See [system-open verification](development.md#application-icon-and-open-with-verification).

The Git sidebar uses a native vertical split initially at half height, with Source
Control above and the shared branch/commit graph below. The sidebar header stays
outside the split; opening Git loads the repository after any pending workspace
operation finishes. Branch names truncate to retain the refresh and actions
buttons at the minimum sidebar width. The graph uses one row per commit with
reference badges and an inline commit summary listing changed files; changes show
file name, folder and a colored status letter, grouped into merge, staged and
unstaged sections. Diffs render numbered, colored lines; clicking a hunk header
selects its changed lines. Staging, commit, branch,
remote, stash and history actions run through Session. Diff reviews support
before/after content and selected-line staging; the merge editor reviews current,
incoming and editable result content. See
[Git Source Control verification](development.md#git-source-control-verification).
