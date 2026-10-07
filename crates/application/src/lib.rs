//! Shared application commands and owned snapshots.

use native_workspace_domain::{CounterOverflow, DiagnosticState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppCommand {
    IncrementDiagnostic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub diagnostic_counter: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppError {
    CounterOverflow,
    RevisionOverflow,
}

#[derive(Debug, Default)]
pub struct Application {
    revision: u64,
    diagnostic: DiagnosticState,
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            revision: self.revision,
            diagnostic_counter: self.diagnostic.counter,
        }
    }

    pub fn dispatch(&mut self, command: AppCommand) -> Result<AppSnapshot, AppError> {
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(AppError::RevisionOverflow)?;
        match command {
            AppCommand::IncrementDiagnostic => self
                .diagnostic
                .increment()
                .map_err(|CounterOverflow| AppError::CounterOverflow)?,
        }
        self.revision = next_revision;
        Ok(self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_returns_new_owned_snapshot() {
        let mut app = Application::new();
        let before = app.snapshot();
        let after = app.dispatch(AppCommand::IncrementDiagnostic).unwrap();
        assert_eq!(before.revision, 0);
        assert_eq!(before.diagnostic_counter, 0);
        assert_eq!(after.revision, 1);
        assert_eq!(after.diagnostic_counter, 1);
        assert_eq!(app.snapshot(), after);
    }

    #[test]
    fn overflow_does_not_change_revision() {
        let mut app = Application::new();
        app.diagnostic.counter = u64::MAX;
        assert_eq!(
            app.dispatch(AppCommand::IncrementDiagnostic),
            Err(AppError::CounterOverflow)
        );
        assert_eq!(app.revision, 0);
    }
}
