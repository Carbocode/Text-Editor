//! OS file drops: native adapters provide paths and a destination intention.
use crate::{AppError, AppEvent, Application, Relocation};
use native_workspace_domain::DocumentId;
use std::path::Path;

impl Application {
    fn file_id(&self, resolved: &Path) -> Result<DocumentId, AppError> {
        let path = resolved
            .strip_prefix(self.workspace_root()?)
            .unwrap_or(resolved);
        let value = path.to_str().ok_or(AppError::InvalidEntryName)?;
        DocumentId::from_document_path(value).map_err(|_| AppError::InvalidEntryName)
    }

    pub(super) fn open_file(&mut self, path: &Path) -> Result<Vec<AppEvent>, AppError> {
        let path = self.filesystem.resolve_file(path)?;
        let id = self.file_id(&path)?;
        self.open_document_from_file(id)
    }

    pub(super) fn move_file(
        &mut self,
        path: &Path,
        directory: &str,
    ) -> Result<Vec<AppEvent>, AppError> {
        let path = self.filesystem.resolve_file(path)?;
        let source = self.file_id(&path)?;
        let target = DocumentId::child_of(directory, source.name())
            .map_err(|_| AppError::InvalidEntryName)?;
        if source == target {
            return Ok(Vec::new());
        }
        if !source.is_external() && self.state().documents.contains(&source) {
            return self.relocate(Relocation { source, target });
        }
        self.validate_destination(&target)?;
        self.filesystem
            .move_file(&path, self.workspace_root()?, &target)?;
        Ok(if self.state().documents.contains(&source) {
            vec![AppEvent::EntryRelocated { id: source, target }]
        } else {
            vec![AppEvent::DocumentAdded { id: target }]
        })
    }
}
