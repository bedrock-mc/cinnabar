#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod priority;
#[cfg(feature = "tracy")]
mod queue_lock;
#[cfg(not(target_arch = "wasm32"))]
use native::{now, restore_lowered_job};
#[cfg(feature = "tracy")]
use queue_lock::QueueGuard;
use simulation_clock::Moment;

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use crossbeam_channel::Sender;

#[cfg(not(feature = "tracy"))]
type QueueGuard<'a> = MutexGuard<'a, Queues>;

/// Cores left to the frame (main and render threads).
const FRAME_CORES: usize = 2;
const MIN_WORLD_THREADS: usize = 3;
/// A lower lane's oldest job jumps the priority order after waiting this long, so
/// sustained mesh load cannot starve the decode and light work that mesh depends on.
const DECODE_MAX_WAIT: Duration = Duration::from_millis(4);
const LIGHT_MAX_WAIT: Duration = Duration::from_millis(16);

/// Work classes in scheduling order: mesh gates chunks appearing, decode feeds it, light and summaries trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Mesh,
    Decode,
    Light,
    Summary,
}

/// Thread split for one machine; only background threads take light, so they cap its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PoolSize {
    pub(super) foreground: usize,
    pub(super) background: usize,
}

impl PoolSize {
    /// Splits the native pool while reserving frame cores and the minimum light width.
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
type Queue = VecDeque<(Moment, Job)>;

#[derive(Default)]
struct Queues {
    /// Work for a block the local player just changed, with its lane. Every worker takes it
    /// before any lane's backlog, so the change shows at once.
    urgent: VecDeque<(Lane, Job)>,
    mesh: Queue,
    decode: Queue,
    light: Queue,
    summary: Queue,
    shutdown: bool,
    #[cfg(target_arch = "wasm32")]
    observed_at: Moment,
    #[cfg(any(test, target_arch = "wasm32"))]
    foreground_turn: bool,
}

impl Queues {
    /// Adds one lane job with its monotonic submission observation.
    fn push(&mut self, lane: Lane, queued_at: Moment, job: Job) {
        let queue = match lane {
            Lane::Mesh => &mut self.mesh,
            Lane::Decode => &mut self.decode,
            Lane::Light => &mut self.light,
            Lane::Summary => &mut self.summary,
        };
        queue.push_back((queued_at, job));
    }

    /// Alternates worker eligibility so overdue light cannot occupy every browser turn.
    #[cfg(any(test, target_arch = "wasm32"))]
    fn take_cooperative(&mut self, now: Moment) -> Option<Job> {
        let background = !self.foreground_turn;
        let job = self
            .take(background, now)
            .or_else(|| (!background).then(|| self.take(true, now)).flatten());
        if job.is_some() {
            self.foreground_turn = !self.foreground_turn;
        }
        job
    }

    /// Next job for a worker at time `now`; the clock is a parameter so ageing is testable.
    /// Urgent work comes first on any thread.
    fn take(&mut self, background: bool, now: Moment) -> Option<Job> {
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
        lane.pop_front()
            .or_else(|| background.then(|| self.summary.pop_front()).flatten())
            .map(|(_, job)| job)
    }
}

#[derive(Default)]
struct Shared {
    queues: Mutex<Queues>,
    ready: Condvar,
    #[cfg(all(test, windows))]
    lowered_locks: std::sync::atomic::AtomicUsize,
}

impl Shared {
    /// Recovers the job queue after poison without discarding accepted work.
    fn lock(&self) -> QueueGuard<'_> {
        #[cfg(feature = "tracy")]
        let owned = tracing::info_span!("stream.queue_owned");
        #[cfg(all(feature = "tracy", not(target_arch = "wasm32")))]
        let wait = tracing::info_span!("stream.worker_wait");
        #[cfg(all(test, windows))]
        if priority::is_lowered() {
            self.lowered_locks
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let guard = {
            #[cfg(feature = "tracy")]
            let _zone = tracing::info_span!("stream.queue_lock").entered();
            self.queues
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        #[cfg(all(feature = "tracy", not(target_arch = "wasm32")))]
        let guard = QueueGuard::new(guard, owned, wait);
        #[cfg(all(feature = "tracy", target_arch = "wasm32"))]
        let guard = QueueGuard::new(guard, owned);
        guard
    }

    /// Sleeps atomically with queue release, excluding the sleep from lock ownership.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait<'a>(&'a self, guard: QueueGuard<'a>) -> QueueGuard<'a> {
        #[cfg(feature = "tracy")]
        {
            let (guard, owned, wait) = guard.end_ownership();
            let waiting = wait.entered();
            let guard = self
                .ready
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            QueueGuard::new(guard, owned, waiting.exit())
        }
        #[cfg(not(feature = "tracy"))]
        self.ready
            .wait(guard)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// World queues; native light jobs run on the lowered-priority background share.
pub(super) struct WorldPool {
    shared: Arc<Shared>,
    size: PoolSize,
}

/// Threads the world pool runs on a machine with `cores` logical processors.
pub fn world_worker_threads(cores: usize) -> usize {
    PoolSize::for_cores(cores).threads()
}

/// Diagnostic cap on the logical processors every thread pool sizes itself for.
const CORES_ENV: &str = "CINNABAR_CPU_CORES";

/// Logical processors the client's pools are sized for: the machine's count, or fewer when
/// [`CORES_ENV`] asks for a smaller machine.
#[cfg(not(target_arch = "wasm32"))]
pub fn logical_cores() -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    capped_cores(available, std::env::var(CORES_ENV).ok().as_deref())
}

/// Applies a positive integer cap below `available`; anything else keeps `available`.
#[cfg(any(test, not(target_arch = "wasm32")))]
fn capped_cores(available: usize, requested: Option<&str>) -> usize {
    requested
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&cores| cores > 0)
        .map_or(available, |cores| cores.min(available))
}

/// Runs pre-stream work on a temporary normal-priority pool sized like the world pool.
/// Falls back to the caller pool if worker threads cannot start.
#[cfg(not(target_arch = "wasm32"))]
pub fn on_idle_world_cores<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let cores = logical_cores();
    match rayon::ThreadPoolBuilder::new()
        .num_threads(world_worker_threads(cores))
        .thread_name(|index| format!("world-borrowed-{index}"))
        .build()
    {
        Ok(pool) => pool.install(work),
        Err(_) => work(),
    }
}

