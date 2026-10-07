//! Platform independent domain values.

use std::path::PathBuf;

/// A file name within the flat Phase 1 workspace root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidDocumentId> {
        let value = value.into();
        let path = std::path::Path::new(&value);
        if value.is_empty()
            || value == "."
            || value == ".."
            || path.components().count() != 1
            || path.file_name().is_none_or(|name| name != value.as_str())
            || !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("md" | "txt")
            )
        {
            return Err(InvalidDocumentId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDocumentId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
}

/// A monotonically increasing diagnostic value used to verify each adapter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticState {
    pub counter: u64,
}

impl DiagnosticState {
    pub fn increment(&mut self) -> Result<(), CounterOverflow> {
        self.counter = self.counter.checked_add(1).ok_or(CounterOverflow)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CounterOverflow;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_and_rejects_overflow() {
        let mut state = DiagnosticState::default();
        state.increment().unwrap();
        assert_eq!(state.counter, 1);
        state.counter = u64::MAX;
        assert_eq!(state.increment(), Err(CounterOverflow));
        assert_eq!(state.counter, u64::MAX);
    }

    #[test]
    fn document_id_is_one_supported_file_name() {
        assert_eq!(
            DocumentId::parse("README.md").unwrap().as_str(),
            "README.md"
        );
        for invalid in ["", "../escape.md", "folder/note.md", "note.rs", ".."] {
            assert_eq!(DocumentId::parse(invalid), Err(InvalidDocumentId));
        }
    }
}
