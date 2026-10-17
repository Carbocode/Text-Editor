//! Workspace-relative path algebra. Every crate and Rust frontend uses these
//! functions instead of splitting or prefix-matching path strings by hand.
//!
//! Paths are slash-separated and relative to the workspace root. The root
//! itself is the empty string.

use std::borrow::Cow;

/// The last path component; the root's name is empty.
pub fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The containing folder; top-level entries and the root have the root ("").
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Number of folders above the entry: 0 for a top-level entry.
pub fn depth(path: &str) -> usize {
    path.matches('/').count()
}

/// `name` inside `directory`, where "" is the workspace root.
pub fn join(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}

/// Whether `path` is `ancestor` itself or lies inside it. Every path is
/// within the root ("").
pub fn is_within(path: &str, ancestor: &str) -> bool {
    ancestor.is_empty() || path == ancestor || is_descendant(path, ancestor)
}

/// Whether `path` lies strictly inside `ancestor`.
pub fn is_descendant(path: &str, ancestor: &str) -> bool {
    if ancestor.is_empty() {
        return !path.is_empty();
    }
    path.strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

/// `path` after relocating `from` to `to`: entries within `from` move with
/// it, every other path is returned unchanged.
pub fn rebase<'a>(path: &'a str, from: &str, to: &str) -> Cow<'a, str> {
    if path == from {
        Cow::Owned(to.to_owned())
    } else if let Some(suffix) = path
        .strip_prefix(from)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .filter(|_| !from.is_empty())
    {
        Cow::Owned(join(to, suffix))
    } else {
        Cow::Borrowed(path)
    }
}

/// Every proper ancestor folder of `path`, nearest first, excluding the root.
pub fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(parent(path)), |current| Some(parent(current)))
        .take_while(|ancestor| !ancestor.is_empty())
}

/// The sidebar order of sibling entries: folders first, then by
/// case-insensitive name, then by exact name so the order is total.
pub fn sibling_order_key(path: &str, directory: bool) -> (bool, String, &str) {
    let name = name(path);
    (!directory, name.to_lowercase(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components() {
        assert_eq!(name("a/b/c.rs"), "c.rs");
        assert_eq!(name("c.rs"), "c.rs");
        assert_eq!(parent("a/b/c.rs"), "a/b");
        assert_eq!(parent("c.rs"), "");
        assert_eq!(depth("a/b/c.rs"), 2);
        assert_eq!(depth("c.rs"), 0);
        assert_eq!(join("", "c.rs"), "c.rs");
        assert_eq!(join("a/b", "c.rs"), "a/b/c.rs");
        assert_eq!(ancestors("a/b/c.rs").collect::<Vec<_>>(), ["a/b", "a"]);
        assert_eq!(ancestors("c.rs").count(), 0);
    }

    #[test]
    fn containment_respects_component_boundaries() {
        assert!(is_within("src", "src"));
        assert!(is_within("src/main.rs", "src"));
        assert!(!is_within("src2/main.rs", "src"));
        assert!(!is_within("sr", "src"));
        assert!(is_within("anything", ""));
        assert!(!is_descendant("src", "src"));
        assert!(is_descendant("src/a", "src"));
        assert!(is_descendant("src", ""));
        assert!(!is_descendant("", ""));
    }

    #[test]
    fn rebase_moves_only_the_subtree() {
        assert_eq!(rebase("src", "src", "lib/src"), "lib/src");
        assert_eq!(rebase("src/a.rs", "src", "lib/src"), "lib/src/a.rs");
        assert_eq!(rebase("src/a.rs", "src", "app"), "app/a.rs");
        assert_eq!(rebase("src2/a.rs", "src", "lib"), "src2/a.rs");
        assert_eq!(rebase("lib/src/a.rs", "lib/src", "src"), "src/a.rs");
        assert!(matches!(rebase("other", "src", "lib"), Cow::Borrowed(_)));
    }

    #[test]
    fn siblings_put_folders_first_then_case_insensitive_names() {
        let mut entries = [("b.rs", false), ("A.rs", false), ("z", true), ("a", false)];
        entries.sort_by_key(|(path, directory)| sibling_order_key(path, *directory));
        assert_eq!(entries.map(|(path, _)| path), ["z", "a", "A.rs", "b.rs"]);
    }
}
