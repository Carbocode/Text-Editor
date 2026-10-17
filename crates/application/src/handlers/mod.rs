//! Command handlers, one module per area. A handler validates its command,
//! performs any filesystem or storage I/O, and returns the events to commit.
//! Handlers never mutate [`crate::AppState`]; `Application::commit` does.

mod documents;
mod entries;
mod external_files;
mod restore;
mod search;
pub(crate) mod workspace;

use crate::{AppCommand, AppError, AppEvent, Application, DocumentEntry};
use native_workspace_domain::DocumentId;

impl Application {
    pub(crate) fn resolve(&mut self, command: AppCommand) -> Result<Vec<AppEvent>, AppError> {
        match command {
            AppCommand::OpenFile(path) => self.open_file(&path),
            AppCommand::MoveFile { path, directory } => self.move_file(&path, &directory),
            AppCommand::OpenWorkspace(path) => self.open_workspace(&path),
            AppCommand::RefreshWorkspace => self.refresh_workspace(),
            AppCommand::PollWorkspaceChanges => self.poll_workspace_changes(),
            AppCommand::QueueSearchQuery(query) => Ok(self.queue_search_query(query)),
            AppCommand::ConfigureSearch(options) => {
                Ok(vec![AppEvent::SearchConfigured { options }])
            }
            AppCommand::RefreshSearch => Ok(vec![AppEvent::SearchStarted]),
            AppCommand::CancelSearch => Ok(vec![AppEvent::SearchCancelled]),
            AppCommand::PrepareReplacement {
                replacement,
                preserve_case,
                target,
            } => self.prepare_replacement(&replacement, preserve_case, target),
            AppCommand::DiscardReplacement => {
                Ok(vec![AppEvent::ReplacementPrepared { preview: None }])
            }
            AppCommand::PollSearchResults => Ok(self.poll_search_results()),
            AppCommand::CreateEntry { id, directory } => self.create_entry(&id, directory),
            AppCommand::RenameEntry { .. } | AppCommand::MoveEntry { .. } => {
                let relocation = command.relocation().expect("rename and move relocate")?;
                self.relocate(relocation)
            }
            AppCommand::DeleteEntry(id) => self.delete_entry(id),
            AppCommand::OpenDocument(id) => self.open_document(id, false),
            AppCommand::PreviewDocument(id) => self.open_document(id, true),
            AppCommand::KeepDocumentOpen(id) => self.keep_document_open(id),
            AppCommand::MoveDocumentTab { id, index } => self.move_document_tab(id, index),
            AppCommand::CloseDocument(id) => self.close_document(id),
            AppCommand::DiscardAndCloseDocument(id) => Ok(self.discard_document(id)),
            AppCommand::ChangeDocument { id, content } => self.change_document(id, content),
            AppCommand::SaveDocument(id) => self.save_document(id),
            AppCommand::ReloadDocument(id) => self.reload_document(id),
            AppCommand::OverwriteDocument(id) => self.overwrite_document(id),
            AppCommand::RestoreSession
            | AppCommand::PrepareToQuit { .. }
            | AppCommand::OpenPaths(_)
            | AppCommand::ApplyReplacement
            | AppCommand::OpenFiles(_)
            | AppCommand::MoveFiles { .. }
            | AppCommand::SaveAllDocuments
            | AppCommand::SaveAndCloseDocument(_) => {
                unreachable!("composite commands run as their steps")
            }
        }
    }

    /// The listed document, or `DocumentNotFound`.
    pub(crate) fn document(&self, id: &DocumentId) -> Result<&DocumentEntry, AppError> {
        self.state()
            .documents
            .get(id)
            .ok_or_else(|| AppError::DocumentNotFound(id.clone()))
    }

    /// The document's buffer, or `DocumentNotOpen` when it is not loaded.
    pub(crate) fn buffer(&self, id: &DocumentId) -> Result<&std::sync::Arc<str>, AppError> {
        self.document(id)?
            .content()
            .ok_or_else(|| AppError::DocumentNotOpen(id.clone()))
    }
}
