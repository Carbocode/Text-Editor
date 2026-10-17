//! OS file drops and application opens: native adapters provide paths and a destination intention.
use crate::{AppCommand, AppError, AppEvent, Application, Relocation};
use native_workspace_domain::DocumentId;
use native_workspace_filesystem::OpenTarget;
use std::path::Path;

impl Application {
    /// Resolve one OS item into ordinary commands; the application commits them.
    pub(crate) fn open_path_commands(&self, path: &Path) -> Result<Vec<AppCommand>, AppError> {
        Ok(match self.filesystem.resolve_open_target(path)? {
            OpenTarget::Directory(path) => {
                if self.workspace_root().ok() == Some(path.as_path()) {
                    Vec::new()
                } else {
                    vec![AppCommand::OpenWorkspace(path)]
                }
            }
            OpenTarget::File(path) => {
                let mut commands = Vec::new();
                if self.state().workspace.is_none() {
                    let parent = path.parent().ok_or(AppError::InvalidEntryName)?;
                    commands.push(AppCommand::OpenWorkspace(parent.into()));
                }
                commands.push(AppCommand::OpenFile(path));
                commands
            }
        })
    }

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
