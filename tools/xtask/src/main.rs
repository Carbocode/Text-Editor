use std::{
    env,
    process::{Command, ExitCode},
};

#[cfg(any(target_os = "macos", test))]
mod qt;
mod tasks;

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
        Ok(status) => {
            eprintln!("{program} failed: {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("Could not run {program}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_kde(args: &[&str]) -> ExitCode {
    let mut command = Command::new("cargo");
    command.args(args);
    #[cfg(target_os = "macos")]
    qt::configure(&mut command);
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        // Qt Minimal 6.10 headers need this declaration with Xcode 27's Clang.
        command.env("CXXFLAGS", "-include arm_acle.h");
    }
    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

fn start_frontend(package: &str, kde: bool) -> ExitCode {
    let executable = env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target".into())
        .join("debug")
        .join(format!("{package}{}", env::consts::EXE_SUFFIX));
    if !executable.is_file() {
        eprintln!(
            "App not built. Run cargo xtask build {} first.",
            if kde { "kde" } else { "gnome" }
        );
        return ExitCode::FAILURE;
    }
    let mut command = Command::new(executable);
    if !kde {
        // A registered GApplication otherwise activates the previously built process.
        command.env("NATIVE_WORKSPACE_NEW_INSTANCE", "1");
    }
    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

fn devtools(demo: bool) -> ExitCode {
    use native_workspace_devtools_protocol::{Descriptor, Health, VERSION, diagnostics_dir};
    use std::time::{Duration, Instant};
    if tasks::execute("build", "devtools", false) != Some(ExitCode::SUCCESS) {
        return ExitCode::FAILURE;
    }
    let Ok(client) = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(500))
        .build()
    else {
        return ExitCode::FAILURE;
    };
    let ready = || {
        Descriptor::read(&diagnostics_dir().join("service.json"))
            .ok()
            .is_some_and(|d| {
                client
                    .get(format!("{}/v1/health", d.base_url()))
                    .bearer_auth(&d.token)
                    .send()
                    .and_then(|r| r.error_for_status())
                    .and_then(|r| r.json::<Health>())
                    .is_ok_and(|h| h.version == VERSION && h.service == d.service)
            })
    };
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target".into())
        .join("debug");
    if !ready() {
        let mut command = Command::new(target.join(format!(
            "native-workspace-devtools{}",
            env::consts::EXE_SUFFIX
        )));
        if demo {
            command.arg("--demo");
        }
        if command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .is_err()
        {
            return ExitCode::FAILURE;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            if Instant::now() >= deadline {
                eprintln!(
                    "Diagnostic service did not become ready; editor launches remain independent."
                );
                return ExitCode::FAILURE;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    run(
        target
            .join(format!(
                "native-workspace-devtools-ui{}",
                env::consts::EXE_SUFFIX
            ))
            .to_str()
            .unwrap(),
        &[],
    )
}

fn main() -> ExitCode {
    // Scripts and Cargo invocations use paths relative to the workspace root.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Err(error) = env::set_current_dir(root) {
        eprintln!("Could not enter workspace: {error}");
        return ExitCode::FAILURE;
    }
    let mut args: Vec<String> = env::args().skip(1).collect();
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    args.retain(|arg| arg != "--dry-run");
    if args.is_empty()
        || matches!(args.as_slice(), [cmd] if matches!(cmd.as_str(), "help" | "--help" | "-h"))
    {
        println!("{}", tasks::HELP);
        return ExitCode::SUCCESS;
    }
    let task = match args.as_slice() {
        [action, target] => Some((action.as_str(), target.as_str())),
        [cmd] if cmd == "fmt" => Some(("fmt", "core")),
        [cmd] => cmd.split_once('-'),
        _ => None,
    };
    if let Some((action, target)) = task
        && let Some(result) = tasks::execute(action, target, dry_run)
    {
        return result;
    }
    if dry_run {
        eprintln!("--dry-run requires a build, start, run, check, lint, test, fmt or verify task.");
        return ExitCode::FAILURE;
    }
    match args.as_slice() {
        [cmd] if cmd == "doctor" => doctor(None),
        [cmd, flag, target] if cmd == "doctor" && flag == "--strict" => doctor(Some(target)),
        [cmd] if cmd == "devtools" => devtools(false),
        [cmd, flag] if cmd == "devtools" && flag == "--demo" => devtools(true),
        [cmd] if cmd == "devtools-service" => {
            run("cargo", &["run", "-p", "native-workspace-devtools"])
        }
        [cmd] if cmd == "devtools-ui" => {
            run("cargo", &["run", "-p", "native-workspace-devtools-ui"])
        }
        _ => {
            eprintln!(
                "Unknown command or target: {}\n\n{}",
                args.join(" "),
                tasks::HELP
            );
            ExitCode::FAILURE
        }
    }
}
