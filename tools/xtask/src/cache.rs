use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn target_directory() -> Result<PathBuf, String> {
    // Cargo resolves environment and configuration overrides for us, including
    // target directories outside this checkout.
    let output = match std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Err(error) => {
            return Err(format!("Could not inspect Cargo build directory: {error}"));
        }
    };
    let metadata: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Err(format!("Could not read Cargo metadata: {error}"));
        }
    };
    let Some(target) = metadata["target_directory"].as_str() else {
        return Err("Cargo metadata did not include target_directory.".into());
    };
    Ok(PathBuf::from(target))
}

pub fn status() -> ExitCode {
    let target = match target_directory() {
        Ok(target) => target,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Build disk usage (totals include the breakdown below):");
    for path in [
        target.to_path_buf(),
        target.join("debug/deps"),
        target.join("debug/build"),
        target.join("debug/incremental"),
        target.join("release"),
        "apps/macos/.build".into(),
    ] {
        if path.exists() {
            let mut command = std::process::Command::new("du");
            if !command
                .args(["-sh"])
                .arg(&path)
                .status()
                .is_ok_and(|s| s.success())
            {
                eprintln!("Could not measure {}", path.display());
                return ExitCode::FAILURE;
            }
        } else {
            println!("not built\t{}", path.display());
        }
    }
    println!(
        "\nReclaim workspace artifacts: cargo xtask clean (preview: --dry-run).\n\
         Reset all Rust build caches: cargo clean.\n\
         Run cleanup when no builds or native packaging scripts are running."
    );
    ExitCode::SUCCESS
}

const LIMIT_ENV: &str = "NATIVE_WORKSPACE_BUILD_CACHE_LIMIT_MB";

pub struct BuildCache {
    target: PathBuf,
    limit_kib: u64,
    exceeded_before: bool,
    _lock: fs::File,
}

fn limit_kib(value: Option<&str>) -> Result<u64, String> {
    value
        .unwrap_or("8192")
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .and_then(|value| value.checked_mul(1024))
        .ok_or_else(|| format!("{LIMIT_ENV} must be a positive integer in MiB"))
}

fn size_kib(target: &Path) -> Result<u64, String> {
    if !target.exists() {
        return Ok(0);
    }
    let output = Command::new("du")
        .arg("-sk")
        .arg(target)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "Could not read build cache disk usage".into())
}

impl BuildCache {
    pub fn begin() -> Result<Self, String> {
        let limit_kib = limit_kib(std::env::var(LIMIT_ENV).ok().as_deref())?;
        let target = target_directory()?;
        fs::create_dir_all(target.parent().ok_or("Build directory has no parent")?)
            .map_err(|error| error.to_string())?;
        let name = target
            .file_name()
            .ok_or("Build directory has no name")?
            .to_string_lossy();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(target.with_file_name(format!(".{name}.native-workspace-build.lock")))
            .map_err(|error| error.to_string())?;
        lock.lock().map_err(|error| error.to_string())?;
        let size = size_kib(&target)?;
        println!(
            "Build cache before: {} MiB; cleanup threshold: {} MiB",
            size / 1024,
            limit_kib / 1024
        );
        Ok(Self {
            target,
            limit_kib,
            exceeded_before: size > limit_kib,
            _lock: lock,
        })
    }

    pub fn finish(self) -> Result<(), String> {
        let size = size_kib(&self.target)?;
        println!("Build cache after: {} MiB", size / 1024);
        if self.exceeded_before || size > self.limit_kib {
            println!(
                "Build cache threshold exceeded; resetting cache and preserving build outputs."
            );
            reset(&self.target, || {
                let status = Command::new("cargo")
                    .arg("clean")
                    .arg("--target-dir")
                    .arg(&self.target)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other("cargo clean failed"))
                }
            })
            .map_err(|error| format!("Build cache cleanup failed: {error}"))?;
            println!(
                "Build cache after cleanup: {} MiB",
                size_kib(&self.target)? / 1024
            );
        }
        Ok(())
    }
}

