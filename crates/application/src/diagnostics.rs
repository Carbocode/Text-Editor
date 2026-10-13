//! This adapter compiles to no-ops without the `devtools` feature.
#[cfg(feature = "devtools")]
use native_workspace_devtools_producer::{EventKind as Kind, EventOutcome as Outcome, Producer};

#[derive(Default)]
pub(crate) struct Diagnostics {
    #[cfg(feature = "devtools")]
    initialized: bool,
    #[cfg(feature = "devtools")]
    producer: Option<Producer>,
}
#[derive(Clone, Default)]
pub(crate) struct Context {
    pub operation: u64,
    pub generation: u64,
    #[cfg(feature = "devtools")]
    producer: Option<Producer>,
}
impl Diagnostics {
    pub fn enable(&mut self, frontend: &'static str) {
        #[cfg(feature = "devtools")]
        {
            if !self.initialized {
                self.initialized = true;
                self.producer = Producer::from_env(frontend);
            }
        }
        #[cfg(not(feature = "devtools"))]
        let _ = frontend;
    }
    pub fn start(&mut self, frontend: &'static str) {
        #[cfg(feature = "devtools")]
        if self.producer.is_none() {
            self.initialized = true;
            self.producer = Producer::start(frontend);
        }
        #[cfg(not(feature = "devtools"))]
        let _ = frontend;
    }
    pub fn context(&self) -> Context {
        #[cfg(feature = "devtools")]
        {
            if let Some(p) = &self.producer
                && p.enabled()
            {
                return Context {
                    operation: p.id(),
                    generation: 0,
                    producer: Some(p.clone()),
                };
            }
        }
        Context::default()
    }
    pub fn observer(&self) -> DiagnosticObserver {
        DiagnosticObserver {
            #[cfg(feature = "devtools")]
            producer: self.producer.clone(),
        }
    }
    pub fn snapshot_applied(&self, operation: u64, revision: u64, duration: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                "frontend",
                "ui",
                "snapshot.apply",
                Kind::Event,
                Outcome::Ok,
                operation,
                0,
                revision,
                duration,
                0,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (operation, revision, duration);
    }
    pub fn enabled(&self) -> bool {
        #[cfg(feature = "devtools")]
        {
            self.producer.as_ref().is_some_and(Producer::enabled)
        }
        #[cfg(not(feature = "devtools"))]
        {
            false
        }
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.stop();
        }
    }
}
impl Context {
    pub fn begin(&self, name: &'static str, lane: &'static str) -> u64 {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer
            && p.enabled()
        {
            let id = p.id();
            p.emit(
                "core",
                lane,
                name,
                Kind::SpanStart,
                Outcome::Ok,
                self.operation,
                id,
                0,
                0,
                self.generation,
            );
            return id;
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (name, lane);
        0
    }
    pub fn end(
        &self,
        name: &'static str,
        lane: &'static str,
        span: u64,
        outcome: &'static str,
        revision: u64,
        value: u64,
    ) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer
            && span != 0
        {
            let outcome = match outcome {
                "error" => Outcome::Error,
                "noop" => Outcome::Noop,
                "cancelled" => Outcome::Cancelled,
                "discarded" => Outcome::Discarded,
                _ => Outcome::Ok,
            };
            p.emit(
                "core",
                lane,
                name,
                Kind::SpanEnd,
                outcome,
                self.operation,
                span,
                revision,
                value,
                self.generation,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (name, lane, span, outcome, revision, value);
    }
    pub fn error(&self, name: &'static str, lane: &'static str, revision: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                "core",
                lane,
                name,
                Kind::Event,
                Outcome::Error,
                self.operation,
                0,
                revision,
                0,
                self.generation,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (name, lane, revision);
    }
    pub fn event(&self, name: &'static str, lane: &'static str, revision: u64, value: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                "core",
                lane,
                name,
                Kind::Event,
                Outcome::Ok,
                self.operation,
                0,
                revision,
                value,
                self.generation,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (name, lane, revision, value);
    }
}

/// Cloneable observer for frontend hooks, independent of the application's mutex.
#[derive(Clone, Default)]
pub struct DiagnosticObserver {
    #[cfg(feature = "devtools")]
    producer: Option<Producer>,
}
impl DiagnosticObserver {
    pub fn available(&self) -> bool {
        #[cfg(feature = "devtools")]
        {
            self.producer.is_some()
        }
        #[cfg(not(feature = "devtools"))]
        {
            false
        }
    }
    pub fn open_console(&self) {
        #[cfg(feature = "devtools")]
        if self.available() {
            native_workspace_devtools_producer::open_console();
        }
    }
    pub fn enabled(&self) -> bool {
        #[cfg(feature = "devtools")]
        {
            self.producer.as_ref().is_some_and(Producer::enabled)
        }
        #[cfg(not(feature = "devtools"))]
        {
            false
        }
    }
    pub fn snapshot_applied(&self, operation: u64, revision: u64, duration: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                "frontend",
                "ui",
                "snapshot.apply",
                Kind::Event,
                Outcome::Ok,
                operation,
                0,
                revision,
                duration,
                0,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (operation, revision, duration);
    }
}
