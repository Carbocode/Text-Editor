use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.nativeworkspace.Diagnostic")
            .qml_file("qml/Main.qml")
            .qml_file("qml/KirigamiMain.qml")
            .qml_file("qml/WorkspacePane.qml"),
    )
    .qrc_resources([
        "qml/icons/readme.png",
        "qml/icons/markdown.png",
        "qml/icons/document.png",
        "qml/icons/action-open.png",
        "qml/icons/action-save.png",
    ])
    .qt_module("Network")
    .files(["src/bridge.rs"])
    .build();
}
