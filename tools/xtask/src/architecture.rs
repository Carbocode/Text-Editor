//! Executable dependency boundaries. New workspace packages must choose a layer.
use serde_json::Value;
use std::{
    collections::HashSet,
    process::{Command, ExitCode},
};

fn permitted(from: &str, to: &str) -> bool {
    let domain = "native-workspace-domain";
    let protocol = "native-workspace-devtools-protocol";
    let application = "native-workspace-application";
    let syntax = "native-workspace-syntax";
    match from {
        "native-workspace-domain"
        | "native-workspace-syntax"
        | "native-workspace-devtools-protocol" => false,
        "native-workspace-filesystem" | "native-workspace-storage" => to == domain,
        "native-workspace-application" => [
            domain,
            "native-workspace-filesystem",
            "native-workspace-storage",
            "native-workspace-devtools-producer",
        ]
        .contains(&to),
        "native-workspace-ffi-swift" | "native-workspace-gnome" | "native-workspace-kde" => {
            [domain, application, syntax].contains(&to)
        }
        "native-workspace-devtools-producer"
        | "native-workspace-devtools"
        | "native-workspace-devtools-ui"
        | "xtask" => to == protocol,
        _ => false,
    }
}

fn violations(metadata: &Value) -> Result<Vec<String>, &'static str> {
    let packages = metadata["packages"].as_array().ok_or("missing packages")?;
    let members: HashSet<_> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut errors = Vec::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("missing package name")?;
        let known = matches!(
            name,
            "native-workspace-domain"
                | "native-workspace-syntax"
                | "native-workspace-devtools-protocol"
                | "native-workspace-filesystem"
                | "native-workspace-storage"
                | "native-workspace-application"
                | "native-workspace-ffi-swift"
                | "native-workspace-gnome"
                | "native-workspace-kde"
                | "native-workspace-devtools-producer"
                | "native-workspace-devtools"
                | "native-workspace-devtools-ui"
                | "xtask"
        );
        if !known {
            errors.push(format!(
                "{name}: assign the new package an architecture layer"
            ));
        }
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
        {
            if dependency["kind"] == "dev" {
                continue;
            }
            let to = dependency["name"]
                .as_str()
                .ok_or("missing dependency name")?;
            if members.contains(to) && !permitted(name, to) {
                errors.push(format!(
                    "{name} → {to}: dependency crosses a layer boundary"
                ));
            }
            if name == "native-workspace-domain" {
                errors.push(format!(
                    "{name}: domain values must depend only on std (found {to})"
                ));
            }
            if matches!(
                name,
                "native-workspace-application"
                    | "native-workspace-domain"
                    | "native-workspace-filesystem"
                    | "native-workspace-storage"
                    | "native-workspace-syntax"
            ) && [
                "gtk4",
                "libadwaita",
                "cxx-qt",
                "cxx-qt-lib",
                "uniffi",
                "eframe",
                "egui",
                "qt-build-utils",
            ]
            .contains(&to)
            {
                errors.push(format!(
                    "{name}: native toolkit dependency {to} belongs in an adapter"
                ));
            }
        }
    }
    Ok(errors)
}

pub fn check() -> ExitCode {
    let result = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .output();
    let output = match result {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("Could not inspect dependency graph: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|e| e.to_string())
        .and_then(|metadata| violations(&metadata).map_err(str::to_owned));
    match result {
        Ok(errors) if errors.is_empty() => {
            println!("Architecture dependency boundaries passed");
            ExitCode::SUCCESS
        }
        Ok(errors) => {
            for error in errors {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("Invalid Cargo metadata: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn editor_adapters_cannot_bypass_application_ports() {
        for adapter in [
            "native-workspace-ffi-swift",
            "native-workspace-gnome",
            "native-workspace-kde",
        ] {
            assert!(permitted(adapter, "native-workspace-application"));
            assert!(!permitted(adapter, "native-workspace-filesystem"));
            assert!(!permitted(adapter, "native-workspace-storage"));
        }
        assert!(!permitted(
            "native-workspace-application",
            "native-workspace-gnome"
        ));
        assert!(!permitted(
            "native-workspace-devtools",
            "native-workspace-application"
        ));
    }

    #[test]
    fn metadata_check_detects_toolkit_leaks_and_unknown_packages() {
        let metadata = json!({"packages": [
            {"name":"native-workspace-application", "dependencies":[{"name":"gtk4", "kind":null}]},
            {"name":"new-editor-adapter", "dependencies":[]}
        ]});
        let errors = violations(&metadata).unwrap();
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().any(|e| e.contains("toolkit")));
        assert!(errors.iter().any(|e| e.contains("assign")));
    }
}
