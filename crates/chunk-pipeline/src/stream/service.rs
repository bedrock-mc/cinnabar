//! Between-frames servicing: while the frame thread waits for its next update, a dedicated
//! thread owns the stream and runs the same poll in short slices, so decode commits and
//! light and mesh scheduling overlap that wait instead of the next frame's critical path.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
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
    /// Polls in slices until `yield_request` is raised, sleeping on worker results whenever a
    /// slice finds nothing to do. Returns the work done and the time spent polling.
    fn service_until_yield(
        &mut self,
        camera_position: [f32; 3],
        max_mesh_jobs: usize,
        yield_request: &Arc<AtomicBool>,
        wake: &Receiver<()>,
    ) -> (WorldStreamPoll, Duration) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.service").entered();
        self.service_yield = Some(Arc::clone(yield_request));
        let mut total = WorldStreamPoll::default();
        let mut busy = Duration::ZERO;
        while !yield_request.load(Ordering::Acquire) {
            let started = Instant::now();
            self.frame_deadline = Some(started + SERVICE_SLICE);
            let report = self.poll(camera_position, max_mesh_jobs);
            busy += started.elapsed();
            total.accumulate(report);
            // Commits, worker activity and queued publications all count as progress.
            // Only a slice that made none sleeps until a worker result can change that.
            if report == WorldStreamPoll::default()
                && self.decode_rx.is_empty()
                && self.lighting.rx.is_empty()
            {
                // Queued mesh results here wait on capacity only the frame frees.
                self.wait_for_worker_result(wake, self.mesh_rx.is_empty(), SERVICE_IDLE_WAIT);
            }
        }
        self.service_yield = None;
        (total, busy)
    }

    /// Blocks until a decode or light result, a mesh result when `mesh` is set, or a wake
    /// signal is ready, or until `timeout`.
    fn wait_for_worker_result(&self, wake: &Receiver<()>, mesh: bool, timeout: Duration) {
        let mut select = Select::new();
        select.recv(&self.decode_rx);
        select.recv(&self.lighting.rx);
        if mesh {
            select.recv(&self.mesh_rx);
        }
        select.recv(wake);
        let _ = select.ready_timeout(timeout);
    }
}

impl WorldStreamPoll {
    /// Adds another poll's work, e.g. a frame's poll to its between-frames service work.
    pub fn accumulate(&mut self, other: Self) {
        self.commit_steps += other.commit_steps;
        self.decoded_results += other.decoded_results;
        self.light_results += other.light_results;
        self.light_jobs_dispatched += other.light_jobs_dispatched;
        self.mesh_results += other.mesh_results;
        self.mesh_jobs_dispatched += other.mesh_jobs_dispatched;
        self.mesh_changes_queued += other.mesh_changes_queued;
    }
}

struct ServiceLaunch {
    stream: WorldStream,
    camera_position: [f32; 3],
    max_mesh_jobs: usize,
}

// Keep successful handoffs inline: boxing the stream would allocate on every frame.
#[allow(clippy::large_enum_variant)]
enum ServiceResult {
    Done(ServicedStream),
    Panicked(Box<dyn Any + Send>),
}

/// A stream handed back by [`WorldStreamService::reclaim`], with the work done meanwhile.
pub struct ServicedStream {
    pub stream: WorldStream,
    /// Poll work performed on the service thread.
    pub report: WorldStreamPoll,
    /// Time the service thread spent polling, excluding idle waits for worker results.
    pub busy: Duration,
    /// Time from launch until the service yielded the stream.
    pub held: Duration,
}

/// Owns one service thread that polls a checked-out [`WorldStream`] between frames.
///
/// The frame thread hands the stream over with [`launch`](Self::launch) once nothing else in
/// the frame needs it, and takes it back with [`reclaim`](Self::reclaim) before anything does.
/// Reclaim waits for the active poll to yield; work already running can exceed its slice.
/// A panic on the service thread resumes on the reclaiming thread, as it would have had the
/// poll run there.
pub struct WorldStreamService {
    launches: Option<Sender<ServiceLaunch>>,
    results: Receiver<ServiceResult>,
    wake: Sender<()>,
    yield_request: Arc<AtomicBool>,
    servicing: bool,
    thread: Option<JoinHandle<()>>,
}

