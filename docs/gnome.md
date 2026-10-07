# GNOME development

The GNOME frontend uses GTK4 4.8+ and libadwaita 1.9+. Run cargo xtask doctor --strict gnome, cargo xtask build-gnome, and cargo xtask run-gnome after installing the development libraries. Linux CI uses Ubuntu 26.04, which supplies libadwaita 1.9.

The app uses AdwSidebar inside AdwOverlaySplitView. Separate AdwToolbarViews let the file sidebar occupy the full window height, including its own header. The file rows have compact spacing. The native directory picker opens from the sidebar header; Save and the project title are in the content header, with document status below the editor. The diagnostic counter is displayed only during NATIVE_WORKSPACE_SMOKE=1.

On the macOS bootstrap host, Homebrew GTK4 4.24.1 and libadwaita 1.10.0 built and ran the app. Linux CI builds and checks diagnostic and document edit/save paths under Xvfb.
