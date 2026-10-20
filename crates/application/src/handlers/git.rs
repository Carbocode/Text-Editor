use crate::{AppError, AppEvent, Application, GitSnapshot};
use std::sync::Arc;

impl Application {
    pub(super) fn refresh_git(
        &self,
        preserve: bool,
        limit: Option<usize>,
    ) -> Result<Vec<AppEvent>, AppError> {
        let workspace = self.workspace_root()?;
        let mut candidates = vec![workspace.to_path_buf()];
        for directory in self
            .state()
            .directories
            .iter()
            .filter(|d| native_workspace_domain::path::name(d) == ".git")
        {
            let root = workspace.join(native_workspace_domain::path::parent(directory));
            if !candidates.contains(&root) {
                candidates.push(root);
            }
        }
        for entry in self
            .state()
            .documents
            .entries()
            .iter()
            .filter(|e| !e.id().is_external() && e.id().name() == ".git")
        {
            let root = workspace.join(entry.id().parent());
            if !candidates.contains(&root) {
                candidates.push(root);
            }
        }
        candidates.truncate(32);
        let roots: Vec<_> = candidates
            .into_iter()
            .filter(|root| {
                self.filesystem
                    .git_repository(root)
                    .ok()
                    .flatten()
                    .is_some()
            })
            .collect();
        let selected = self
            .state()
            .git
            .selected_root
            .as_ref()
            .filter(|root| roots.contains(root))
            .cloned()
            .or_else(|| roots.first().cloned());
        let root = selected.as_deref().unwrap_or(workspace);
        let repository = self.filesystem.git_repository(root)?;
        let limit = limit
            .unwrap_or(self.state().git.history_limit)
            .clamp(1, 5000);
        let history = if repository.is_some() {
            crate::git::graph(self.filesystem.git_history(root, limit)?)
        } else {
            Vec::new()
        };
        let mut snapshot = GitSnapshot {
            loaded: true,
            roots,
            selected_root: selected.clone(),
            repository,
            history,
            history_limit: limit,
            ..GitSnapshot::default()
        };
        if preserve && selected == self.state().git.selected_root {
            snapshot.inspection = self.state().git.inspection.clone();
            if let Some(diff) = &self.state().git.diff {
                snapshot.diff = self.filesystem.git_diff(root, &diff.path, diff.staged).ok();
                snapshot.patch_lines = snapshot
                    .diff
                    .as_ref()
                    .map(|d| crate::git::patch::lines(&d.text))
                    .unwrap_or_default();
            }
        }
        if *self.state().git == snapshot {
            Ok(Vec::new())
        } else {
            Ok(vec![AppEvent::GitUpdated {
                snapshot: Arc::new(snapshot),
            }])
        }
    }

    pub(super) fn show_git_diff(
        &self,
        path: &str,
        staged: bool,
    ) -> Result<Vec<AppEvent>, AppError> {
        let diff = self.filesystem.git_diff(self.git_root()?, path, staged)?;
        let mut snapshot = (*self.state().git).clone();
        snapshot.inspection = None;
        snapshot.patch_lines = crate::git::patch::lines(&diff.text);
        snapshot.diff = Some(diff);
        Ok(vec![AppEvent::GitUpdated {
            snapshot: Arc::new(snapshot),
        }])
    }

    pub(super) fn inspect_git(&self, query: &crate::GitQuery) -> Result<Vec<AppEvent>, AppError> {
        let mut snapshot = (*self.state().git).clone();
        let mut inspection = self.filesystem.git_inspect(self.git_root()?, query)?;
        if let Some(conflict) = &mut inspection.conflict {
            let (current, incoming, both) = crate::git::conflict_choices(&conflict.expected);
            conflict.accept_current = current;
            conflict.accept_incoming = incoming;
            conflict.accept_both = both;
        }
        snapshot.inspection = Some(inspection);
        snapshot.diff = None;
        snapshot.patch_lines.clear();
        Ok(vec![AppEvent::GitUpdated {
            snapshot: Arc::new(snapshot),
        }])
    }

    pub(crate) fn git_root(&self) -> Result<&std::path::Path, AppError> {
        Ok(self
            .state()
            .git
            .selected_root
            .as_deref()
            .unwrap_or(self.workspace_root()?))
    }
    pub(super) fn select_git_repository(
        &self,
        root: &std::path::Path,
    ) -> Result<Vec<AppEvent>, AppError> {
        if !self
            .state()
            .git
            .roots
            .iter()
            .any(|candidate| candidate == root)
        {
            return Err(AppError::Git(
                "Choose a repository discovered in this workspace.".into(),
            ));
        }
        let mut snapshot = (*self.state().git).clone();
        snapshot.selected_root = Some(root.into());
        snapshot.diff = None;
        snapshot.inspection = None;
        snapshot.repository = self.filesystem.git_repository(root)?;
        snapshot.history =
            crate::git::graph(self.filesystem.git_history(root, snapshot.history_limit)?);
        snapshot.patch_lines.clear();
        Ok(vec![AppEvent::GitUpdated {
            snapshot: Arc::new(snapshot),
        }])
    }
    pub(crate) fn execute_git_operation(
        &mut self,
        operation: &crate::GitOperation,
    ) -> Result<Vec<AppEvent>, AppError> {
        // Index operations must not silently stage older disk content. Operations
        // changing the worktree cannot replace unsaved editor text.
        if crate::git::requires_saved_buffers(operation)
            && self
                .state()
                .documents
                .entries()
                .iter()
                .any(crate::DocumentEntry::dirty)
        {
            return Err(AppError::UnsavedChanges);
        }
        let root = self.git_root()?.to_path_buf();
        let outcome = self
            .filesystem
            .git_operation(&root, operation)
            .map_err(AppError::from);
        // A failed merge or pull can still change the worktree/index. Reconcile
        // those facts before returning its error; no successful step is rolled back.
        let mut events = self.refresh_workspace()?;
        let refreshed = self.refresh_git(false, None);
        match refreshed {
            Ok(git) => events.extend(git),
            Err(error) if outcome.is_ok() => {
                self.commit(&events)?;
                return Err(error);
            }
            Err(_) => {}
        }
        self.commit(&events)?;
        outcome.map(|()| events)
    }
}
