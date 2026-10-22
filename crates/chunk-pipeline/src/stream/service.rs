//! Between-frames servicing: while the frame thread waits for its next update, a dedicated
//! thread owns the stream and runs the same poll in short slices, so decode commits and
//! light and mesh scheduling overlap that wait instead of the next frame's critical path.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Mutex,
    thread::JoinHandle,
};

use crossbeam_channel::Select;

use super::*;

/// Cooperative poll budget; a work item already running can exceed this slice.
const SERVICE_SLICE: Duration = Duration::from_micros(250);
/// Longest idle sleep between worker results. Work no result announces, such as an expired
/// neighbour deadline, waits for the next frame's poll, so idle runs rarely wake.
const SERVICE_IDLE_WAIT: Duration = Duration::from_millis(16);

impl WorldStream {
    /// Polls one slice and reports whether it found nothing to do, so the caller may sleep.
    fn service_slice(
        &mut self,
        camera_position: [f32; 3],
        max_mesh_jobs: usize,
        yield_request: &Arc<AtomicBool>,
    ) -> (WorldStreamPoll, bool) {
        self.service_yield = Some(Arc::clone(yield_request));
        self.frame_deadline = Some(Instant::now() + SERVICE_SLICE);
        let report = self.poll(camera_position, max_mesh_jobs);
        self.service_yield = None;
        // Commits, worker activity and queued publications all count as progress.
        // Only a slice that made none sleeps until a worker result can change that.
        let idle = !self.retention_due
            && !self.has_pending_full_detail()
            && report == WorldStreamPoll::default()
            && self.decode_rx.is_empty()
            && self.lighting.rx.is_empty();
        (report, idle)
    }

    /// Worker result channels an idle service sleeps on; mesh results only while none queue,
    /// since queued ones wait on capacity only the frame frees.
    fn idle_wait(&self) -> IdleWait {
        IdleWait {
            decode: self.decode_rx.clone(),
            light: self.lighting.rx.clone(),
            mesh: self.mesh_rx.is_empty().then(|| self.mesh_rx.clone()),
        }
    }
}

/// Clones of the lent stream's worker channels, so the service sleeps without holding it.
struct IdleWait {
    decode: Receiver<DecodeCompletion>,
    light: Receiver<LightCompletion>,
    mesh: Option<Receiver<MeshCompletion>>,
}

impl IdleWait {
    /// Blocks until a worker result or a wake is ready, or until `timeout`, consuming the wake.
    fn wait(&self, wake: &Receiver<()>, yield_requested: bool, timeout: Duration) {
        let mut select = Select::new();
        select.recv(&self.decode);
        select.recv(&self.light);
        if let Some(mesh) = &self.mesh {
            select.recv(mesh);
        }
        select.recv(wake);
        let ready = {
            #[cfg(feature = "tracy")]
            let _zone =
                tracing::info_span!("stream.service_wait", mesh = self.mesh.is_some()).entered();
            select.ready_timeout(timeout)
        };
        #[cfg(feature = "tracy")]
        {
            let source = match ready {
                Ok(0) => "decode",
                Ok(1) => "light",
                Ok(2) if self.mesh.is_some() => "mesh",
                Ok(_) => "reclaim",
                Err(_) => "timeout",
            };
            let _zone =
                tracing::info_span!("stream.service_woke", source, yield_requested).entered();
        }
        #[cfg(not(feature = "tracy"))]
        let _ = (ready, yield_requested);
        // A wake only asks for a state check; left queued, it would cut every later wait short.
        let _ = wake.try_recv();
    }
}

impl WorldStreamPoll {
    /// Adds another poll's work, e.g. a frame's poll to its between-frames service work.
    pub fn accumulate(&mut self, other: Self) {
        self.maintenance_items += other.maintenance_items;
        self.service_item_max = self.service_item_max.max(other.service_item_max);
        self.commit_steps += other.commit_steps;
        self.decoded_results += other.decoded_results;
        self.light_results += other.light_results;
        self.light_jobs_dispatched += other.light_jobs_dispatched;
        self.mesh_results += other.mesh_results;
        self.mesh_jobs_dispatched += other.mesh_jobs_dispatched;
        self.mesh_changes_queued += other.mesh_changes_queued;
    }
}

