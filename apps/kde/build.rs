use cxx_qt_build::{CxxQtBuilder, QmlModule};
use qt_build_utils::{QResource, QResourceFile, QResources};
use std::path::PathBuf;

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
    // ColorScheme is already used by Breeze; link its public manager API too,
    // so KDE colors follow the host's light/dark preference outside Plasma.
    println!("cargo:rerun-if-env-changed=KF6_COLORSCHEME_PREFIX");
    let qt = qt_build_utils::QtBuild::new(vec![]).expect("Could not locate Qt");
    let paths = qt.qtpaths();
    let kde_prefix = std::env::var_os("KF6_COLORSCHEME_PREFIX").map(PathBuf::from);
    let prefix = kde_prefix.clone().unwrap_or_else(|| {
        PathBuf::from(paths.query("QT_INSTALL_PREFIX").expect("Qt prefix missing"))
    });
    let include = prefix.join("include/KF6/KColorScheme");
    assert!(
        include.join("KColorSchemeManager").exists(),
        "Install KF6 ColorScheme development headers beside Qt, or set KF6_COLORSCHEME_PREFIX"
    );
    let libraries = kde_prefix.map_or_else(
        || {
            PathBuf::from(
                paths
                    .query("QT_INSTALL_LIBS")
                    .expect("Qt library path missing"),
            )
        },
        |prefix| prefix.join("lib"),
    );
    println!("cargo:rustc-link-search=native={}", libraries.display());
    println!("cargo:rustc-link-lib=KF6ColorScheme");

    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("org.nativeworkspace.Diagnostic")
            .qml_file("qml/Main.qml")
            .qml_file("qml/WorkspacePane.qml")
            .qml_file("qml/WorkspaceHome.qml")
            .qml_file("qml/EntryDialogs.qml")
            .qml_file("tests/WorkspaceSmoke.qml")
            .qml_file("tests/SidebarSmoke.qml")
            .qml_file("tests/SidebarLayoutSmoke.qml")
            .qml_file("tests/CursorSmoke.qml")
            .qml_file("tests/AppearanceSmoke.qml")
            .qml_file("tests/DragSmoke.qml"),
    )
    .qrc_resources(QResources::new().resource(resources))
    .qt_module("Network")
    .qt_module("Quick")
    .cpp_files([
        "tests/cursor_probe.h",
        "src/syntax_highlighter.h",
        "src/syntax_highlighter.cpp",
        "src/search_results_model.h",
        "src/search_results_model.cpp",
        "src/editor_fonts.h",
        "src/editor_fonts.cpp",
        "src/desktop_integration.h",
        "src/desktop_integration.cpp",
    ])
    .files(["src/bridge.rs"]);
    // SAFETY: only add the installed public KDE header directory.
    unsafe {
        builder
            .cc_builder(|cc| {
                cc.include(&include);
            })
            .build();
    }
}
