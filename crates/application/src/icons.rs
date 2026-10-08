//! Material Icon Theme associations shared by the Rust frontends.

use std::{collections::HashMap, sync::OnceLock};

struct IconManifest {
    file_names: HashMap<String, String>,
    file_extensions: Vec<(String, String)>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
    default_file: String,
    default_folder: String,
    default_folder_expanded: String,
}

fn manifest() -> &'static IconManifest {
    static MANIFEST: OnceLock<IconManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/material-icon-theme/manifest.json"
        ))
        .expect("bundled Material Icon Theme manifest");
        let table = |key: &str| -> HashMap<String, String> {
            value[key]
                .as_object()
                .expect("icon association table")
                .iter()
                .map(|(name, icon)| {
                    (
                        name.to_lowercase(),
                        icon.as_str().expect("icon name").to_owned(),
                    )
                })
                .collect()
        };
        let mut file_extensions: Vec<_> = table("fileExtensions").into_iter().collect();
        file_extensions.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        IconManifest {
            file_names: table("fileNames"),
            file_extensions,
            folder_names: table("folderNames"),
            folder_names_expanded: table("folderNamesExpanded"),
            default_file: value["file"]
                .as_str()
                .expect("default file icon")
                .to_owned(),
            default_folder: value["folder"]
                .as_str()
                .expect("default folder icon")
                .to_owned(),
            default_folder_expanded: value["folderExpanded"]
                .as_str()
                .expect("default expanded folder icon")
                .to_owned(),
        }
    })
}

pub fn file_icon_name(path: &str) -> &'static str {
    let manifest = manifest();
    let path = path.to_lowercase();
    let name = path.rsplit('/').next().unwrap_or(&path);
    if let Some(icon) = manifest
        .file_names
        .get(&path)
        .or_else(|| manifest.file_names.get(name))
    {
        return icon;
    }
    for (extension, icon) in &manifest.file_extensions {
        if name == extension
            || (name.len() > extension.len()
                && name.ends_with(extension)
                && name.as_bytes()[name.len() - extension.len() - 1] == b'.')
        {
            return icon;
        }
    }
    &manifest.default_file
}

pub fn folder_icon_name(path: &str, expanded: bool) -> &'static str {
    let manifest = manifest();
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let icon = if expanded {
        manifest
            .folder_names_expanded
            .get(&name)
            .unwrap_or(&manifest.default_folder_expanded)
    } else {
        manifest
            .folder_names
            .get(&name)
            .unwrap_or(&manifest.default_folder)
    };
    if icon == "folder-macos" || icon == "folder-macos-open" {
        if expanded {
            &manifest.default_folder_expanded
        } else {
            &manifest.default_folder
        }
    } else {
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_names_before_extensions_and_nested_folders() {
        assert_eq!(file_icon_name("src/main.rs"), "rust");
        assert_eq!(file_icon_name("README.md"), "readme");
        assert_eq!(file_icon_name("nested/package.json"), "nodejs");
        assert_eq!(file_icon_name(".ncurc.json"), "dependencies-update");
        assert_eq!(folder_icon_name("project/src", false), "folder-src");
        assert_eq!(folder_icon_name("project/src", true), "folder-src-open");
        assert_eq!(folder_icon_name("apps/macos", false), "folder");
        assert_eq!(folder_icon_name("apps/macos", true), "folder-open");
        assert_eq!(folder_icon_name("vendor/apple", false), "folder");
        assert_eq!(folder_icon_name("vendor/osx", false), "folder");
    }
}
