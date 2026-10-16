//! Deterministic thread-start failures with a local, pipe-controlled helper process.

use super::*;
use crate::media::worker::lifecycle::Task;
use std::io;

struct FaultThreads {
    fail: &'static str,
    before_failure: Box<dyn Fn() + Send + Sync>,
}

impl Threads for FaultThreads {
    fn spawn(&self, name: &'static str, task: Task) -> io::Result<()> {
        if name == self.fail {
            // A failed OS spawn also destroys the task and its captured owners.
            drop(task);
            (self.before_failure)();
            Err(io::Error::other("injected thread creation failure"))
        } else {
            SystemThreads.spawn(name, task)
        }
    }
}

#[derive(Default)]
struct SourceState {
    dropped: AtomicBool,
    leased_on_drop: AtomicBool,
}

struct Source {
    state: Arc<SourceState>,
    entered: Option<mpsc::Sender<()>>,
    release: Option<mpsc::Receiver<()>>,
}

impl Source {
    /// Records source destruction independently of the helper process and decoder lease.
    fn new(state: &Arc<SourceState>) -> Self {
        Self {
            state: Arc::clone(state),
            entered: None,
            release: None,
        }
    }

    /// Holds a request until its test releases it, even after cancellation kills the helper.
    fn blocked(
        state: &Arc<SourceState>,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            state: Arc::clone(state),
            entered: Some(entered),
            release: Some(release),
        }
    }
}

impl ChunkSource for Source {
    fn load(&mut self, _index: usize, _start: u64, _length: u64) -> Result<Vec<u8>> {
        if let Some(entered) = self.entered.take() {
            entered.send(())?;
        }
        if let Some(release) = self.release.take() {
            release.recv()?;
        }
        Ok(vec![1])
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.state
            .leased_on_drop
            .store(!DecoderLease::free(), Ordering::Release);
        self.state.dropped.store(true, Ordering::Release);
    }
}

/// Starts only our shell helper; it requests chunk zero then waits for pipe closure or termination.
fn starting() -> (Starting, SharedChild, Arc<AtomicBool>, u32) {
    let mut request = Vec::new();
    ipc::write_reply(&mut request, &Reply::Need(0)).unwrap();
    let escaped: String = request.iter().map(|byte| format!("\\{byte:03o}")).collect();
    let script = format!("printf '{escaped}'; while read -r ignored; do :; done");
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let starting = Starting::spawn(command, Arc::clone(&cancelled)).unwrap();
    let child = Arc::clone(&starting.child);
    let pid = lock_child(&child).as_ref().unwrap().id();
    (starting, child, cancelled, pid)
}

/// Checks both Rust ownership and the OS wait contract for the helper created by this test.
fn assert_reaped(child: &SharedChild, pid: u32) {
    assert!(lock_child(child).is_none(), "a child wait owner remains");
    let mut status = 0;
    // SAFETY: pid identifies only this test's child, and status is writable for the call.
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

/// Waits for the source and the IPC-held decoder lease to leave together.
fn assert_source_released(state: &SourceState) {
    test_time::eventually("the IPC source and decoder slot to be released", || {
        state.dropped.load(Ordering::Acquire) && DecoderLease::free()
    });
    assert!(state.leased_on_drop.load(Ordering::Acquire));
}

#[test]
fn ipc_spawn_failure_reaps_before_releasing_the_startup_lease() {
    let _slot = DECODER_SLOT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (starting, child, cancelled, pid) = starting();
    let state = Arc::new(SourceState::default());
    let result = Worker::start_threads(
        starting,
        descriptor_for(&[1]),
        Source::new(&state),
        1,
        0,
        Arc::new(FaultThreads {
            fail: IPC_THREAD,
            before_failure: Box::new(|| {
                assert!(
                    !DecoderLease::free(),
                    "spawn failure released the live helper's slot"
                );
            }),
        }),
    );
    assert!(result.is_err());
    assert!(cancelled.load(Ordering::Acquire));
    assert_reaped(&child, pid);
    assert_source_released(&state);
}

#[test]
fn watchdog_spawn_failure_cancels_and_reaps_but_retains_the_ipc_source_and_lease() {
    let _slot = DECODER_SLOT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (starting, child, cancelled, pid) = starting();
    let state = Arc::new(SourceState::default());
    let (entered_sender, entered) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel();
    let entered = Mutex::new(entered);
    let source = Source::blocked(&state, entered_sender, release_receiver);
    let result = Worker::start_threads(
        starting,
        descriptor_for(&[1]),
        source,
        1,
        0,
        Arc::new(FaultThreads {
            fail: WATCHDOG_THREAD,
            before_failure: Box::new(move || {
                entered
                    .lock()
                    .unwrap()
                    .recv_timeout(test_time::DEFAULT_TIMEOUT)
                    .unwrap();
            }),
        }),
    );
    assert!(result.is_err());
    assert!(cancelled.load(Ordering::Acquire));
    assert_reaped(&child, pid);
    assert!(!state.dropped.load(Ordering::Acquire));
    assert!(!DecoderLease::free());
    release.send(()).unwrap();
    assert_source_released(&state);
}

#[test]
fn reaper_spawn_failure_retains_the_wait_owner_for_synchronous_cleanup() {
    let _slot = DECODER_SLOT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (starting, child, cancelled, pid) = starting();
    let state = Arc::new(SourceState::default());
    let (entered_sender, entered) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel();
    let source = Source::blocked(&state, entered_sender, release_receiver);
    let retained = Arc::clone(&child);
    let worker = Worker::start_threads(
        starting,
        descriptor_for(&[1]),
        source,
        1,
        0,
        Arc::new(FaultThreads {
            fail: REAPER_THREAD,
            before_failure: Box::new(move || {
                assert!(
                    lock_child(&retained).is_some(),
                    "failed spawn lost the wait owner"
                );
            }),
        }),
    )
    .unwrap();
    entered.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
    drop(worker);
    assert!(cancelled.load(Ordering::Acquire));
    assert_reaped(&child, pid);
    assert!(!state.dropped.load(Ordering::Acquire));
    assert!(!DecoderLease::free());
    release.send(()).unwrap();
    assert_source_released(&state);
}