/// A stream lent to the service and the work done on it; either thread may hold it.
struct Lent {
    handoff_id: u64,
    stream: WorldStream,
    camera_position: [f32; 3],
    max_mesh_jobs: usize,
    report: WorldStreamPoll,
    busy: Duration,
    /// When the service first polled this loan; `None` while it has not run.
    serving_since: Option<Instant>,
}

#[derive(Default)]
struct Slot {
    lent: Option<Lent>,
    panic: Option<Box<dyn Any + Send>>,
    shutdown: bool,
}

/// State both threads lock. The service holds the lock only while it polls a slice, so a
/// reclaim never waits for an idle or descheduled service thread to wake up.
struct Shared {
    slot: Mutex<Slot>,
    yield_request: Arc<AtomicBool>,
    /// Lets tests keep the service thread from taking any turn.
    #[cfg(test)]
    turns: Mutex<()>,
}

impl Shared {
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A stream handed back by [`WorldStreamService::reclaim`], with the work done meanwhile.
pub struct ServicedStream {
    pub stream: WorldStream,
    /// Poll work performed on the service thread.
    pub report: WorldStreamPoll,
    /// Time the service thread spent polling, excluding idle waits for worker results.
    pub busy: Duration,
    /// Time from the service's first poll of this loan until the reclaim; zero if it never ran.
    pub held: Duration,
}

/// Owns one service thread that polls a lent [`WorldStream`] between frames.
///
/// The frame thread lends the stream with [`launch`](Self::launch) once nothing else in the
/// frame needs it, and takes it back with [`reclaim`](Self::reclaim) before anything does.
/// Reclaim waits only for a poll slice in progress; work already running can exceed its slice.
/// A panic on the service thread resumes on the reclaiming thread, as it would have had the
/// poll run there.
pub struct WorldStreamService {
    shared: Arc<Shared>,
    wake: Sender<()>,
    servicing: bool,
    handoff_id: u64,
    thread: Option<JoinHandle<()>>,
}

impl WorldStreamService {
    /// Starts the service thread, named `stream-service`.
    pub fn spawn() -> std::io::Result<Self> {
        // One-slot channel and a preallocated slot keep the hand-off allocation-free.
        let (wake, wake_rx) = bounded(1);
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            yield_request: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            turns: Mutex::new(()),
        });
        let thread = std::thread::Builder::new()
            .name("stream-service".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                move || serve(&shared, &wake_rx)
            })?;
        Ok(Self {
            shared,
            wake,
            servicing: false,
            handoff_id: 0,
            thread: Some(thread),
        })
    }

    /// Whether the service currently holds a stream.
    #[must_use]
    pub const fn is_servicing(&self) -> bool {
        self.servicing
    }

    /// Lends `stream` to the service thread, which polls it until the next reclaim.
    ///
    /// # Panics
    ///
    /// If the service already holds a stream.
    pub fn launch(
        &mut self,
        mut stream: WorldStream,
        camera_position: [f32; 3],
        max_mesh_jobs: usize,
    ) {
        assert!(!self.servicing, "the service already holds a stream");
        // Its next service run reaches any retention request before the frame's next poll.
        stream.between_frames_service = true;
        self.handoff_id = self.handoff_id.wrapping_add(1);
        #[cfg(feature = "tracy")]
        let _zone =
            tracing::info_span!("stream.service_launch", handoff_id = self.handoff_id).entered();
        {
            let mut slot = self.shared.slot();
            self.shared.yield_request.store(false, Ordering::Release);
            slot.lent = Some(Lent {
                handoff_id: self.handoff_id,
                stream,
                camera_position,
                max_mesh_jobs,
                report: WorldStreamPoll::default(),
                busy: Duration::ZERO,
                serving_since: None,
            });
        }
        let _ = self.wake.try_send(());
        self.servicing = true;
    }

    /// Asks the service to yield and takes the stream back; `None` when it holds none.
    ///
    /// # Panics
    ///
    /// Resumes a panic raised while servicing.
    pub fn reclaim(&mut self) -> Option<ServicedStream> {
        if !self.servicing {
            return None;
        }
        self.servicing = false;
        #[cfg(feature = "tracy")]
        let _zone =
            tracing::info_span!("stream.service_reclaim", handoff_id = self.handoff_id,).entered();
        self.shared.yield_request.store(true, Ordering::Release);
        let taken = {
            #[cfg(feature = "tracy")]
            let _zone =
                tracing::info_span!("stream.service_receive", handoff_id = self.handoff_id,)
                    .entered();
            let mut slot = self.shared.slot();
            if let Some(payload) = slot.panic.take() {
                drop(slot);
                resume_unwind(payload);
            }
            slot.lent.take()
        };
        let lent = taken.expect("a lent stream stays in the slot until reclaimed");
        let held = lent
            .serving_since
            .map_or(Duration::ZERO, |since| since.elapsed());
        let mut stream = lent.stream;
        stream.service_window = held;
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!(
            "stream.service_received",
            handoff_id = lent.handoff_id,
            service_item_max_ms = lent.report.service_item_max.as_secs_f64() * 1000.0,
            maintenance_items = lent.report.maintenance_items,
            commit_steps = lent.report.commit_steps,
        )
        .entered();
        #[cfg(not(feature = "tracy"))]
        let _ = lent.handoff_id;
        Some(ServicedStream {
            stream,
            report: lent.report,
            busy: lent.busy,
            held,
        })
    }
}

