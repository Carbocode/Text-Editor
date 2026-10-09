mod bridge;

unsafe extern "C" {
    fn register_editor_syntax_highlighter();
}

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    unsafe { register_editor_syntax_highlighter() };
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    let qml = if std::env::var_os("NATIVE_WORKSPACE_KIRIGAMI").is_some() {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/KirigamiMain.qml"
    } else {
        "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/Main.qml"
    };
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(qml));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
