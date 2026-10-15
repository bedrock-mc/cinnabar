//! Queue ownership intervals exclude acquisition and condition-variable sleep.

use super::*;
use std::ops::{Deref, DerefMut};

pub(super) struct QueueGuard<'a> {
    guard: Option<MutexGuard<'a, Queues>>,
    owned: Option<tracing::span::EnteredSpan>,
    #[cfg(not(target_arch = "wasm32"))]
    wait: Option<tracing::Span>,
}

impl<'a> QueueGuard<'a> {
    /// Starts ownership only after the mutex has been acquired.
    pub(super) fn new(
        guard: MutexGuard<'a, Queues>,
        owned: tracing::Span,
        #[cfg(not(target_arch = "wasm32"))] wait: tracing::Span,
    ) -> Self {
        Self {
            guard: Some(guard),
            owned: Some(owned.entered()),
            #[cfg(not(target_arch = "wasm32"))]
            wait: Some(wait),
        }
    }

    /// Ends ownership before the condition variable releases and reacquires the mutex.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn end_ownership(
        mut self,
    ) -> (MutexGuard<'a, Queues>, tracing::Span, tracing::Span) {
        let owned = self.owned.take().expect("ownership is traced").exit();
        (
            self.guard.take().expect("the queue is held"),
            owned,
            self.wait.take().expect("the worker wait is prepared"),
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::stream::trace_test_support::Capture;

    /// Ownership starts after acquisition and ends before waiting, including spurious wakes.
    #[test]
    fn queue_ownership_excludes_acquisition_and_condvar_sleep() {
        let shared = Arc::new(Shared::default());
        let metadata_under_lock = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let capture = Capture::default().before_new_span({
            let shared = Arc::clone(&shared);
            let metadata_under_lock = Arc::clone(&metadata_under_lock);
            move |metadata| {
                if matches!(metadata.name(), "stream.queue_owned" | "stream.worker_wait")
                    && shared.queues.try_lock().is_err()
                {
                    metadata_under_lock.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });
        let (started, waiting) = crossbeam_channel::bounded(1);
        let worker = std::thread::spawn({
            let shared = Arc::clone(&shared);
            let capture = capture.clone();
            move || {
                capture.with(|| {
                    let mut queues = shared.lock();
                    started.send(()).unwrap();
                    while !queues.shutdown {
                        queues = shared.wait(queues);
                    }
                })
            }
        });
        waiting.recv().unwrap();
        // Acquiring this lock proves the worker released it atomically for its wait.
        shared.lock().shutdown = true;
        shared.ready.notify_one();
        worker.join().unwrap();
        assert!(shared.queues.try_lock().is_ok());
        assert_eq!(
            metadata_under_lock.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "subscriber metadata allocation must happen before queue acquisition",
        );

        let (mut owned, mut acquiring, mut sleeping) = (false, false, false);
        let mut ownerships = 0;
        for observation in capture.observations() {
            match observation.name {
                "stream.queue_lock" => {
                    assert!(!owned && !sleeping);
                    acquiring = observation.entered;
                }
                "stream.queue_owned" => {
                    assert!(!acquiring && !sleeping);
                    assert_ne!(owned, observation.entered);
                    owned = observation.entered;
                    ownerships += usize::from(owned);
                }
                "stream.worker_wait" => {
                    assert!(!owned && !acquiring);
                    sleeping = observation.entered;
                }
                _ => {}
            }
        }
        assert!(!owned && !acquiring && !sleeping);
        assert!(ownerships >= 2, "ownership is observed again after waking");
    }
}

impl Deref for QueueGuard<'_> {
    type Target = Queues;

    /// Reads the queue while this guard owns its mutex.
    fn deref(&self) -> &Self::Target {
        self.guard.as_deref().expect("the queue is held")
    }
}

impl DerefMut for QueueGuard<'_> {
    /// Mutates the queue while this guard owns its mutex.
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_deref_mut().expect("the queue is held")
    }
}

impl Drop for QueueGuard<'_> {
    /// Releases the mutex before tracing the end of ownership.
    fn drop(&mut self) {
        drop(self.guard.take());
    }
}
