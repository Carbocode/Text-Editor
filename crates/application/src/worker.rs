//! The one debounced background worker used by every "latest query wins"
//! job: content search and the file name filter.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};

/// Typing within this interval replaces the pending job without running it.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(150);

/// Whether a job has been superseded by a newer one or cancelled.
#[derive(Clone)]
pub(crate) struct Cancellation {
    generation: Arc<AtomicU64>,
    expected: u64,
}

impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.generation.load(Ordering::Relaxed) != self.expected
    }

    pub fn generation(&self) -> u64 {
        self.expected
    }
}

pub(crate) struct DebouncedWorker<J> {
    jobs: Sender<(u64, J)>,
    generation: Arc<AtomicU64>,
}

impl<J: Send + 'static> DebouncedWorker<J> {
    /// Runs the latest job once no newer one arrived for [`DEBOUNCE`].
    /// `merge(old, newer)` combines a superseded job into its replacement.
    pub fn spawn(
        merge: impl Fn(J, J) -> J + Send + 'static,
        mut run: impl FnMut(J, &Cancellation) + Send + 'static,
    ) -> Self {
        let (jobs, receiver) = mpsc::channel::<(u64, J)>();
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&generation);
        thread::spawn(move || {
            while let Ok((mut expected, mut job)) = receiver.recv() {
                loop {
                    match receiver.recv_timeout(DEBOUNCE) {
                        Ok((newer_generation, newer)) => {
                            expected = newer_generation;
                            job = merge(job, newer);
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let cancellation = Cancellation {
                    generation: Arc::clone(&worker_generation),
                    expected,
                };
                run(job, &cancellation);
            }
        });
        Self { jobs, generation }
    }

    /// Supersedes every earlier job; returns the new job's generation.
    pub fn submit(&self, job: J) -> u64 {
        let generation = self.cancel();
        let _ = self.jobs.send((generation, job));
        generation
    }

    /// Cancels every submitted job; returns the new current generation.
    pub fn cancel(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn current(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
}
