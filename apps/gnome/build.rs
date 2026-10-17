use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=icons.gresource.xml");
    let dependencies = Command::new("glib-compile-resources")
        .args([
            "--sourcedir=icons",
            "--generate-dependencies",
            "icons.gresource.xml",
        ])
        .output()
        .expect("glib-compile-resources is required for the GNOME frontend");
    assert!(
        dependencies.status.success(),
        "failed to list GNOME icon dependencies"
    );
    for dependency in String::from_utf8(dependencies.stdout)
        .expect("UTF-8 icon paths")
        .lines()
    {
        println!("cargo:rerun-if-changed={dependency}");
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("icons.gresource");
    let status = Command::new("glib-compile-resources")
        .args(["--sourcedir=icons", "--target"])
        .arg(&output)
        .arg("icons.gresource.xml")
        .status()
        .expect("glib-compile-resources is required for the GNOME frontend");
    assert!(status.success(), "failed to compile GNOME icons");
}
