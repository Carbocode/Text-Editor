//! Folder listings for native sidebars. Snapshot paths are sorted, so every
//! folder's descendants form one contiguous range found by binary search. A
//! listing costs its direct children, not the workspace size, which keeps
//! collapsed dependency trees such as node_modules out of each UI update.
use crate::{AppSnapshot, DocumentSummary};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarChild {
    pub path: String,
    pub directory: bool,
}

impl AppSnapshot {
    /// Direct children of `parent` ("" for the root): folders first, then by
    /// case-insensitive name, matching the order of a full sidebar sort.
    pub fn sidebar_children(&self, parent: &str) -> Vec<SidebarChild> {
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let mut children = Vec::new();
        direct_children(
            &self.directories,
            String::as_str,
            true,
            &prefix,
            &mut children,
        );
        direct_children(
            &self.documents,
            |document| document.id.as_str(),
            false,
            &prefix,
            &mut children,
        );
        children.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(right.directory.cmp(&left.directory))
        });
        children.dedup_by(|later, earlier| later.path == earlier.path);
        children.sort_by_cached_key(|child| {
            let name = child.path.rsplit('/').next().unwrap_or(&child.path);
            (!child.directory, name.to_lowercase(), name.to_owned())
        });
        children
    }

    /// Whether the folder has any entry, without listing its children.
    pub fn sidebar_has_children(&self, parent: &str) -> bool {
        let prefix = format!("{parent}/");
        let starts = |path: &str| path.starts_with(&prefix);
        self.directories
            .get(
                self.directories
                    .partition_point(|path| path.as_str() < prefix.as_str()),
            )
            .is_some_and(|path| starts(path))
            || self
                .documents
                .get(
                    self.documents
                        .partition_point(|document| document.id.as_str() < prefix.as_str()),
                )
                .is_some_and(|document| starts(document.id.as_str()))
    }

    /// The listed document with this path, found without scanning the list.
    pub fn document_summary(&self, path: &str) -> Option<&DocumentSummary> {
        self.documents
            .binary_search_by(|document| document.id.as_str().cmp(path))
            .ok()
            .map(|index| &self.documents[index])
    }
}

/// Appends direct children within `prefix`, skipping each nested folder's
/// range. A nested path also reveals its folder when that folder is unlisted.
fn direct_children<T>(
    items: &[T],
    path: impl Fn(&T) -> &str,
    folders: bool,
    prefix: &str,
    children: &mut Vec<SidebarChild>,
) {
    let mut index = items.partition_point(|item| path(item) < prefix);
    while let Some(item) = items.get(index).map(&path) {
        let Some(rest) = item.strip_prefix(prefix) else {
            break;
        };
        match rest.find('/') {
            None => {
                children.push(SidebarChild {
                    path: item.to_owned(),
                    directory: folders,
                });
                index += 1;
            }
            Some(separator) => {
                let folder = &item[..prefix.len() + separator];
                children.push(SidebarChild {
                    path: folder.to_owned(),
                    directory: true,
                });
                let nested = format!("{folder}/");
                index += items[index..].partition_point(|item| path(item).starts_with(&nested));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_workspace_domain::DocumentId;

    fn snapshot(directories: &[&str], documents: &[&str]) -> AppSnapshot {
        let mut directories: Vec<_> = directories.iter().map(|path| path.to_string()).collect();
        directories.sort();
        let mut documents: Vec<_> = documents
            .iter()
            .map(|path| DocumentSummary {
                id: DocumentId::parse(*path).unwrap(),
                title: path.to_string(),
                dirty: false,
                external_change: false,
            })
            .collect();
        documents.sort_by(|left, right| left.id.cmp(&right.id));
        AppSnapshot {
            open_documents: Vec::new(),
            diagnostic_operation: 0,
            revision: 0,
            diagnostic_counter: 0,
            workspace_name: None,
            workspace_root: None,
            recent_workspaces: Vec::new(),
            search_query: Default::default(),
            search_results: Default::default(),
            search_in_progress: false,
            search_truncated: false,
            directories: directories.into(),
            documents: documents.into(),
            active_document: None,
        }
    }

    fn paths(children: Vec<SidebarChild>) -> Vec<(String, bool)> {
        children
            .into_iter()
            .map(|child| (child.path, child.directory))
            .collect()
    }

    #[test]
    fn lists_direct_children_with_folders_first() {
        let snapshot = snapshot(
            &[
                "node_modules",
                "node_modules/a",
                "node_modules/a/lib",
                "src",
                "z",
            ],
            &[
                ".gitignore",
                "B.txt",
                "a.txt",
                "node_modules/a/index.js",
                "node_modules/a/lib/deep.js",
                "src/main.rs",
                "z/b.txt",
                "loose/only.txt",
            ],
        );
        let owned = |items: &[(&str, bool)]| -> Vec<(String, bool)> {
            items
                .iter()
                .map(|(path, dir)| (path.to_string(), *dir))
                .collect()
        };
        assert_eq!(
            paths(snapshot.sidebar_children("")),
            owned(&[
                ("loose", true),
                ("node_modules", true),
                ("src", true),
                ("z", true),
                (".gitignore", false),
                ("a.txt", false),
                ("B.txt", false),
            ])
        );
        assert_eq!(
            paths(snapshot.sidebar_children("node_modules/a")),
            owned(&[
                ("node_modules/a/lib", true),
                ("node_modules/a/index.js", false)
            ])
        );
        assert_eq!(
            paths(snapshot.sidebar_children("src")),
            owned(&[("src/main.rs", false)])
        );
        assert!(snapshot.sidebar_children("missing").is_empty());
        assert!(snapshot.sidebar_has_children("node_modules/a/lib"));
        assert!(!snapshot.sidebar_has_children("src/main.rs"));
        assert!(!snapshot.sidebar_has_children("missing"));
        assert!(snapshot.document_summary("z/b.txt").is_some());
        assert!(snapshot.document_summary("z").is_none());
    }

    #[test]
    fn similar_prefixes_stay_in_their_folders() {
        let snapshot = snapshot(
            &["a", "a-b", "a/b", "a.b"],
            &["a-b/x.txt", "a/b/y.txt", "a/c.txt", "a.b/z.txt", "a0.txt"],
        );
        assert_eq!(
            paths(snapshot.sidebar_children("a")),
            vec![("a/b".to_owned(), true), ("a/c.txt".to_owned(), false)]
        );
        assert_eq!(
            paths(snapshot.sidebar_children("a-b")),
            vec![("a-b/x.txt".to_owned(), false)]
        );
    }
}
