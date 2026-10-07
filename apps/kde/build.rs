use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.nativeworkspace.Diagnostic")
            .qml_file("qml/Main.qml")
            .qml_file("qml/KirigamiMain.qml"),
    )
    .qt_module("Network")
    .files(["src/bridge.rs"])
    .build();
}
