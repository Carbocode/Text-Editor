//! Platform independent domain values.

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
}
