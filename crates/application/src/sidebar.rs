//! Sidebar rows for every frontend. The order is defined once here: folders
//! first, then case-insensitive names ([`path::sibling_order_key`]).
//!
//! Snapshot paths are sorted, so every folder's descendants form one
//! contiguous range found by binary search. Listing a folder costs its direct
//! children, not the workspace size, which keeps collapsed dependency trees
//! such as node_modules out of each UI update.

use crate::AppSnapshot;
use native_workspace_domain::path;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarChild {
    pub path: String,
    pub directory: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarRowKind {
    Directory {
        expanded: bool,
    },
    File,
    /// A content match; the index refers to `AppSnapshot::search_results`.
    Match {
        result: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarRow {
    pub path: String,
    pub depth: usize,
    pub kind: SidebarRowKind,
}

impl SidebarRow {
    fn entry(path: String, directory: bool, expanded: bool) -> Self {
        Self {
            depth: path::depth(&path),
            kind: if directory {
                SidebarRowKind::Directory { expanded }
            } else {
                SidebarRowKind::File
            },
            path,
        }
    }

    pub fn is_directory(&self) -> bool {
        matches!(self.kind, SidebarRowKind::Directory { .. })
    }
}

impl AppSnapshot {
    /// Changed ancestors are precomputed; ignored directories apply to descendants.
    pub fn sidebar_git_status(&self, entry: &str) -> &str {
        self.git
            .decorations
            .get(entry)
            .map(String::as_str)
            .unwrap_or_else(|| {
                if path::ancestors(entry)
                    .any(|parent| self.git.decorations.get(parent).is_some_and(|s| s == "!"))
                {
                    "!"
                } else {
                    ""
                }
            })
    }

    /// Direct children of `parent` ("" for the root) in sidebar order.
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
            let (file, lower, name) = path::sibling_order_key(&child.path, child.directory);
            (file, lower, name.to_owned())
        });
        children
    }

    /// Whether the folder has any entry, without listing its children.
    pub fn sidebar_has_children(&self, parent: &str) -> bool {
        if parent.is_empty() {
            return self.documents.iter().any(|d| !d.id.is_external())
                || !self.directories.is_empty();
        }
        let prefix = format!("{parent}/");
        let first_under = |paths: &mut dyn Iterator<Item = &str>| {
            paths.next().is_some_and(|path| path.starts_with(&prefix))
        };
        let directory = self
            .directories
            .partition_point(|path| path.as_str() < prefix.as_str());
        let document = self
            .documents
            .partition_point(|document| document.id.as_str() < prefix.as_str());
        first_under(&mut self.directories[directory..].iter().map(String::as_str))
            || first_under(&mut self.documents[document..].iter().map(|d| d.id.as_str()))
    }

    /// The Files tree without a name filter: the children of every folder
    /// for which `expanded` holds, depth first.
    pub fn file_tree_rows(&self, expanded: impl Fn(&str) -> bool) -> Vec<SidebarRow> {
        fn visit(
            snapshot: &AppSnapshot,
            parent: &str,
            expanded: &dyn Fn(&str) -> bool,
            rows: &mut Vec<SidebarRow>,
        ) {
            for child in snapshot.sidebar_children(parent) {
                let open = child.directory && expanded(&child.path);
                let path = child.path.clone();
                rows.push(SidebarRow::entry(child.path, child.directory, open));
                if open {
                    visit(snapshot, &path, expanded, rows);
                }
            }
        }
        let mut rows = Vec::new();
        visit(self, "", &expanded, &mut rows);
        rows
    }

    /// The Files tree with an active name filter: every match below the
    /// folders that contain it, all expanded.
    pub fn file_filter_rows(&self) -> Vec<SidebarRow> {
        let filter = &self.file_filter;
        let entries = filter.files.iter().map(|file| (file.clone(), false)).chain(
            filter
                .directories
                .iter()
                .map(|folder| (folder.clone(), true)),
        );
        tree_order(entries.collect())
            .into_iter()
            .map(|(path, directory)| SidebarRow::entry(path, directory, true))
            .collect()
    }

    /// The Search tab: every file with matches below the folders that
    /// contain it, each followed by its match rows.
    pub fn search_rows(&self) -> Vec<SidebarRow> {
        let mut matches: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, result) in self.search_results.iter().enumerate() {
            matches.entry(result.id.as_str()).or_default().push(index);
        }
        let mut entries: Vec<(String, bool)> = Vec::new();
        let mut folders = HashSet::new();
        for file in matches.keys() {
            entries.push(((*file).to_owned(), false));
            for ancestor in path::ancestors(file) {
                if !folders.insert(ancestor) {
                    break;
                }
                entries.push((ancestor.to_owned(), true));
            }
        }
        let mut rows = Vec::new();
        for (path, directory) in tree_order(entries) {
            let row = SidebarRow::entry(path, directory, true);
            let depth = row.depth + 1;
            let results = matches.get(row.path.as_str()).cloned().unwrap_or_default();
            let file = row.path.clone();
            rows.push(row);
            rows.extend(results.into_iter().map(|result| SidebarRow {
                path: file.clone(),
                depth,
                kind: SidebarRowKind::Match { result },
            }));
        }
        rows
    }
}

