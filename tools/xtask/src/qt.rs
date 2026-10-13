use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::{env, process::Command};

#[cfg(target_os = "macos")]
pub fn configure(command: &mut Command) {
    // Respect an explicit installation; otherwise prefer the cached bootstrap Qt.
    if env::var_os("QMAKE").is_some() {
        return;
    }
    let root = env::var_os("QT_MINIMAL_DOWNLOAD_ROOT")
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|root| root.join("qt_minimal_download")));
    let arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        arch => arch,
    };
    if let Some(qmake) = root.and_then(|root| local_qmake(&root, arch)) {
        // qt-build-utils 0.10 panics on unrelated cache entries such as .DS_Store.
        // QMAKE bypasses its cache scan without modifying the user's cache.
        command.env("QMAKE", qmake);
    }
}

fn compatible(version: &semver::Version) -> bool {
    version.major == 6 && version >= &semver::Version::new(6, 10, 0)
}

fn local_qmake(root: &Path, arch: &str) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let version = semver::Version::parse(entry.file_name().to_str()?).ok()?;
            let qmake = entry.path().join("macos").join(arch).join("qt/bin/qmake");
            (compatible(&version) && qmake.is_file()).then_some((version, qmake))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, qmake)| qmake)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_scan_ignores_metadata_and_selects_latest_compatible_installation() {
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join(".DS_Store"), "Finder metadata").unwrap();
        std::fs::create_dir(cache.path().join("unrelated-folder")).unwrap();
        for version in ["6.9.0", "6.10.0", "6.11.0", "6.12.0", "7.0.0"] {
            let bin = cache.path().join(version).join("macos/arm64/qt/bin");
            std::fs::create_dir_all(&bin).unwrap();
            if version != "6.12.0" {
                std::fs::write(bin.join("qmake"), "").unwrap();
            }
        }
        assert_eq!(
            local_qmake(cache.path(), "arm64"),
            Some(cache.path().join("6.11.0/macos/arm64/qt/bin/qmake"))
        );
        assert_eq!(local_qmake(cache.path(), "x86_64"), None);
    }

    #[test]
    fn absent_cache_leaves_qt_bootstrap_available() {
        let cache = tempfile::tempdir().unwrap();
        assert_eq!(local_qmake(cache.path(), "arm64"), None);
        assert_eq!(local_qmake(&cache.path().join("missing"), "arm64"), None);
    }
}
