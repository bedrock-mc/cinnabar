//! Completion accounting for existing application-owned OS workers.

use tokio_util::task::{TaskTracker, task_tracker::TaskTrackerToken};

/// Arms completion before spawn and retains it through the body's captures, return or unwind.
pub(crate) fn tracked<F: FnOnce()>(work: F, tasks: &TaskTracker) -> impl FnOnce() + use<F> {
    let worker = Worker {
        work: Some(work),
        _completion: tasks.token(),
    };
    move || worker.run()
}

// Field order also destroys an unstarted body before its token when thread creation fails.
struct Worker<F> {
    work: Option<F>,
    _completion: TaskTrackerToken,
}

impl<F: FnOnce()> Worker<F> {
    /// Consumes the actual body before the outer completion owner can retire.
    fn run(mut self) {
        self.work.take().expect("one worker invocation")();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        sync::mpsc,
        task::{Context, Poll, Waker},
        time::Duration,
    };

    /// Holds the actual consumed worker capture inside its destructor until the fixture releases it.
    struct Cleanup {
        dropping: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.dropping.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }

    #[test]
    fn completion_waits_for_the_actual_worker_capture_destructor() {
        let tasks = TaskTracker::new();
        let (dropping, observed) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let cleanup = Cleanup {
            dropping,
            release: blocked,
        };
        let work = tracked(move || drop(cleanup), &tasks);
        tasks.close();
        let worker = std::thread::spawn(work);
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut wait = std::pin::pin!(tasks.wait());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(wait.as_mut().poll(&mut context), Poll::Pending));
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(matches!(wait.as_mut().poll(&mut context), Poll::Ready(())));
    }

    /// Detects token retirement before a failed-to-start body releases its owned state.
    struct Unstarted(TaskTracker);
    impl Drop for Unstarted {
        fn drop(&mut self) {
            assert!(
                !self.0.is_empty(),
                "the actual body must drop before its completion token"
            );
        }
    }

    #[test]
    fn dropping_an_unstarted_worker_destroys_its_body_before_completion() {
        let tasks = TaskTracker::new();
        let owned = Unstarted(tasks.clone());
        let work = tracked(move || drop(owned), &tasks);
        assert!(!tasks.is_empty());
        drop(work);
        assert!(tasks.is_empty());
    }
}