pub(super) static WORKERS: LazyLock<WorldPool> = LazyLock::new(|| {
    #[cfg(not(target_arch = "wasm32"))]
    let cores = logical_cores();
    #[cfg(target_arch = "wasm32")]
    let cores = 1;
    WorldPool::new(PoolSize::for_cores(cores))
});

impl WorldPool {
    /// Submits detached work to the executor driven by the browser host.
    #[cfg(any(test, target_arch = "wasm32"))]
    fn retire(&self, job: impl FnOnce() + Send + 'static) {
        // Decode ageing also prevents a mesh backlog from retaining evicted allocations.
        self.spawn(Lane::Decode, job);
    }

    /// Creates the queues and starts native threads; the browser driver retains the same jobs.
    fn new(size: PoolSize) -> Self {
        let shared = Arc::new(Shared::default());
        #[cfg(not(target_arch = "wasm32"))]
        native::start(&shared, size);
        Self { shared, size }
    }

    /// Samples native time, or uses the browser driver's latest frame observation.
    fn observed_at(&self) -> Moment {
        #[cfg(not(target_arch = "wasm32"))]
        {
            now()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.shared.lock().observed_at
        }
    }

    pub(super) const fn size(&self) -> PoolSize {
        self.size
    }

    /// Accepts a finite job without running it on the submitting thread.
    pub(super) fn spawn(&self, lane: Lane, job: impl FnOnce() + Send + 'static) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.queue_submit", ?lane, jobs = 1).entered();
        let queued_at = self.observed_at();
        self.shared
            .lock()
            .push(lane, queued_at, Box::new(move |_| job()));
        // Background-only lanes must wake an eligible worker even if a foreground one waits.
        if matches!(lane, Lane::Light | Lane::Summary) {
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

/// Runs detached work, such as releasing world allocations or building a shared table, on the
/// host's background executor, never on the submitting thread.
pub(super) fn detach(job: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(job);
    #[cfg(target_arch = "wasm32")]
    WORKERS.retire(job);
}

/// Prepared jobs retain independent priority after their shared publication.
pub(super) struct DispatchBatch<'a> {
    pool: &'a WorldPool,
    lane: Lane,
    jobs: Vec<(Moment, bool, Job)>,
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
        self.jobs
            .push((self.pool.observed_at(), urgent, Box::new(job)));
    }
}

impl Drop for DispatchBatch<'_> {
    fn drop(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        #[cfg(feature = "tracy")]
        let _zone =
            tracing::info_span!("stream.queue_submit", lane = ?self.lane, jobs = self.jobs.len())
                .entered();
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

/// Restores normal job priority before reserving a result-channel slot.
/// This prevents the receiving frame from spinning on a preempted, lowered sender.
pub(super) fn send_result<T>(tx: &Sender<T>, result: T) {
    #[cfg(not(target_arch = "wasm32"))]
    restore_lowered_job();
    let _ = tx.send(result);
}

/// Updates the browser observation before a frame submits jobs.
#[cfg(target_arch = "wasm32")]
pub(crate) fn observe(now: Moment) {
    let mut queues = WORKERS.shared.lock();
    queues.observed_at = queues.observed_at.max(now);
}

/// Runs one cooperative turn using native foreground and background eligibility.
#[cfg(any(test, target_arch = "wasm32"))]
fn run_one(shared: &Shared, now: Moment, scratch: &mut world::LightSolverScratch) -> bool {
    let Some(job) = shared.lock().take_cooperative(now) else {
        return false;
    };
    // Matches native workers: a panic cannot silently lose job permits.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(scratch))).is_err() {
        std::process::abort();
    }
    true
}

/// Runs at most one browser world job; the host yields between calls.
#[cfg(target_arch = "wasm32")]
pub(crate) fn run_browser_job(now: Moment, scratch: &mut world::LightSolverScratch) -> bool {
    observe(now);
    let now = WORKERS.shared.lock().observed_at;
    run_one(&WORKERS.shared, now, scratch)
}

#[cfg(test)]
mod tests;
