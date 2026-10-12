mod frame_budget;
mod priority;

pub use frame_budget::FrameJobBudget;

use std::cell::Cell;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Duration;
use web_time::Instant;

use crossbeam_channel::Sender;

/// Cores left to the frame (main and render threads).
const FRAME_CORES: usize = 2;
const MIN_WORLD_THREADS: usize = 3;
/// A lower lane's oldest job jumps the priority order after waiting this long, so
/// sustained mesh load cannot starve the decode and light work that mesh depends on.
const DECODE_MAX_WAIT: Duration = Duration::from_millis(4);
const LIGHT_MAX_WAIT: Duration = Duration::from_millis(16);
/// Windows lowers workers only during jobs so queue locks retain normal priority.
/// Other platforms keep workers lowered because they cannot restore their niceness.
const LOWER_PER_JOB: bool = cfg!(windows);

/// Work classes in scheduling order: mesh gates chunks appearing, decode feeds it, light trails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Mesh,
    Decode,
    Light,
}

impl Lane {
    /// This lane's place in per-lane arrays, in scheduling order.
    const fn index(self) -> usize {
        match self {
            Self::Mesh => 0,
            Self::Decode => 1,
            Self::Light => 2,
        }
    }
}

/// Urgent jobs a browser runs inline per call of [`run_urgent_world_jobs`]: one block change's
/// light batch and the meshes of its section and neighbours.
#[cfg(target_arch = "wasm32")]
const MAX_INLINE_URGENT_JOBS: usize = 8;

/// Thread split for one machine; only background threads take light, so they cap its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PoolSize {
    pub(super) foreground: usize,
    pub(super) background: usize,
}

impl PoolSize {
    pub(super) fn for_cores(cores: usize) -> Self {
        let threads = cores.saturating_sub(FRAME_CORES).max(MIN_WORLD_THREADS);
        let background = (threads * 2 / 3).max(super::MIN_EFFECTIVE_LIGHT_JOB_CAP);
        Self {
            foreground: threads - background,
            background,
        }
    }

    pub(super) const fn threads(self) -> usize {
        self.foreground + self.background
    }
}

type Job = Box<dyn FnOnce(&mut world::LightSolverScratch) + Send + 'static>;
type Queue = VecDeque<(Instant, Job)>;

#[derive(Default)]
struct Queues {
    /// Work for a block the local player just changed, with its lane. Every worker takes it
    /// before any lane's backlog, so the change shows at once.
    urgent: VecDeque<(Lane, Job)>,
    mesh: Queue,
    decode: Queue,
    light: Queue,
    shutdown: bool,
}

impl Queues {
    fn push(&mut self, lane: Lane, queued_at: Instant, job: Job) {
        let queue = match lane {
            Lane::Mesh => &mut self.mesh,
            Lane::Decode => &mut self.decode,
            Lane::Light => &mut self.light,
        };
        queue.push_back((queued_at, job));
    }

    /// Next job for a worker at time `now`; the clock is a parameter so ageing is testable.
    /// Urgent work comes first on any thread.
    fn take(&mut self, background: bool, now: Instant) -> Option<Job> {
        if let Some((_, job)) = self.urgent.pop_front() {
            return Some(job);
        }
        let overdue = |queue: &Queue, limit| {
            queue
                .front()
                .is_some_and(|(queued, _)| now.saturating_duration_since(*queued) >= limit)
        };
        let lane = if background && overdue(&self.light, LIGHT_MAX_WAIT) {
            &mut self.light
        } else if overdue(&self.decode, DECODE_MAX_WAIT) || self.mesh.is_empty() {
            if self.decode.is_empty() && background {
                &mut self.light
            } else {
                &mut self.decode
            }
        } else {
            &mut self.mesh
        };
        lane.pop_front().map(|(_, job)| job)
    }
}

/// Shares of a browser frame's job time for mesh, decode and light when all three have work:
/// terrain goes on screen first, and its light follows at a quarter.
const LANE_WEIGHTS: [u32; 3] = [3, 3, 1];

impl Queues {
    /// Next job for a frame that runs every lane on one thread, given each lane's recent time
    /// and `fits`, whether a lane's typical job fits the time left. The lane least served for
    /// its weight that fits goes first, so no lane waits behind another's backlog; ties go to mesh, then decode,
    /// then light, the order in which they put terrain on screen. Without a fitting lane,
    /// `must_run` takes the least-served one anyway, and otherwise nothing runs.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn take_least_served(
        &mut self,
        served: &[Duration; 3],
        fits: impl Fn(usize) -> bool,
        must_run: bool,
    ) -> Option<(usize, Job)> {
        if let Some((lane, job)) = self.urgent.pop_front() {
            return Some((lane.index(), job));
        }
        let mut lanes = [0, 1, 2];
        lanes.sort_by_key(|&lane| served[lane] / LANE_WEIGHTS[lane]);
        let ready = lanes.map(|lane| (!self.lane(lane).is_empty()).then_some(lane));
        let lane = ready
            .into_iter()
            .flatten()
            .find(|&lane| fits(lane))
            .or_else(|| ready.into_iter().flatten().next().filter(|_| must_run))?;
        self.lane_mut(lane).pop_front().map(|(_, job)| (lane, job))
    }

    fn lane(&self, lane: usize) -> &Queue {
        match lane {
            0 => &self.mesh,
            1 => &self.decode,
            _ => &self.light,
        }
    }

    fn lane_mut(&mut self, lane: usize) -> &mut Queue {
        match lane {
            0 => &mut self.mesh,
            1 => &mut self.decode,
            _ => &mut self.light,
        }
    }
}