impl Drop for WorldStreamService {
    fn drop(&mut self) {
        self.shared.yield_request.store(true, Ordering::Release);
        // Waits for at most one slice in progress; a held stream drops with the service.
        let lent = {
            let mut slot = self.shared.slot();
            slot.shutdown = true;
            slot.lent.take()
        };
        drop(lent);
        let _ = self.wake.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Sleeps until woken, then polls whatever stream is lent until it is reclaimed.
fn serve(shared: &Shared, wake: &Receiver<()>) {
    while wake.recv().is_ok() {
        if service_lent(shared, wake) {
            return;
        }
    }
}

/// Polls the lent stream in slices until it is reclaimed or yield is requested; returns
/// whether the service is shutting down.
fn service_lent(shared: &Shared, wake: &Receiver<()>) -> bool {
    loop {
        #[cfg(test)]
        let _turn = shared
            .turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut slot = shared.slot();
        if slot.shutdown {
            return true;
        }
        if shared.yield_request.load(Ordering::Acquire) {
            return false;
        }
        let Some(lent) = slot.lent.as_mut() else {
            return false;
        };
        #[cfg(feature = "tracy")]
        let _zone =
            tracing::info_span!("stream.service_run", handoff_id = lent.handoff_id).entered();
        let started = Instant::now();
        lent.serving_since.get_or_insert(started);
        let polled = catch_unwind(AssertUnwindSafe(|| {
            lent.stream.service_slice(
                lent.camera_position,
                lent.max_mesh_jobs,
                &shared.yield_request,
            )
        }));
        match polled {
            Ok((report, idle)) => {
                lent.busy += started.elapsed();
                lent.report.accumulate(report);
                if idle {
                    let wait = lent.stream.idle_wait();
                    drop(slot);
                    wait.wait(
                        wake,
                        shared.yield_request.load(Ordering::Acquire),
                        SERVICE_IDLE_WAIT,
                    );
                }
            }
            Err(payload) => {
                // The stream may be inconsistent; it drops here and the panic resumes on reclaim.
                slot.lent = None;
                slot.panic = Some(payload);
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "tracy"))]
mod trace_tests;
