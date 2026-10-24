//! Final process cancellation at the original account event owner.

use super::*;
use bridge::tasks::{NativeExecution, StopAuthority};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc as sync,
    },
    task::{Context, Poll, Waker},
};
use tokio_util::task::TaskTracker;

#[derive(Debug, PartialEq, Eq)]
enum Observation {
    Dispatched,
    StillHeld,
    Dropped,
    Returned,
}

/// A dispatched service future that completes only through its explicit fixture release.
pub(in crate::launcher_account) struct PendingPresence {
    observations: sync::Sender<Observation>,
    probes: Mutex<Option<mpsc::Receiver<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    dropped: AtomicBool,
    removals: AtomicUsize,
}

struct PendingUpdate<'a>(&'a PendingPresence);

impl Drop for PendingUpdate<'_> {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::Release);
        let _ = self.0.observations.send(Observation::Dropped);
    }
}

impl Service for PendingPresence {
    type Scope = ();

    async fn bind(&self) -> io::Result<()> {
        Ok(())
    }

    async fn update(
        &self,
        _: &(),
        _: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        let _actual_future = PendingUpdate(self);
        let mut probes = self.probes.lock().unwrap().take().unwrap();
        let mut release = self.release.lock().unwrap().take().unwrap();
        *dispatched = true;
        self.observations.send(Observation::Dispatched).unwrap();
        loop {
            tokio::select! {
                _ = &mut release => return Ok(None),
                probe = probes.recv() => {
                    let Some(()) = probe else { return Err(io::Error::other("fixture ended")); };
                    self.observations.send(Observation::StillHeld).unwrap();
                }
            }
        }
    }

