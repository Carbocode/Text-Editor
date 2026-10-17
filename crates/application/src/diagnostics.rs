//! This adapter compiles to no-ops without the `devtools` feature.
#[cfg(feature = "devtools")]
use native_workspace_devtools_producer::{
    EventKind as Kind, EventOutcome as ProducerOutcome, Producer,
};

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
    /// Starts a span; [`Span::end`] records its outcome.
    pub fn span(&self, name: &'static str, lane: &'static str) -> Span<'_> {
        #[cfg(feature = "devtools")]
        let id = self
            .producer
            .as_ref()
            .filter(|p| p.enabled())
            .map_or(0, |p| {
                let id = p.id();
                p.emit(
                    "core",
                    lane,
                    name,
                    Kind::SpanStart,
                    ProducerOutcome::Ok,
                    self.operation,
                    id,
                    0,
                    0,
                    self.generation,
                );
                id
            });
        #[cfg(not(feature = "devtools"))]
        let id = 0;
        Span {
            context: self,
            name,
            lane,
            id,
        }
    }
    pub fn error(&self, name: &'static str, lane: &'static str, revision: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                "core",
                lane,
                name,
                Kind::Event,
                ProducerOutcome::Error,
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
                ProducerOutcome::Ok,
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
    pub fn ui_heartbeat(&self) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.ui_heartbeat(50_000_000);
        }
    }
    pub(crate) fn ui_delay(&self) -> u64 {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            return p.recent_ui_delay(50_000_000);
        }
        0
    }
    pub(crate) fn context(&self, operation: u64) -> Context {
        Context {
            operation,
            generation: 0,
            #[cfg(feature = "devtools")]
            producer: self.producer.clone(),
        }
    }

    pub(crate) fn id(&self) -> u64 {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            return p.id();
        }
        0
    }

    pub(crate) fn drop_record(
        &self,
        operation: u64,
        span: u64,
        name: &'static str,
        end: bool,
        success: bool,
        duration: u64,
    ) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.producer {
            p.emit(
                if name == "drop.filesystem" {
                    "core"
                } else {
                    "frontend"
                },
                if name == "drop.filesystem" {
                    "filesystem-worker"
                } else {
                    "ui"
                },
                name,
                if span == 0 {
                    Kind::Event
                } else if end {
                    Kind::SpanEnd
                } else {
                    Kind::SpanStart
                },
                if success {
                    ProducerOutcome::Ok
                } else {
                    ProducerOutcome::Error
                },
                operation,
                span,
                0,
                duration,
                0,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (operation, span, name, end, success, duration);
    }
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
                ProducerOutcome::Ok,
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

/// How a diagnostic span ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Ok,
    Noop,
    Error,
    Cancelled,
}

/// An open diagnostic span of a [`Context`].
#[must_use = "a span records nothing until it ends"]
pub(crate) struct Span<'a> {
    context: &'a Context,
    name: &'static str,
    lane: &'static str,
    id: u64,
}

impl Span<'_> {
    pub fn end(self, outcome: Outcome, revision: u64, value: u64) {
        #[cfg(feature = "devtools")]
        if let Some(p) = &self.context.producer
            && self.id != 0
        {
            p.emit(
                "core",
                self.lane,
                self.name,
                Kind::SpanEnd,
                match outcome {
                    Outcome::Ok => ProducerOutcome::Ok,
                    Outcome::Noop => ProducerOutcome::Noop,
                    Outcome::Error => ProducerOutcome::Error,
                    Outcome::Cancelled => ProducerOutcome::Cancelled,
                },
                self.context.operation,
                self.id,
                revision,
                value,
                self.context.generation,
            );
        }
        #[cfg(not(feature = "devtools"))]
        let _ = (
            self.context,
            self.name,
            self.lane,
            self.id,
            outcome,
            revision,
            value,
        );
    }
}
