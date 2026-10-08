# GNOME development

The GNOME frontend uses GTK4 4.8+ and libadwaita 1.9+. Run cargo xtask doctor --strict gnome, cargo xtask build-gnome, and cargo xtask run-gnome after installing the development libraries. Linux CI uses Ubuntu 26.04, which supplies libadwaita 1.9.

The app uses a compact, virtualized GTK ListView inside AdwOverlaySplitView. Each file row indents its icon and text together according to directory depth, and directories sort before files. Separate AdwToolbarViews let the file sidebar occupy the full window height, including its own header. The sidebar header has a symbolic Open button with a tooltip; the content header has a standard sidebar toggle icon, an icon and label for Save, and the workspace heading. The native window title follows the open workspace name. Both header bars leave title button placement to libadwaita, so window controls follow the desktop's left or right decoration layout and move to the content header when the sidebar is hidden. The text view does not wrap long lines and enables horizontal scrolling. Document status appears below the editor. The diagnostic counter is displayed only during NATIVE_WORKSPACE_SMOKE=1.

On the macOS bootstrap host, Homebrew GTK4 4.24.1 and libadwaita 1.10.0 built and ran the app. Linux CI builds and checks diagnostic and document edit/save paths under Xvfb.
