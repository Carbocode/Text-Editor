# KDE development

The KDE adapter uses CXX-Qt 0.10 and a QML AppController object. Main.qml is the Qt Quick variant; set NATIVE_WORKSPACE_KIRIGAMI=1 to use KirigamiMain.qml on a host with Kirigami. Both share WorkspacePane.qml with a top toolbar, file tree, editor, and status bar. The workspace name appears in the window title instead of the sidebar. There is no manual path entry. Toolbar buttons use theme icons when present and bundled PNG fallbacks otherwise.

On a KDE desktop, the app selects the `org.kde.desktop` Qt Quick Controls style when `QT_QUICK_CONTROLS_STYLE` is unset. Install `qqc2-desktop-style` to use that style; an explicitly selected style takes precedence.

The editor uses `QFontDatabase::FixedFont`, preserving the platform's configured fixed-width font and size, with an explicit monospace hint and kerning disabled. Line numbers use the same resolved family and pixel size. The sidebar smoke test also verifies that narrow and wide ASCII characters have equal advances in the editor font.

Run cargo xtask doctor --strict kde, cargo xtask build-kde, and cargo xtask run-kde with Qt 6 development tools and modules. The macOS bootstrap host uses downloaded Qt Minimal 6.10.0 for Qt Quick build and smoke tests. Linux CI is the Kirigami validation target and installs Qt Quick Dialogs for the folder picker.

`cargo xtask build kde` compiles without tests; `cargo xtask start kde` opens the existing binary; `cargo xtask run kde` combines them. `cargo xtask test kde` runs Rust tests; the GUI smoke modes below remain explicit. The hyphenated spellings remain aliases. Qt discovery and compiler overrides also apply to the new `check`, `lint` and `test` tasks. See [terminal commands](development.md#terminal-commands).

On macOS, `build-kde` and `run-kde` select the newest installed Qt 6.10+ bootstrap `qmake` from the Qt Minimal cache, ignoring unrelated entries such as Finder's `.DS_Store`. This bypasses a cache-scanning panic in qt-build-utils 0.10. Set `QMAKE` to choose another installation explicitly; `QT_MINIMAL_DOWNLOAD_ROOT` is honored for custom cache locations. When no cached installation exists, the usual Qt discovery and download remain available.

The Files tab uses Qt Quick `TreeView` with a `TreeModel` and the style-provided `TreeViewDelegate`, so KDE draws its native branch indicators. The Search tab keeps its grouped results in a virtualized `ListView`. The sidebar groups sorted file and folder paths by parent when the workspace or search results change and caches the visible tree order. To measure the tree path on a workspace containing a populated `src` directory, run `target/debug/native-workspace-kde --benchmark-workspace=/path/to/workspace` (with `QT_QPA_PLATFORM=offscreen` for headless runs). The benchmark verifies that the native tree follows expansion, collapse, and name filtering. Its timings do not include display presentation.

Files and Search keep separate caches and models across tab changes. Workspace paths and the name filter invalidate Files; text queries and search results invalidate Search. Tree rows draw bundled Material icons explicitly because some `TreeViewDelegate` styles render only text. Row bindings follow changes to the tree's visible row mapping after expansion or collapse.

The sidebar regression smoke test runs with `--smoke-sidebar=/path/to/fixture`. The fixture must contain `README.md` and `src/main.rs`, with `sidebar needle` in the Rust file. The test checks repeated tab changes without tree resets, icon loading and size, retained expansion/collapse, search and filter invalidation, and new-file updates. It creates `src/new.rs`; use a disposable fixture. CI runs it with Basic and Fusion styles. Its tab timings measure cached switching logic, excluding display presentation.
