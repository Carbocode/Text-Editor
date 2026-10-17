//! Creating, renaming, moving and deleting files and folders.

use crate::{AppError, AppEvent, Application, Relocation};
use native_workspace_domain::{DocumentId, path};

impl Application {
    pub(super) fn create_entry(
        &mut self,
        id: &DocumentId,
        directory: bool,
    ) -> Result<Vec<AppEvent>, AppError> {
        if id.is_external() {
            return Err(AppError::InvalidEntryName);
        }
        let root = self.workspace_root()?.to_path_buf();
        self.filesystem.create_entry(&root, id, directory)?;
        self.refresh_workspace()
    }

    /// Rename and move share validation, the filesystem operation and the
    /// event; open buffers and conflicts follow their paths.
    pub(super) fn relocate(&mut self, relocation: Relocation) -> Result<Vec<AppEvent>, AppError> {
        if !self.validate_relocation(&relocation)? {
            return Ok(Vec::new());
        }
        let root = self.workspace_root()?.to_path_buf();
        let trace = self.drop_trace.clone().unwrap_or_default();
        trace.filesystem_started();
        let result = self
            .filesystem
            .rename_entry(&root, &relocation.source, &relocation.target);
        trace.filesystem_finished(result.is_ok());
        result?;
        Ok(vec![AppEvent::EntryRelocated {
            id: relocation.source,
            target: relocation.target,
        }])
    }

    /// The validated relocation a rename or move would perform, or `None`
    /// when the entry would not move. Nothing touches the disk.
    pub(crate) fn plan_relocation(
        &self,
        relocation: Relocation,
    ) -> Result<Option<Relocation>, AppError> {
        Ok(self.validate_relocation(&relocation)?.then_some(relocation))
    }

    /// `Ok(false)` when the entry would not move.
    fn validate_relocation(&self, relocation: &Relocation) -> Result<bool, AppError> {
        let Relocation { source, target } = relocation;
        if source.is_external() || target.is_external() {
            return Err(AppError::InvalidEntryName);
        }
        self.workspace_root()?;
        if !self.state().entry_exists(source.as_str()) {
            return Err(AppError::DocumentNotFound(source.clone()));
        }
        if target == source {
            return Ok(false);
        }
        if path::is_descendant(target.as_str(), source.as_str()) {
            return Err(AppError::InvalidMove);
        }
        self.validate_destination(target)?;
        Ok(true)
    }

    pub(crate) fn validate_destination(&self, target: &DocumentId) -> Result<(), AppError> {
        let parent = target.parent();
        if !parent.is_empty()
            && self
                .state()
                .directories
                .binary_search_by(|dir| dir.as_str().cmp(parent))
                .is_err()
        {
            return Err(match DocumentId::parse(parent) {
                Ok(parent) => AppError::DocumentNotFound(parent),
                Err(_) => AppError::InvalidEntryName,
            });
        }
        // Protect buffers retained after external deletion as well as disk entries.
        let prefix = format!("{target}/");
        let entries = self.state().documents.entries();
        let first_file = entries.partition_point(|entry| entry.id().as_str() < prefix.as_str());
        let first_folder = self
            .state()
            .directories
            .partition_point(|folder| folder.as_str() < prefix.as_str());
        let occupied = self.state().entry_exists(target.as_str())
            || entries
                .get(first_file)
                .is_some_and(|entry| entry.id().is_within(target))
            || self
                .state()
                .directories
                .get(first_folder)
                .is_some_and(|folder| path::is_within(folder, target.as_str()));
        if occupied {
            return Err(AppError::DestinationExists(target.clone()));
        }
        Ok(())
    }

    pub(super) fn delete_entry(&mut self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        if id.is_external() {
            return Err(AppError::InvalidEntryName);
        }
        let root = self.workspace_root()?.to_path_buf();
        if !self.state().entry_exists(id.as_str()) {
            return Err(AppError::DocumentNotFound(id));
        }
        let modified = self
            .state()
            .documents
            .entries()
            .iter()
            .any(|entry| entry.id().is_within(&id) && entry.dirty());
        if modified {
            return Err(AppError::DeleteUnsavedChanges);
        }
        self.filesystem.delete_entry(&root, &id)?;
        self.pending_reconcile = None;
        Ok(vec![AppEvent::EntryDeleted { id }])
    }
}
