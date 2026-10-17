//! Session restoration reuses normal validation, loading and reducer events.

use crate::{AppCommand, AppError, AppEvent, Application};
use native_workspace_domain::DocumentId;

impl Application {
    pub(crate) fn restore_session(&mut self) -> Result<Vec<AppEvent>, AppError> {
        // A launch request or an explicit open always takes precedence.
        if self.state().workspace.is_some() {
            return Ok(Vec::new());
        }
        let Some(saved) = self.storage.session()? else {
            return Ok(Vec::new());
        };
        let mut events = self.execute_one(AppCommand::OpenWorkspace(saved.root))?;
        let mut first_error = None;
        for path in &saved.documents {
            let result = (|| {
                let id =
                    DocumentId::from_document_path(path).map_err(|_| AppError::InvalidEntryName)?;
                let command = if id.is_external() {
                    AppCommand::OpenFile(path.into())
                } else {
                    AppCommand::OpenDocument(id)
                };
                self.execute_one(command)
            })();
            match result {
                Ok(opened) => events.extend(opened),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(path) = saved.preview_document
            && let Some(id) = self
                .state()
                .open_documents
                .iter()
                .find(|id| id.as_str() == path)
                .cloned()
        {
            let preview = AppEvent::DocumentPreviewed { id };
            self.commit(std::slice::from_ref(&preview))?;
            events.push(preview);
        }
        let active = saved
            .active_document
            .as_ref()
            .and_then(|path| {
                self.state()
                    .open_documents
                    .iter()
                    .find(|id| id.as_str() == path)
                    .cloned()
            })
            .or_else(|| self.state().open_documents.first().cloned());
        if let Some(id) = active {
            // Activation must not promote the restored preview.
            events.extend(self.execute_one(AppCommand::PreviewDocument(id))?);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(events),
        }
    }
}
