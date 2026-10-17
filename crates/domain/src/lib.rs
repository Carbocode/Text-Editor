//! Platform independent domain values.

pub mod path;

use std::path::PathBuf;

/// A validated, slash-separated path of a file or folder relative to the
/// workspace root. Path arithmetic goes through these methods or [`path`],
/// never through ad hoc string splitting.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidDocumentId> {
        let value = value.into();
        let path = std::path::Path::new(&value);
        if value.is_empty()
            || value.contains('\\')
            || value.contains('\0')
            || value
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(InvalidDocumentId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The final component, e.g. `main.rs` for `src/main.rs`.
    pub fn name(&self) -> &str {
        path::name(&self.0)
    }

    /// The containing folder, "" for top-level entries.
    pub fn parent(&self) -> &str {
        path::parent(&self.0)
    }

    /// Whether this entry is `ancestor` or lies inside it.
    pub fn is_within(&self, ancestor: &DocumentId) -> bool {
        path::is_within(&self.0, &ancestor.0)
    }

    /// This entry after `from` has been relocated to `to`.
    pub fn rebased(&self, from: &DocumentId, to: &DocumentId) -> DocumentId {
        // Rebasing a valid path between valid paths yields a valid path.
        Self(path::rebase(&self.0, &from.0, &to.0).into_owned())
    }

    /// The sibling entry called `name`. `name` must be a single component.
    pub fn renamed(&self, name: &str) -> Result<DocumentId, InvalidDocumentId> {
        Self::child_of(self.parent(), name)
    }

    /// This entry moved into `directory` ("" for the workspace root).
    pub fn moved_into(&self, directory: &str) -> Result<DocumentId, InvalidDocumentId> {
        if !directory.is_empty() {
            Self::parse(directory)?;
        }
        Ok(Self(path::join(directory, self.name())))
    }

    /// The entry called `name` inside `directory` ("" for the root).
    pub fn child_of(directory: &str, name: &str) -> Result<DocumentId, InvalidDocumentId> {
        if name.contains('/') {
            return Err(InvalidDocumentId);
        }
        if !directory.is_empty() {
            Self::parse(directory)?;
        }
        Self::parse(path::join(directory, name))
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDocumentId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> DocumentId {
        DocumentId::parse(value).unwrap()
    }

    #[test]
    fn document_id_is_a_safe_relative_file_path() {
        assert_eq!(id("README.md").as_str(), "README.md");
        assert_eq!(id("folder/note.rs").as_str(), "folder/note.rs");
        for invalid in [
            "",
            "../escape.md",
            "folder/../note.md",
            "a//b",
            "a/./b",
            "..",
            "/tmp/file",
            "a\\b",
            "a\0b",
        ] {
            assert_eq!(DocumentId::parse(invalid), Err(InvalidDocumentId));
        }
    }

    #[test]
    fn derived_paths_stay_valid() {
        let note = id("docs/note.md");
        assert_eq!(note.name(), "note.md");
        assert_eq!(note.parent(), "docs");
        assert_eq!(note.renamed("todo.md").unwrap(), id("docs/todo.md"));
        assert!(note.renamed("a/b").is_err());
        assert!(note.renamed("..").is_err());
        assert_eq!(note.moved_into("").unwrap(), id("note.md"));
        assert_eq!(note.moved_into("src").unwrap(), id("src/note.md"));
        assert!(note.moved_into("../x").is_err());
        assert_eq!(
            note.rebased(&id("docs"), &id("archive/docs")),
            id("archive/docs/note.md")
        );
        assert!(note.is_within(&id("docs")));
        assert!(!id("docs2/a").is_within(&id("docs")));
    }
}
