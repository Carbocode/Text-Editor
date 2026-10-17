//! Tabs, buffers, saving and conflict resolution.

use crate::{AppError, AppEvent, Application};
use native_workspace_domain::DocumentId;

impl Application {
    /// Opens `id` in a new tab after the active one, or activates its tab.
    /// A preview replaces the previous preview, only once `id` has loaded.
    pub(super) fn open_document(
        &mut self,
        id: DocumentId,
        preview: bool,
    ) -> Result<Vec<AppEvent>, AppError> {
        let root = self.workspace_root()?.to_path_buf();
        let entry = self.document(&id)?;
        let state = self.state();
        let mut events = Vec::new();
        let new_tab = !state.open_documents.contains(&id);
        let preview = preview && !entry.dirty();
        if !entry.is_loaded() {
            let content = self.filesystem.read_document(&root, &id)?;
            events.push(AppEvent::DocumentLoaded {
                id: id.clone(),
                content: content.into(),
            });
        }
        let mut target_index = state
            .active_document
            .as_ref()
            .and_then(|active| state.open_documents.iter().position(|open| open == active))
            .map_or(state.open_documents.len(), |index| index + 1);
        let mut tab_count = state.open_documents.len();
        if preview
            && new_tab
            && let Some(replaced) = &state.preview_document
        {
            let replaced_index = state
                .open_documents
                .iter()
                .position(|open| open == replaced)
                .expect("the preview is an open tab");
            if replaced_index < target_index {
                target_index -= 1;
            }
            tab_count -= 1;
            events.push(AppEvent::DocumentClosed {
                id: replaced.clone(),
            });
        }
        if state.active_document.as_ref() != Some(&id) {
            events.push(AppEvent::DocumentOpened { id: id.clone() });
        }
        if new_tab && target_index < tab_count {
            events.push(AppEvent::DocumentTabMoved {
                id: id.clone(),
                index: target_index,
            });
        }
        if preview && new_tab {
            events.push(AppEvent::DocumentPreviewed { id });
        } else if !preview && state.preview_document.as_ref() == Some(&id) {
            events.push(AppEvent::DocumentKeptOpen { id });
        }
        Ok(events)
    }

    pub(super) fn keep_document_open(&self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        self.require_tab(&id)?;
        Ok(self.promotion(id))
    }

    pub(super) fn move_document_tab(
        &self,
        id: DocumentId,
        index: usize,
    ) -> Result<Vec<AppEvent>, AppError> {
        let old = self.require_tab(&id)?;
        let index = index.min(self.state().open_documents.len() - 1);
        Ok(if old == index {
            Vec::new()
        } else {
            vec![AppEvent::DocumentTabMoved { id, index }]
        })
    }

    pub(super) fn close_document(&self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        if self.document(&id)?.dirty() {
            return Err(AppError::UnsavedChanges);
        }
        Ok(self.discard_document(id))
    }

    pub(super) fn discard_document(&self, id: DocumentId) -> Vec<AppEvent> {
        if self.state().open_documents.contains(&id) {
            vec![AppEvent::DocumentClosed { id }]
        } else {
            Vec::new()
        }
    }

    pub(super) fn change_document(
        &self,
        id: DocumentId,
        content: String,
    ) -> Result<Vec<AppEvent>, AppError> {
        self.workspace_root()?;
        Ok(if **self.buffer(&id)? == *content {
            Vec::new()
        } else {
            vec![AppEvent::DocumentChanged {
                id,
                content: content.into(),
            }]
        })
    }

    /// Writes a modified buffer unless the disk changed since it was loaded.
    /// Saving a clean preview keeps it open.
    pub(super) fn save_document(&self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        let root = self.workspace_root()?;
        let content = self.buffer(&id)?;
        let entry = self.document(&id)?;
        if !entry.dirty() {
            return Ok(self.promotion(id));
        }
        let disk = self.filesystem.read_document(root, &id).ok();
        if disk.as_deref() != entry.saved_content() {
            return Err(AppError::ExternalChanges(id));
        }
        self.filesystem.write_document(root, &id, content)?;
        Ok(vec![AppEvent::DocumentSaved { id }])
    }

    pub(super) fn reload_document(&self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        let root = self.workspace_root()?;
        self.document(&id)?;
        let content = self.filesystem.read_document(root, &id)?;
        Ok(vec![AppEvent::DocumentLoaded {
            id,
            content: content.into(),
        }])
    }

    /// Writes the buffer even though the disk changed: the explicit
    /// resolution of an external-change conflict.
    pub(super) fn overwrite_document(&self, id: DocumentId) -> Result<Vec<AppEvent>, AppError> {
        let root = self.workspace_root()?;
        self.filesystem
            .write_document(root, &id, self.buffer(&id)?)?;
        Ok(vec![AppEvent::DocumentSaved { id }])
    }

    fn require_tab(&self, id: &DocumentId) -> Result<usize, AppError> {
        self.state()
            .open_documents
            .iter()
            .position(|open| open == id)
            .ok_or_else(|| AppError::DocumentNotOpen(id.clone()))
    }

    fn promotion(&self, id: DocumentId) -> Vec<AppEvent> {
        if self.state().preview_document.as_ref() == Some(&id) {
            vec![AppEvent::DocumentKeptOpen { id }]
        } else {
            Vec::new()
        }
    }
}