impl WorldStreamService {
    /// Starts the service thread, named `stream-service`.
    pub fn spawn() -> std::io::Result<Self> {
        // One-slot channels keep the hand-off allocation-free after startup.
        let (launches, launch_rx) = bounded::<ServiceLaunch>(1);
        let (result_tx, results) = bounded(1);
        let (wake, wake_rx) = bounded(1);
        let yield_request = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("stream-service".to_owned())
            .spawn({
                let yield_request = Arc::clone(&yield_request);
                move || serve(&launch_rx, &result_tx, &wake_rx, &yield_request)
            })?;
        Ok(Self {
            launches: Some(launches),
            results,
            wake,
            yield_request,
            servicing: false,
            thread: Some(thread),
        })
    }

    /// Whether the service currently holds a stream.
    #[must_use]
    pub const fn is_servicing(&self) -> bool {
        self.servicing
    }

    /// Hands `stream` to the service thread, which polls it until the next reclaim.
    ///
    /// # Panics
    ///
    /// If the service already holds a stream or its thread has exited.
    pub fn launch(
        &mut self,
        mut stream: WorldStream,
        camera_position: [f32; 3],
        max_mesh_jobs: usize,
    ) {
        assert!(!self.servicing, "the service already holds a stream");
        // Its next service run reaches any retention request before the frame's next poll.
        stream.between_frames_service = true;
        self.yield_request.store(false, Ordering::Release);
        self.launches
            .as_ref()
            .expect("service is live until dropped")
            .send(ServiceLaunch {
                stream,
                camera_position,
                max_mesh_jobs,
            })
            .unwrap_or_else(|_| panic!("stream service thread exited"));
        self.servicing = true;
    }

    /// Asks the service to yield and waits for the stream; `None` when it holds none.
    ///
    /// # Panics
    ///
    /// Resumes a panic raised while servicing, or panics if the service thread exited.
    pub fn reclaim(&mut self) -> Option<ServicedStream> {
        if !self.servicing {
            return None;
        }
        self.servicing = false;
        self.yield_request.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        match self.results.recv() {
            Ok(ServiceResult::Done(serviced)) => Some(serviced),
            Ok(ServiceResult::Panicked(payload)) => resume_unwind(payload),
            Err(_) => panic!("stream service thread exited while servicing"),
        }
    }
}

impl Drop for WorldStreamService {
    fn drop(&mut self) {
        if self.servicing {
            self.servicing = false;
            self.yield_request.store(true, Ordering::Release);
            let _ = self.wake.try_send(());
            // Drops a held stream here rather than on a detached thread; a service panic
            // already ended its run, so only the payload is discarded.
            let _ = self.results.recv();
        }
        drop(self.launches.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    launches: &Receiver<ServiceLaunch>,
    results: &Sender<ServiceResult>,
    wake: &Receiver<()>,
    yield_request: &Arc<AtomicBool>,
) {
    while let Ok(ServiceLaunch {
        mut stream,
        camera_position,
        max_mesh_jobs,
    }) = launches.recv()
    {
        // A wake left by an earlier reclaim must not cut this run's first idle wait short.
        while wake.try_recv().is_ok() {}
        let started = Instant::now();
        let serviced = catch_unwind(AssertUnwindSafe(|| {
            stream.service_until_yield(camera_position, max_mesh_jobs, yield_request, wake)
        }));
        let result = match serviced {
            Ok((report, busy)) => {
                let held = started.elapsed();
                stream.service_window = held;
                ServiceResult::Done(ServicedStream {
                    stream,
                    report,
                    busy,
                    held,
                })
            }
            Err(payload) => ServiceResult::Panicked(payload),
        };
        if results.send(result).is_err() {
            return;
        }
    }
}
