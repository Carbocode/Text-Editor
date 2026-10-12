mod bridge;

unsafe extern "C" {
    fn register_editor_syntax_highlighter();
    fn register_editor_fonts();
}

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QUrl};

fn main() -> std::process::ExitCode {
    unsafe {
        register_editor_syntax_highlighter();
        register_editor_fonts();
    }
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none()
        && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
            desktop
                .split(':')
                .any(|name| name.eq_ignore_ascii_case("KDE"))
        })
    {
        QQuickStyle::set_style(&"org.kde.desktop".into());
    }
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    let qml = if std::env::args().any(|argument| argument.starts_with("--smoke-sidebar=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/SidebarSmoke.qml"
    } else if std::env::var_os("NATIVE_WORKSPACE_KIRIGAMI").is_some() {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/KirigamiMain.qml"
    } else {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/Main.qml"
    };
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(qml));
    }
    if let Some(app) = app.as_mut() {
        return std::process::ExitCode::from(u8::try_from(app.exec()).unwrap_or(1));
    }
    std::process::ExitCode::FAILURE
}
