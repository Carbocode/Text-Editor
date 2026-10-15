mod bridge;

unsafe extern "C" {
    fn register_editor_syntax_highlighter();
    fn register_editor_fonts();
    fn configure_kde_desktop();
}

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QUrl};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn main() -> std::process::ExitCode {
    unsafe {
        register_editor_syntax_highlighter();
        register_editor_fonts();
    }
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        QQuickStyle::set_style(&"org.kde.breeze".into());
    }
    if std::env::var_os("QT_QUICK_CONTROLS_FALLBACK_STYLE").is_none() {
        // Breeze does not provide TreeViewDelegate. Use Qt's desktop delegate
        // for missing controls, while retaining Breeze for the rest of the UI.
        QQuickStyle::set_fallback_style(&"Fusion".into());
    }
    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut()
            .set_application_name(&"Native Workspace".into());
        app.set_organization_domain(&"nativeworkspace.org".into());
        QGuiApplication::set_desktop_file_name(&"org.nativeworkspace.App".into());
    }
    unsafe { configure_kde_desktop() };
    let mut engine = QQmlApplicationEngine::new();
    let qml = if std::env::args().any(|argument| argument.starts_with("--smoke-cursors=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/CursorSmoke.qml"
    } else if std::env::args().any(|argument| argument.starts_with("--smoke-sidebar-layout=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/SidebarLayoutSmoke.qml"
    } else if std::env::args().any(|argument| argument.starts_with("--smoke-appearance=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/AppearanceSmoke.qml"
    } else if std::env::args().any(|argument| argument.starts_with("--smoke-drag=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/DragSmoke.qml"
    } else if std::env::args().any(|argument| argument.starts_with("--smoke-sidebar=")) {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/tests/SidebarSmoke.qml"
    } else {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/Main.qml"
    };
    let load_failed = Arc::new(AtomicBool::new(false));
    if let Some(mut engine) = engine.as_mut() {
        let failed = load_failed.clone();
        engine
            .as_mut()
            .on_object_creation_failed(move |_, _| failed.store(true, Ordering::Relaxed))
            .release();
        engine.load(&QUrl::from(qml));
    }
    if load_failed.load(Ordering::Relaxed) {
        eprintln!(
            "Could not load the KDE interface. Install Kirigami and qqc2-breeze-style for this Qt installation; \
             use QML_IMPORT_PATH if its QML modules are in a separate prefix."
        );
        return std::process::ExitCode::FAILURE;
    }
    if let Some(app) = app.as_mut() {
        return std::process::ExitCode::from(u8::try_from(app.exec()).unwrap_or(1));
    }
    std::process::ExitCode::FAILURE
}
