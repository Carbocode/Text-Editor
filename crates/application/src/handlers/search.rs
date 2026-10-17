//! Search queries and streamed results.

use crate::{AppEvent, Application};

impl Application {
    /// Typing only records the query; the worker debounces and searches.
    pub(super) fn queue_search_query(&self, query: String) -> Vec<AppEvent> {
        if self.state().search_query == query {
            Vec::new()
        } else {
            vec![AppEvent::SearchQueryQueued { query }]
        }
    }

    /// Streamed batches are merged into one event per poll.
    pub(super) fn poll_search_results(&mut self) -> Vec<AppEvent> {
        match self.search.collect(self.state().revision) {
            Some((event, context)) => {
                self.search_delivered(context);
                vec![event]
            }
            None => Vec::new(),
        }
    }
}
