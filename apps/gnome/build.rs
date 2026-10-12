use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=icons.gresource.xml");
    println!("cargo:rerun-if-changed=icons/native-workspace-branch-symbolic.svg");
    println!("cargo:rerun-if-changed=icons/native-workspace-bookmark-symbolic.svg");

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