/// Orders entries depth-first in sidebar order. Ancestors missing from the
/// entries still order their descendants without being emitted. Siblings
/// are sorted once with precomputed keys: comparing whole paths rebuilt
/// every prefix per comparison and took seconds on node_modules trees.
fn tree_order(mut entries: Vec<(String, bool)>) -> Vec<(String, bool)> {
    entries.sort_unstable();
    entries.dedup();
    let directories: HashSet<&str> = entries
        .iter()
        .filter(|(_, directory)| *directory)
        .map(|(path, _)| path.as_str())
        .collect();
    let mut children: HashMap<&str, Vec<(&str, Option<usize>)>> = HashMap::new();
    let mut known: HashSet<&str> = entries.iter().map(|(path, _)| path.as_str()).collect();
    for (index, (entry, _)) in entries.iter().enumerate() {
        let mut child = entry.as_str();
        let mut row = Some(index);
        loop {
            let parent = path::parent(child);
            children.entry(parent).or_default().push((child, row));
            if parent.is_empty() || !known.insert(parent) {
                break;
            }
            child = parent;
            row = None;
        }
    }
    // Unlisted ancestors are folders too.
    let parents: HashSet<&str> = children.keys().copied().collect();
    for siblings in children.values_mut() {
        siblings.sort_by_cached_key(|&(entry, _)| {
            let directory = directories.contains(entry) || parents.contains(entry);
            path::sibling_order_key(entry, directory)
        });
    }
    let mut order = Vec::with_capacity(entries.len());
    let mut visited = HashSet::new();
    let mut stack = vec![("", 0)];
    while let Some((parent, position)) = stack.pop() {
        let Some(&(entry, row)) = children
            .get(parent)
            .and_then(|siblings| siblings.get(position))
        else {
            continue;
        };
        order.extend(row);
        stack.push((parent, position + 1));
        if visited.insert(entry) {
            stack.push((entry, 0));
        }
    }
    let mut rows: Vec<_> = entries.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|row| rows[row].take())
        .collect()
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
        if item.starts_with('/') {
            index += 1;
            continue;
        }
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
    use crate::DocumentSummary;
    use native_workspace_domain::DocumentId;

    fn snapshot(directories: &[&str], documents: &[&str]) -> AppSnapshot {
        let mut directories: Vec<_> = directories.iter().map(|path| path.to_string()).collect();
        directories.sort();
        let mut documents: Vec<_> = documents
            .iter()
            .map(|path| DocumentSummary {
                id: DocumentId::parse(*path).unwrap(),
                dirty: false,
                external_change: false,
            })
            .collect();
        documents.sort_by(|left, right| left.id.cmp(&right.id));
        AppSnapshot {
            directories: directories.into(),
            documents: documents.into(),
            ..crate::AppState::default().snapshot()
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

    #[test]
    fn tree_order_puts_folders_first_at_each_level() {
        let entries = [
            ("a.txt", false),
            ("z/b.txt", false),
            ("B.txt", false),
            ("lib/deep/only.txt", false),
            ("z/child.txt", false),
            ("z/x", true),
            ("z", true),
            ("z/x/nested.txt", false),
            ("z/b.txt", false),
        ]
        .map(|(path, directory)| (path.to_owned(), directory));
        let ordered = tree_order(entries.to_vec());
        let paths: Vec<_> = ordered.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "lib/deep/only.txt",
                "z",
                "z/x",
                "z/x/nested.txt",
                "z/b.txt",
                "z/child.txt",
                "a.txt",
                "B.txt",
            ]
        );
    }

    #[test]
    fn rows_expand_only_open_folders_and_list_matches_below_files() {
        let mut snapshot = snapshot(
            &["src", "src/app"],
            &["README.md", "src/app/main.rs", "src/lib.rs"],
        );
        let rows = snapshot.file_tree_rows(|path| path == "src");
        let listed: Vec<_> = rows
            .iter()
            .map(|row| (row.path.as_str(), row.depth))
            .collect();
        assert_eq!(
            listed,
            [
                ("src", 0),
                ("src/app", 1),
                ("src/lib.rs", 1),
                ("README.md", 0)
            ]
        );
        assert_eq!(rows[0].kind, SidebarRowKind::Directory { expanded: true });
        assert_eq!(rows[1].kind, SidebarRowKind::Directory { expanded: false });
        let location = crate::SearchLocation {
            line: 1,
            column: 1,
            start_char: 0,
            end_char: 1,
            start_utf16: 0,
            end_utf16: 1,
        };
        let result = |path: &str| crate::SearchResult {
            id: DocumentId::parse(path).unwrap(),
            snippet: String::new(),
            location: location.clone(),
        };
        snapshot.search_results = vec![
            result("README.md"),
            result("src/app/main.rs"),
            result("src/app/main.rs"),
        ]
        .into();
        let rows = snapshot.search_rows();
        let listed: Vec<_> = rows
            .iter()
            .map(|row| (row.path.as_str(), row.kind))
            .collect();
        assert_eq!(
            listed,
            [
                ("src", SidebarRowKind::Directory { expanded: true }),
                ("src/app", SidebarRowKind::Directory { expanded: true }),
                ("src/app/main.rs", SidebarRowKind::File),
                ("src/app/main.rs", SidebarRowKind::Match { result: 1 }),
                ("src/app/main.rs", SidebarRowKind::Match { result: 2 }),
                ("README.md", SidebarRowKind::File),
                ("README.md", SidebarRowKind::Match { result: 0 }),
            ]
        );
        assert_eq!(rows[3].depth, 3);
    }
}

#[cfg(test)]
mod ordering_regression {
    #[test]
    fn missing_ancestors_order_descendants_without_extra_rows() {
        let entries = [
            ("a.txt", false),
            ("z/b.txt", false),
            ("B.txt", false),
            ("lib/deep/only.txt", false),
            ("z/child.txt", false),
            ("z/x", true),
            ("z", true),
            ("z/x/nested.txt", false),
            ("z/b.txt", false),
        ];
        let paths = super::tree_order(
            entries
                .into_iter()
                .map(|(p, d)| (p.to_owned(), d))
                .collect(),
        );
        assert_eq!(
            paths.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            [
                "lib/deep/only.txt",
                "z",
                "z/x",
                "z/x/nested.txt",
                "z/b.txt",
                "z/child.txt",
                "a.txt",
                "B.txt"
            ]
        );
    }
}
