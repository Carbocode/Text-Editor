# KDE development

The KDE adapter uses CXX-Qt 0.10 and a QML AppController object. Main.qml is the Qt Quick variant; set NATIVE_WORKSPACE_KIRIGAMI=1 to use KirigamiMain.qml on a host with Kirigami. Both share WorkspacePane.qml with a top toolbar for the folder picker and Save, a document list, editor, and status bar. The workspace name appears in the window title instead of the sidebar. There is no manual path entry. Toolbar buttons use theme icons when present and bundled PNG fallbacks otherwise.

Run cargo xtask doctor --strict kde, cargo xtask build-kde, and cargo xtask run-kde with Qt 6 development tools and modules. The macOS bootstrap host uses downloaded Qt Minimal 6.10.0 for Qt Quick build and smoke tests. Linux CI is the Kirigami validation target and installs Qt Quick Dialogs for the folder picker.
