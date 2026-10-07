//! The only UniFFI boundary in the repository.

use native_workspace_application::{AppCommand, Application};
use std::sync::Mutex;

uniffi::setup_scaffolding!();

#[derive(Clone, Copy, uniffi::Record)]
pub struct WorkspaceSnapshot {
    pub revision: u64,
    pub counter: u64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WorkspaceError {
    #[error("diagnostic counter overflow")]
    CounterOverflow,
    #[error("application revision overflow")]
    RevisionOverflow,
    #[error("application lock poisoned")]
    LockPoisoned,
}

#[derive(uniffi::Object)]
pub struct WorkspaceApplication {
    inner: Mutex<Application>,
}

#[uniffi::export]
impl WorkspaceApplication {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Application::new()),
        }
    }

    pub fn snapshot(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        let snapshot = app.snapshot();
        Ok(WorkspaceSnapshot {
            revision: snapshot.revision,
            counter: snapshot.diagnostic_counter,
        })
    }

    pub fn increment(&self) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let mut app = self
            .inner
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned)?;
        let snapshot = app
            .dispatch(AppCommand::IncrementDiagnostic)
            .map_err(|err| match err {
                native_workspace_application::AppError::CounterOverflow => {
                    WorkspaceError::CounterOverflow
                }
                native_workspace_application::AppError::RevisionOverflow => {
                    WorkspaceError::RevisionOverflow
                }
            })?;
        Ok(WorkspaceSnapshot {
            revision: snapshot.revision,
            counter: snapshot.diagnostic_counter,
        })
    }
}

impl Default for WorkspaceApplication {
    fn default() -> Self {
        Self::new()
    }
}
