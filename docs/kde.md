# KDE development

The KDE adapter uses CXX-Qt 0.10 and a QML `AppController` QObject. `Main.qml` is a Qt Quick smoke shell. Set `NATIVE_WORKSPACE_KIRIGAMI=1` to load `KirigamiMain.qml` on a host with Kirigami. Linux CI builds and runs this path under Xvfb.

Run `cargo xtask doctor --strict kde`, then `cargo xtask build-kde` and `cargo xtask run-kde` when Qt 6 development tools and modules are available. CXX-Qt needs Qt Core, Gui, Qml, Quick, and Network plus a C++ compiler. On the macOS 27 bootstrap host, no system Qt or Kirigami was installed. The build fetched Qt Minimal 6.10.0; `cargo xtask build-kde` passed, and `QT_QPA_PLATFORM=offscreen target/debug/native-workspace-kde --smoke` logged a successful QML → CXX-Qt → Rust increment. The xtask command supplies `-include arm_acle.h` for the bundled Qt headers with Xcode 27. Linux CI is the canonical Kirigami validation target.