    async fn remove(&self, _: &()) -> io::Result<()> {
        self.removals.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

struct Harness {
    runtime: tokio::runtime::Runtime,
    execution: NativeExecution,
    stop: StopAuthority,
    tasks: TaskTracker,
    lifetime: TitlePresenceLifetime,
    service: Arc<PendingPresence>,
    observations: sync::Receiver<Observation>,
    probes: mpsc::Sender<()>,
    release: Mutex<Option<oneshot::Sender<()>>>,
}

impl Harness {
    /// Pauses service budgets so only explicit owner polls decide every outcome.
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        let tasks = TaskTracker::new();
        let (execution, stop) =
            NativeExecution::for_process(runtime.handle().clone(), tasks.clone());
        let (observed, observations) = sync::channel();
        let (probes, received) = mpsc::channel(1);
        let (release, released) = oneshot::channel();
        Self {
            runtime,
            execution,
            stop,
            tasks,
            lifetime: TitlePresenceLifetime::new(),
            service: Arc::new(PendingPresence {
                observations: observed,
                probes: Mutex::new(Some(received)),
                release: Mutex::new(Some(released)),
                dropped: AtomicBool::new(false),
                removals: AtomicUsize::new(0),
            }),
            observations,
            probes,
            release: Mutex::new(Some(release)),
        }
    }

    /// Owns the real joined service futures and WorkerGuard without a competing OS-thread poll.
    fn owner(&self) -> impl Future<Output = ()> + use<> {
        let shared = Mutex::new(Snapshot {
            pending_presence_probe: Some(Arc::clone(&self.service)),
            ..Snapshot::default()
        });
        let (requests, received) = bounded(1);
        let lifetime = self.lifetime.clone();
        let execution = self.execution.clone();
        let observations = self.service.observations.clone();
        async move {
            let _requests = requests;
            events::tests::run_owner(&shared, &received, &lifetime, execution.stopped()).await;
            observations.send(Observation::Returned).unwrap();
        }
    }

    /// Reads only observations produced by the immediately preceding synchronous owner poll.
    fn observed(&self) -> Observation {
        self.observations
            .try_recv()
            .expect("the explicit owner poll did not produce its fixture observation")
    }

    /// Releases an unfinished control or RED owner before checking its captured terminal oracle.
    fn finish(&self, owner: Pin<&mut impl Future<Output = ()>>, ready: bool) {
        if !ready {
            self.lifetime.retire();
            let _ = self.release.lock().unwrap().take().unwrap().send(());
            assert_eq!(poll(owner), Poll::Ready(()));
        }
    }

    /// Tracker readiness follows destruction of its actual retained future, never a detached token.
    fn assert_retired(&self) {
        self.tasks.close();
        let mut wait = std::pin::pin!(self.tasks.wait());
        assert_eq!(poll(wait.as_mut()), Poll::Ready(()));
        assert!(self.tasks.is_empty());
        assert!(self.lifetime.worker_finished());
    }
}

/// Polls the real owner once, with all process-stop transitions controlled between polls.
fn poll(future: Pin<&mut impl Future<Output = ()>>) -> Poll<()> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn ordinary_retirement_keeps_the_dispatched_presence_until_completion() {
    let harness = Harness::new();
    let _runtime = harness.runtime.enter();
    let mut owner = Box::pin(harness.tasks.track_future(harness.owner()));
    assert_eq!(poll(owner.as_mut()), Poll::Pending);
    assert_eq!(harness.observed(), Observation::Dispatched);
    harness.lifetime.retire();
    harness.probes.try_send(()).unwrap();
    assert_eq!(poll(owner.as_mut()), Poll::Pending);
    assert_eq!(harness.observed(), Observation::StillHeld);
    assert!(!harness.service.dropped.load(Ordering::Acquire));
    assert_eq!(harness.lifetime.retirement(), None);
    assert!(!harness.lifetime.worker_finished());
    assert!(!harness.tasks.is_empty());
    harness.finish(owner.as_mut(), false);
    drop(owner);
    harness.assert_retired();
    assert_eq!(harness.service.removals.load(Ordering::Acquire), 1);
    assert_eq!(
        harness.lifetime.retirement(),
        Some(TitlePresenceRetirement::Complete)
    );
}

#[test]
fn final_process_stop_drops_dispatched_presence_and_retires_the_actual_worker() {
    let harness = Harness::new();
    let _runtime = harness.runtime.enter();
    let mut owner = Box::pin(harness.tasks.track_future(harness.owner()));
    assert_eq!(poll(owner.as_mut()), Poll::Pending);
    assert_eq!(harness.observed(), Observation::Dispatched);
    harness.lifetime.retire();
    harness.probes.try_send(()).unwrap();
    assert_eq!(poll(owner.as_mut()), Poll::Pending);
    assert_eq!(harness.observed(), Observation::StillHeld);
    harness.tasks.close();
    let mut wait = std::pin::pin!(harness.tasks.wait());
    assert_eq!(poll(wait.as_mut()), Poll::Pending);
    harness.stop.stop();
    let after_stop = poll(owner.as_mut());
    let dropped_before_release = harness.service.dropped.load(Ordering::Acquire);
    let retirement_before_release = harness.lifetime.retirement();
    let worker_finished_before_release = harness.lifetime.worker_finished();
    harness.finish(owner.as_mut(), after_stop.is_ready());
    drop(owner);
    harness.assert_retired();
    assert_eq!(after_stop, Poll::Ready(()));
    assert!(dropped_before_release);
    assert!(worker_finished_before_release);
    assert_eq!(harness.service.removals.load(Ordering::Acquire), 0);
    assert_eq!(
        retirement_before_release,
        Some(TitlePresenceRetirement::Unconfirmed)
    );
}

#[test]
fn final_process_stop_is_sticky_before_the_account_worker_starts() {
    let harness = Harness::new();
    let _runtime = harness.runtime.enter();
    harness.tasks.close();
    harness.stop.stop();
    let mut owner = Box::pin(harness.tasks.track_future(harness.owner()));
    let first_poll = poll(owner.as_mut());
    let first = harness.observed();
    let retirement_before_release = harness.lifetime.retirement();
    harness.finish(owner.as_mut(), first_poll.is_ready());
    drop(owner);
    harness.assert_retired();
    assert_eq!(first_poll, Poll::Ready(()));
    assert_eq!(first, Observation::Returned);
    assert!(!harness.service.dropped.load(Ordering::Acquire));
    assert_eq!(harness.service.removals.load(Ordering::Acquire), 0);
    assert_eq!(
        retirement_before_release,
        Some(TitlePresenceRetirement::Unconfirmed)
    );
}