#[derive(Default)]
struct Shared {
    queues: Mutex<Queues>,
    ready: Condvar,
    /// Queue locks taken by a thread running below normal priority.
    #[cfg(all(test, windows))]
    lowered_locks: std::sync::atomic::AtomicUsize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.queue_lock").entered();
        #[cfg(all(test, windows))]
        if priority::is_lowered() {
            self.lowered_locks
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One world pool: threads prefer mesh, then decode, then light, unless a lower lane is
/// overdue. Light runs only on the lowered-priority background threads.
pub(super) struct WorldPool {
    shared: Arc<Shared>,
    size: PoolSize,
}

/// Threads the world pool runs on a machine with `cores` logical processors.
pub fn world_worker_threads(cores: usize) -> usize {
    PoolSize::for_cores(cores).threads()
}

/// Runs pre-stream work on a temporary normal-priority pool sized like the world pool.
/// Falls back to the caller pool if worker threads cannot start.
pub fn on_idle_world_cores<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    match rayon::ThreadPoolBuilder::new()
        .num_threads(world_worker_threads(cores))
        .thread_name(|index| format!("world-borrowed-{index}"))
        .build()
    {
        Ok(pool) => pool.install(work),
        Err(_) => work(),
    }
}

/// Cores a browser's pool is sized for. Its frame runs the jobs on its only thread under a time
/// budget, so admission only has to keep that budget fed, as this many workers would be.
#[cfg(target_arch = "wasm32")]
const FRAME_DRIVEN_CORES: usize = 14;

pub(super) static WORKERS: LazyLock<WorldPool> = LazyLock::new(|| {
    #[cfg(target_arch = "wasm32")]
    let cores = FRAME_DRIVEN_CORES;
    #[cfg(not(target_arch = "wasm32"))]
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    WorldPool::new(PoolSize::for_cores(cores))
});

impl WorldPool {
    fn new(size: PoolSize) -> Self {
        let shared = Arc::new(Shared::default());
        // Browsers have no threads; the frame runs queued jobs through `run_queued_world_jobs`.
        #[cfg(not(target_arch = "wasm32"))]
        for (name, count, background) in [
            ("world", size.foreground, false),
            ("world-bg", size.background, true),
        ] {
            for index in 0..count {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("{name}-{index}"))
                    .spawn(move || work(&shared, name, background))
                    .expect("world worker could not start");
            }
        }
        Self { shared, size }
    }

    pub(super) const fn size(&self) -> PoolSize {
        self.size
    }

    pub(super) fn spawn(&self, lane: Lane, job: impl FnOnce() + Send + 'static) {
        self.shared
            .lock()
            .push(lane, Instant::now(), Box::new(move |_| job()));
        // A foreground waiter cannot take light, so light wakes everyone.
        if lane == Lane::Light {
            self.shared.ready.notify_all();
        } else {
            self.shared.ready.notify_one();
        }
    }
    /// Publishes a bounded dispatch cohort with one queue lock and one wakeup.
    pub(super) fn batch(&self, lane: Lane) -> DispatchBatch<'_> {
        DispatchBatch {
            pool: self,
            lane,
            jobs: Vec::new(),
        }
    }
}

/// Prepared jobs retain independent scheduling priority after their shared publication.
pub(super) struct DispatchBatch<'a> {
    pool: &'a WorldPool,
    lane: Lane,
    jobs: Vec<(Instant, bool, Job)>,
}

impl DispatchBatch<'_> {
    /// Adds an independent task without waking or locking the pool yet; an `urgent` one, for a
    /// block the local player just changed, runs ahead of every lane's backlog.
    pub(super) fn spawn(&mut self, urgent: bool, job: impl FnOnce() + Send + 'static) {
        self.spawn_with_scratch(urgent, move |_| job());
    }

    /// Gives a solve exclusive access to this worker's retained scratch buffers.
    pub(super) fn spawn_with_scratch(
        &mut self,
        urgent: bool,
        job: impl FnOnce(&mut world::LightSolverScratch) + Send + 'static,
    ) {
        self.jobs.push((Instant::now(), urgent, Box::new(job)));
    }
}

