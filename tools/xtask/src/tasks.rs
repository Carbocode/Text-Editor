use std::process::ExitCode;

pub const HELP: &str = "usage: cargo xtask <command> [target] [--dry-run]

Commands:
  build  <core|macos|gnome|kde|devtools|xtask>  Compile only
  start  <macos|gnome|kde>                    Open an existing build
  run    <macos|gnome|kde>                    Build, then start (no tests)
  check  <core|macos|gnome|kde|devtools|xtask>  Cargo type checking
  lint   <core|macos|gnome|kde|devtools|xtask>  Clippy with warnings denied
  test   <core|macos|gnome|kde|devtools|xtask>  Rust tests; macOS adds Swift smoke tests
  fmt                                       Check workspace formatting
  verify <core|macos|gnome|kde|devtools|xtask>  fmt + check + lint + test

Other commands:
  doctor [--strict macos|gnome|kde]
  devtools [--demo] | devtools-service | devtools-ui

The action-target spelling remains available (e.g. run-macos, check-core).
--dry-run prints the task steps without building, launching or testing.
GNOME/KDE GUI smoke modes remain separate; see docs/development.md.";

const DEVTOOLS_BINARIES: &[&str] = &["native-workspace-devtools", "native-workspace-devtools-ui"];

#[derive(Debug, PartialEq)]
enum Step {
    Cargo { args: Vec<&'static str>, kde: bool },
    Shell(&'static str),
    Start { package: &'static str, kde: bool },
}

fn cargo(action: &'static str, packages: &[&'static str], kde: bool) -> Step {
    let mut args = vec![action];
    for package in packages {
        args.extend(["-p", package]);
    }
    if action == "clippy" {
        args.extend(["--", "-D", "warnings"]);
    }
    Step::Cargo { args, kde }
}

fn packages(target: &str) -> Option<&'static [&'static str]> {
    match target {
        "core" => Some(&[
            "native-workspace-domain",
            "native-workspace-filesystem",
            "native-workspace-storage",
            "native-workspace-application",
            "native-workspace-syntax",
        ]),
        "macos" => Some(&["native-workspace-ffi-swift"]),
        "gnome" => Some(&["native-workspace-gnome"]),
        "kde" => Some(&["native-workspace-kde"]),
        "devtools" => Some(&[
            "native-workspace-devtools-protocol",
            "native-workspace-devtools-producer",
            "native-workspace-devtools",
            "native-workspace-devtools-ui",
        ]),
        "xtask" => Some(&["xtask"]),
        _ => None,
    }
}

// Compound commands share the same plans as their individual prerequisites.
// Building and opening an editor never depend on a test or verification step.
fn plan(action: &str, target: &str) -> Option<Vec<Step>> {
    let packages = packages(target)?;
    let kde = target == "kde";
    match action {
        "fmt" if target == "core" => Some(vec![Step::Cargo {
            args: vec!["fmt", "--all", "--check"],
            kde: false,
        }]),
        "build" => match target {
            "macos" => Some(vec![Step::Shell("apps/macos/build.sh")]),
            "gnome" | "kde" => Some(vec![
                // Keep Qt compiler overrides out of DuckDB's build.
                cargo("build", DEVTOOLS_BINARIES, false),
                cargo("build", packages, kde),
            ]),
            "devtools" => Some(vec![cargo("build", DEVTOOLS_BINARIES, false)]),
            _ => Some(vec![cargo("build", packages, false)]),
        },
        "start" => match target {
            "macos" => Some(vec![Step::Shell("apps/macos/start.sh")]),
            "gnome" | "kde" => Some(vec![Step::Start {
                package: packages[0],
                kde,
            }]),
            _ => None,
        },
        "run" => {
            let mut steps = plan("build", target)?;
            steps.extend(plan("start", target)?);
            Some(steps)
        }
        "check" => Some(vec![cargo("check", packages, kde)]),
        "lint" => Some(vec![cargo("clippy", packages, kde)]),
        "test" => {
            let mut steps = vec![cargo("test", packages, kde)];
            if target == "macos" {
                steps.extend(plan("build", target)?);
                steps.push(Step::Shell("apps/macos/test.sh"));
            }
            Some(steps)
        }
        "verify" => {
            let mut steps = plan("fmt", "core")?;
            for action in ["check", "lint", "test"] {
                steps.extend(plan(action, target)?);
            }
            Some(steps)
        }
        _ => None,
    }
}

impl Step {
    fn description(&self) -> String {
        match self {
            Self::Cargo { args, kde } => format!(
                "cargo {}{}",
                args.join(" "),
                if *kde { " [Qt environment]" } else { "" }
            ),
            Self::Shell(script) => format!("sh {script}"),
            Self::Start { package, kde } => {
                let target = std::env::var_os("CARGO_TARGET_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| "target".into());
                format!(
                    "{}{}",
                    if *kde {
                        ""
                    } else {
                        "NATIVE_WORKSPACE_NEW_INSTANCE=1 "
                    },
                    target
                        .join("debug")
                        .join(format!("{package}{}", std::env::consts::EXE_SUFFIX))
                        .display()
                )
            }
        }
    }

    fn execute(&self) -> ExitCode {
        match self {
            Self::Cargo { args, kde: true } => crate::run_kde(args),
            Self::Cargo { args, kde: false } => crate::run("cargo", args),
            Self::Shell(script) => crate::run("sh", &[script]),
            Self::Start { package, kde } => crate::start_frontend(package, *kde),
        }
    }
}

pub fn execute(action: &str, target: &str, dry_run: bool) -> Option<ExitCode> {
    let steps = plan(action, target)?;
    for step in steps {
        println!("{}", step.description());
        if !dry_run && step.execute() != ExitCode::SUCCESS {
            return Some(ExitCode::FAILURE);
        }
    }
    Some(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_launches_compose_build_and_start_without_checks_or_tests() {
        for target in ["macos", "gnome", "kde"] {
            let build = plan("build", target).unwrap();
            let start = plan("start", target).unwrap();
            assert_eq!(start.len(), 1);
            assert!(!matches!(start[0], Step::Cargo { .. }));
            let mut expected = build;
            expected.extend(start);
            assert_eq!(plan("run", target).unwrap(), expected);
            for step in expected {
                match step {
                    Step::Cargo { args, .. } => assert_eq!(args[0], "build"),
                    Step::Shell(script) => {
                        assert!(script.ends_with("build.sh") || script.ends_with("start.sh"));
                        // Inspect script contents too: macOS used to hide smoke
                        // test compilation and execution in its build script.
                        let content = std::fs::read_to_string(
                            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("../..")
                                .join(script),
                        )
                        .unwrap();
                        assert!(!content.contains("Smoke"));
                        assert!(!content.contains("test.sh"));
                        assert!(!content.contains("cargo test"));
                    }
                    Step::Start { .. } => {}
                }
            }
        }
    }

    #[test]
    fn verification_composes_explicit_checks_and_mac_smoke_prerequisites() {
        for target in ["core", "macos", "gnome", "kde", "devtools", "xtask"] {
            let mut expected = plan("fmt", "core").unwrap();
            for action in ["check", "lint", "test"] {
                expected.extend(plan(action, target).unwrap());
            }
            assert_eq!(plan("verify", target).unwrap(), expected);
        }
        let mac_tests = plan("test", "macos").unwrap();
        assert_eq!(mac_tests[1], Step::Shell("apps/macos/build.sh"));
        assert_eq!(mac_tests[2], Step::Shell("apps/macos/test.sh"));
    }

    #[test]
    fn qt_overrides_apply_only_to_kde_cargo_steps() {
        for action in ["build", "check", "lint", "test", "verify"] {
            for step in plan(action, "kde").unwrap() {
                if let Step::Cargo { args, kde } = step {
                    assert_eq!(kde, args.contains(&"native-workspace-kde"));
                }
            }
        }
    }

    #[test]
    fn invalid_actions_and_targets_have_no_executable_plan() {
        for (action, target) in [
            ("run", "core"),
            ("start", "devtools"),
            ("build", "unknown"),
            ("unknown", "macos"),
        ] {
            assert!(plan(action, target).is_none());
        }
    }
}