// Cargo's public outputs sit at a profile's root; deps/build/incremental and
// fingerprints are disposable. Keep outputs for all profiles and target triples,
// including the Swift static library needed by the subsequent native smoke step.
fn outputs(directory: &Path, depth: usize, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let profile = directory.join(".fingerprint").is_dir() || directory.join("deps").is_dir();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if profile && kind.is_file() {
            paths.push(entry.path());
        } else if !profile && depth > 0 && kind.is_dir() {
            outputs(&entry.path(), depth - 1, paths)?;
        }
    }
    Ok(())
}

fn reset(target: &Path, clean: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    // Back up before asking Cargo to clean (and acquire its own build lock).
    // Leave the originals intact if backup fails; retain the backup if restore fails.
    let backup = tempfile::Builder::new()
        .prefix("native-workspace-build-outputs-")
        .tempdir_in(
            target
                .parent()
                .ok_or_else(|| io::Error::other("Build directory has no parent"))?,
        )?;
    let mut paths = Vec::new();
    outputs(target, 2, &mut paths)?;
    // Keep Cargo's safety marker so another clean before the next build remains
    // valid. Empty fingerprint directories identify retained custom profiles.
    if target.join("CACHEDIR.TAG").is_file() {
        paths.push(target.join("CACHEDIR.TAG"));
    }
    for path in &paths {
        let destination = backup.path().join(path.strip_prefix(target).unwrap());
        fs::create_dir_all(destination.parent().unwrap())?;
        // Avoid allocating another copy of large binaries on a nearly full disk.
        fs::hard_link(path, &destination).or_else(|_| fs::copy(path, &destination).map(|_| ()))?;
    }
    let result = clean();
    for path in &paths {
        let source = backup.path().join(path.strip_prefix(target).unwrap());
        let restore = || -> io::Result<()> {
            fs::create_dir_all(path.parent().unwrap())?;
            if path.parent() != Some(target) {
                fs::create_dir_all(path.parent().unwrap().join(".fingerprint"))?;
            }
            fs::rename(&source, path).or_else(|_| fs::copy(&source, path).map(|_| ()))?;
            Ok(())
        };
        if let Err(error) = restore() {
            let saved = backup.keep();
            return Err(io::Error::other(format!(
                "{error}; build outputs saved in {}",
                saved.display()
            )));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_rejects_zero_invalid_and_overflow_values() {
        assert_eq!(limit_kib(None).unwrap(), 8 * 1024 * 1024);
        assert_eq!(limit_kib(Some("1")).unwrap(), 1024);
        for value in ["0", "-1", "oops", "18446744073709551615"] {
            assert!(limit_kib(Some(value)).is_err());
        }
    }

    #[test]
    fn cleanup_preserves_outputs_for_profiles_and_cross_targets() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target with spaces");
        for profile in ["debug", "release", "aarch64-example/custom"] {
            let root = target.join(profile);
            fs::create_dir_all(root.join("deps")).unwrap();
            fs::write(root.join("deps/obsolete"), "cache").unwrap();
            fs::write(root.join("app"), "executable").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(root.join("app"), fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(root.join("libbridge.a"), "library").unwrap();
        }
        fs::write(target.join("CACHEDIR.TAG"), "cargo safety marker").unwrap();
        reset(&target, || fs::remove_dir_all(&target)).unwrap();
        // A second cleanup without another build must still preserve outputs.
        reset(&target, || fs::remove_dir_all(&target)).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("CACHEDIR.TAG")).unwrap(),
            "cargo safety marker"
        );
        for profile in ["debug", "release", "aarch64-example/custom"] {
            let root = target.join(profile);
            assert_eq!(fs::read_to_string(root.join("app")).unwrap(), "executable");
            assert_eq!(
                fs::read_to_string(root.join("libbridge.a")).unwrap(),
                "library"
            );
            assert!(!root.join("deps").exists());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(root.join("app")).unwrap().permissions().mode() & 0o777,
                    0o755
                );
            }
        }
    }

    #[test]
    fn failed_cleanup_restores_outputs_and_reports_failure() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        fs::create_dir_all(target.join("debug/deps")).unwrap();
        fs::write(target.join("debug/app"), "executable").unwrap();
        let result = reset(&target, || {
            fs::remove_dir_all(&target)?;
            Err(io::Error::other("partial clean failure"))
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(target.join("debug/app")).unwrap(),
            "executable"
        );
    }
}