impl Drop for DispatchBatch<'_> {
    fn drop(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        let mut queues = self.pool.shared.lock();
        for (at, urgent, job) in self.jobs.drain(..) {
            if urgent {
                queues.urgent.push_back((self.lane, job));
            } else {
                queues.push(self.lane, at, job);
            }
        }
        drop(queues);
        self.pool.shared.ready.notify_all();
    }
}

impl Drop for WorldPool {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.ready.notify_all();
    }
}

/// Runs queued world jobs on the calling thread until none remain or the next would overrun
/// `budget`, judged by each lane's recent job times; the first always runs, so a busy frame
/// still makes progress. Returns the time spent. WebAssembly has no worker threads, so the frame
/// drives the pool through this. Results arrive on the stream's channels for its next poll.
#[cfg(target_arch = "wasm32")]
pub fn run_queued_world_jobs(budget: Duration) -> Duration {
    thread_local! {
        /// Recent job time per lane, for judging whether the next job fits.
        static TYPICAL: Cell<[Duration; 3]> = const { Cell::new([Duration::ZERO; 3]) };
        /// Each lane's recent share of frame time, decaying every frame, so a lane whose jobs
        /// rarely fit what is left of a frame still leads the next one.
        static SERVED: Cell<[Duration; 3]> = const { Cell::new([Duration::ZERO; 3]) };
    }
    let started = Instant::now();
    let mut served = SERVED.get().map(|served| served - served / 8);
    let mut must_run = true;
    // Light is taken too: this thread stands in for the background workers as well.
    loop {
        let typical = TYPICAL.get();
        let left = budget.saturating_sub(started.elapsed());
        let Some((lane, job)) = WORKERS.shared.lock().take_least_served(
            &served,
            |lane| typical[lane] <= left,
            must_run,
        ) else {
            break;
        };
        must_run = false;
        let job_started = Instant::now();
        FRAME_SCRATCH.with_borrow_mut(|scratch| job(scratch));
        let took = job_started.elapsed();
        served[lane] += took;
        let mut typical = typical;
        typical[lane] = if took > typical[lane] {
            typical[lane] + (took - typical[lane]) / 4
        } else {
            typical[lane] - (typical[lane] - took) / 4
        };
        TYPICAL.set(typical);
    }
    SERVED.set(served);
    started.elapsed()
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Light scratch buffers for the jobs a browser's only thread runs.
    static FRAME_SCRATCH: std::cell::RefCell<world::LightSolverScratch> =
        std::cell::RefCell::default();
}

/// Runs queued urgent jobs on the calling thread, at most [`MAX_INLINE_URGENT_JOBS`], so a
/// browser, which has no workers to run them meanwhile, shows the local player's block change
/// in the frame that made it rather than after the next frame's job pass. Returns the jobs run.
#[cfg(target_arch = "wasm32")]
pub(super) fn run_urgent_world_jobs() -> usize {
    let mut ran = 0;
    while ran < MAX_INLINE_URGENT_JOBS {
        let Some((_, job)) = WORKERS.shared.lock().urgent.pop_front() else {
            break;
        };
        FRAME_SCRATCH.with_borrow_mut(|scratch| job(scratch));
        ran += 1;
    }
    ran
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn work(shared: &Shared, name: &str, background: bool) {
    if background
        && !LOWER_PER_JOB
        && let Err(error) = priority::lower()
    {
        eprintln!("{name}: could not lower worker priority: {error}");
    }
    let mut scratch = world::LightSolverScratch::default();
    let mut queues = shared.lock();
    loop {
        if queues.shutdown {
            return;
        }
        let Some(job) = queues.take(background, Instant::now()) else {
            #[cfg(feature = "tracy")]
            let _zone = tracing::info_span!("stream.worker_wait").entered();
            queues = shared
                .ready
                .wait(queues)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            continue;
        };
        drop(queues);
        LOWERED_FOR_JOB.set(background && LOWER_PER_JOB && priority::lower().is_ok());
        // Matches rayon's default: a panicking world job aborts rather than losing its permits.
        if catch_unwind(AssertUnwindSafe(|| job(&mut scratch))).is_err() {
            std::process::abort();
        }
        restore_lowered_job();
        queues = shared.lock();
    }
}

thread_local! {
    /// Whether this worker runs its current job lowered and must restore normal priority.
    static LOWERED_FOR_JOB: Cell<bool> = const { Cell::new(false) };
}

/// Returns a worker lowered for its job to normal priority; does nothing otherwise.
fn restore_lowered_job() {
    if LOWERED_FOR_JOB.replace(false)
        && let Err(error) = priority::restore()
    {
        let name = std::thread::current().name().unwrap_or("world").to_owned();
        eprintln!("{name}: could not restore worker priority: {error}");
    }
}

/// Restores normal job priority before reserving a result-channel slot.
/// This prevents the receiving frame from spinning on a preempted, lowered sender.
pub(super) fn send_result<T>(tx: &Sender<T>, result: T) {
    restore_lowered_job();
    let _ = tx.send(result);
}

#[cfg(test)]
mod tests;
