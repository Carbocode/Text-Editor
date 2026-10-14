//! A drop operation shared by native UI hooks and filesystem workers. No paths
//! are recorded, and the disabled path neither timestamps nor touches a queue.
use crate::{DiagnosticObserver, diagnostics::Context};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Default)]
pub struct SidebarDropTrace(Option<Arc<Timing>>);
struct Timing {
    observer: DiagnosticObserver,
    operation: u64,
    started: Instant,
    spans: [u64; 5],
    completed: AtomicU8,
    filesystem_span: AtomicU64,
    filesystem_started: std::sync::OnceLock<Instant>,
}
impl SidebarDropTrace {
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }
    /// 0: drop callback; 1: AppKit prepare hook; 2: pointer release handler.
    pub fn begin(observer: DiagnosticObserver, origin: u8) -> Self {
        if !observer.enabled() {
            return Self::default();
        }
        let operation = observer.id();
        let spans = [
            observer.id(),
            observer.id(),
            observer.id(),
            observer.id(),
            observer.id(),
        ];
        let started = Instant::now();
        observer.drop_record(
            operation,
            0,
            match origin {
                1 => "drop.begin.prepare",
                2 => "drop.begin.release",
                _ => "drop.begin.callback",
            },
            false,
            true,
            0,
        );
        observer.drop_record(
            operation,
            0,
            "drop.ui_delay",
            true,
            true,
            observer.ui_delay(),
        );
        for (name, span) in [
            "drop.handler",
            "drop.sidebar_ready",
            "drop.confirmation",
            "drop.preview_hidden",
            "drop.dispatch_wait",
        ]
        .into_iter()
        .zip(spans)
        {
            observer.drop_record(operation, span, name, false, true, 0);
        }
        Self(Some(Arc::new(Timing {
            observer,
            operation,
            started,
            spans,
            completed: AtomicU8::new(0),
            filesystem_span: AtomicU64::new(0),
            filesystem_started: std::sync::OnceLock::new(),
        })))
    }
    pub(crate) fn context(&self) -> Option<Context> {
        self.0.as_ref().map(|t| t.observer.context(t.operation))
    }
    fn mark(&self, bit: u8, name: &'static str, span: Option<usize>, success: bool) {
        let Some(t) = &self.0 else { return };
        if t.completed.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
            return;
        }
        t.observer.drop_record(
            t.operation,
            span.map_or(0, |i| t.spans[i]),
            name,
            true,
            success,
            t.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
    }
    pub fn preview_hidden(&self) {
        self.mark(1, "drop.preview_hidden", Some(3), true);
    }
    pub fn handler_finished(&self, accepted: bool) {
        self.mark(2, "drop.handler", Some(0), accepted);
        if !accepted {
            self.mark(32, "drop.dispatch_wait", Some(4), false);
            self.sidebar_finished(false);
            self.confirmed(false);
        }
    }
    fn sidebar_finished(&self, success: bool) {
        self.mark(4, "drop.sidebar_ready", Some(1), success);
    }
    /// The native sidebar model/layout is ready; this is not display presentation.
    pub fn sidebar_ready(&self) {
        self.sidebar_finished(true);
    }
    pub fn confirmed(&self, success: bool) {
        self.mark(8, "drop.confirmation", Some(2), success);
        if !success {
            self.sidebar_finished(false);
        }
    }
    pub fn filesystem_started(&self) {
        let Some(t) = &self.0 else { return };
        if t.filesystem_started.set(Instant::now()).is_err() {
            return;
        }
        let span = t.observer.id();
        t.filesystem_span.store(span, Ordering::Relaxed);
        self.mark(32, "drop.dispatch_wait", Some(4), true);
        // Keep the span used for both endpoints under the same operation.
        t.observer
            .drop_record(t.operation, span, "drop.filesystem", false, true, 0);
    }
    pub fn filesystem_finished(&self, success: bool) {
        let Some(t) = &self.0 else { return };
        let Some(started) = t.filesystem_started.get() else {
            return;
        };
        if t.completed.fetch_or(16, Ordering::Relaxed) & 16 != 0 {
            return;
        }
        t.observer.drop_record(
            t.operation,
            t.filesystem_span.load(Ordering::Relaxed),
            "drop.filesystem",
            true,
            success,
            started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
    }
}
