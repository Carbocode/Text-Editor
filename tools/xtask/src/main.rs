use std::{
    env,
    process::{Command, ExitCode},
};

fn available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(env::temp_dir())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn report(name: &str, ok: bool) {
    println!("  {name:<18} {}", if ok { "OK" } else { "MISSING" });
}

fn doctor(strict: Option<&str>) -> ExitCode {
    let rust = available("rustc", &["--version"]);
    let cargo = available("cargo", &["--version"]);
    let uniffi =
        std::fs::read_to_string("Cargo.lock").is_ok_and(|lock| lock.contains("name = \"uniffi\""));
    let xcode = available("xcodebuild", &["-version"]);
    let swift = available("swift", &["--version"]);
    let pkg = available("pkg-config", &["--version"]);
    let gtk = available("pkg-config", &["--exists", "gtk4"]);
    let adw = available("pkg-config", &["--exists", "libadwaita-1"]);
    let cmake = available("cmake", &["--version"]);
    let qt = available("pkg-config", &["--exists", "Qt6Core"]) || available("qmake6", &["-v"]);
    let kirigami = available("pkg-config", &["--exists", "Kirigami2"])
        || available("pkg-config", &["--exists", "KF6Kirigami"])
        || available(
            "cmake",
            &[
                "--find-package",
                "-DNAME=KF6Kirigami",
                "-DCOMPILER_ID=GNU",
                "-DLANGUAGE=CXX",
                "-DMODE=EXIST",
            ],
        );

    println!("native-workspace development environment\n\nRust");
    report("rustc", rust);
    report("cargo", cargo);
    report("UniFFI crate", uniffi);
    println!("\nmacOS");
    report("xcodebuild", xcode);
    report("swift", swift);
    println!("\nGNOME");
    report("pkg-config", pkg);
    report("gtk4", gtk);
    report("libadwaita-1", adw);
    println!("\nKDE");
    report("cmake", cmake);
    report("Qt6", qt);
    report("Kirigami", kirigami);
    let all =
        rust && cargo && uniffi && xcode && swift && pkg && gtk && adw && cmake && qt && kirigami;
    println!("\nStatus: {}", if all { "complete" } else { "partial" });
    let pass = match strict {
        None => true,
        Some("macos") => rust && cargo && uniffi && xcode && swift,
        Some("gnome") => rust && cargo && pkg && gtk && adw,
        Some("kde") => rust && cargo && cmake && qt && kirigami,
        Some(_) => false,
    };
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run(program: &str, args: &[&str]) -> ExitCode {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

fn run_kde(args: &[&str]) -> ExitCode {
    let mut command = Command::new("cargo");
    command.args(args);
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        // Qt Minimal 6.10 headers need this declaration with Xcode 27's Clang.
        command.env("CXXFLAGS", "-include arm_acle.h");
    }
    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [cmd] if cmd == "doctor" => doctor(None),
        [cmd, flag, target] if cmd == "doctor" && flag == "--strict" => doctor(Some(target)),
        [cmd] if cmd == "check-core" => run("cargo", &["check"]),
        [cmd] if cmd == "build-macos" => run("sh", &["apps/macos/build.sh"]),
        [cmd] if cmd == "run-macos" => run("sh", &["apps/macos/run.sh"]),
        [cmd] if cmd == "build-gnome" => run("cargo", &["build", "-p", "native-workspace-gnome"]),
        [cmd] if cmd == "run-gnome" => run("cargo", &["run", "-p", "native-workspace-gnome"]),
        [cmd] if cmd == "build-kde" => run_kde(&["build", "-p", "native-workspace-kde"]),
        [cmd] if cmd == "run-kde" => run_kde(&["run", "-p", "native-workspace-kde"]),
        _ => {
            eprintln!(
                "usage: cargo xtask doctor [--strict macos|gnome|kde] | check-core | build-macos | run-macos | build-gnome | run-gnome | build-kde | run-kde"
            );
            ExitCode::FAILURE
        }
    }
}
