use cxx_qt_build::{CxxQtBuilder, QmlModule};
use qt_build_utils::{QResource, QResourceFile, QResources};

fn main() {
    let icon_directory = "../../assets/material-icon-theme/png";
    let mut icon_files: Vec<_> = std::fs::read_dir(icon_directory)
        .expect("bundled Material Icon Theme PNGs")
        .map(|entry| entry.expect("icon file").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .collect();
    icon_files.sort();
    let resources = icon_files
        .into_iter()
        .fold(QResource::new(), |resource, path| {
            let name = path.file_name().expect("icon file name").to_string_lossy();
            resource.file(QResourceFile::new(&path).alias(format!("material/{name}")))
        });
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.nativeworkspace.Diagnostic")
            .qml_file("qml/Main.qml")
            .qml_file("qml/KirigamiMain.qml")
            .qml_file("qml/WorkspacePane.qml")
            .qml_file("tests/SidebarSmoke.qml"),
    )
    .qrc_resources([
        "qml/icons/action-open.png",
        "qml/icons/action-save.png",
        "qml/icons/git-branch.png",
        "qml/icons/bookmark.png",
    ])
    .qrc_resources(QResources::new().resource(resources))
    .qt_module("Network")
    .qt_module("Quick")
    .cpp_files([
        "src/syntax_highlighter.h",
        "src/syntax_highlighter.cpp",
        "src/editor_fonts.h",
        "src/editor_fonts.cpp",
    ])
    .files(["src/bridge.rs"])
    .build();
}
