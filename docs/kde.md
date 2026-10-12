# KDE development

The KDE adapter uses CXX-Qt 0.10 and a QML AppController object. Main.qml is the Qt Quick variant; set NATIVE_WORKSPACE_KIRIGAMI=1 to use KirigamiMain.qml on a host with Kirigami. Both share WorkspacePane.qml with a top toolbar, file tree, editor, and status bar. The workspace name appears in the window title instead of the sidebar. There is no manual path entry. Toolbar buttons use theme icons when present and bundled PNG fallbacks otherwise.

On a KDE desktop, the app selects the `org.kde.desktop` Qt Quick Controls style when `QT_QUICK_CONTROLS_STYLE` is unset. Install `qqc2-desktop-style` to use that style; an explicitly selected style takes precedence.

Run cargo xtask doctor --strict kde, cargo xtask build-kde, and cargo xtask run-kde with Qt 6 development tools and modules. The macOS bootstrap host uses downloaded Qt Minimal 6.10.0 for Qt Quick build and smoke tests. Linux CI is the Kirigami validation target and installs Qt Quick Dialogs for the folder picker.

The Files tab uses Qt Quick `TreeView` with a `TreeModel` and the style-provided `TreeViewDelegate`, so KDE draws its native branch indicators. The Search tab keeps its grouped results in a virtualized `ListView`. The sidebar groups sorted file and folder paths by parent when the workspace or search results change and caches the visible tree order. To measure the tree path on a workspace containing a populated `src` directory, run `target/debug/native-workspace-kde --benchmark-workspace=/path/to/workspace` (with `QT_QPA_PLATFORM=offscreen` for headless runs). The benchmark verifies that the native tree follows expansion, collapse, and name filtering. Its timings do not include display presentation.
