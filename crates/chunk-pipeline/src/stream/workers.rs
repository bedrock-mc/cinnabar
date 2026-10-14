mod priority;
mod queues;

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Instant;

use crossbeam_channel::Sender;

pub(super) use self::queues::Lane;
#[cfg(test)]
use self::queues::{DECODE_MAX_WAIT, LIGHT_MAX_WAIT};
use self::queues::{Job, Queues};

/// Cores left to the frame (main and render threads).
const FRAME_CORES: usize = 2;
const MIN_WORLD_THREADS: usize = 3;
/// Windows lowers workers only during jobs so queue locks retain normal priority.
/// Other platforms keep workers lowered because they cannot restore their niceness.
const LOWER_PER_JOB: bool = cfg!(windows);

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

/// One world pool: threads prefer mesh, then decode, then light, then distant terrain, unless
/// decode or light is overdue. Light and distant terrain run only on the lowered-priority
/// background threads.
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

pub(super) static WORKERS: LazyLock<WorldPool> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    WorldPool::new(PoolSize::for_cores(cores))
});

impl WorldPool {
    fn new(size: PoolSize) -> Self {
        let shared = Arc::new(Shared::default());
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
        #[cfg(test)]
        note_queued(lane, 1);
        self.shared
            .lock()
            .push(lane, Instant::now(), Box::new(move |_| job()));
        // A foreground waiter cannot take light or distant terrain work, so they wake everyone.
        if matches!(lane, Lane::Light | Lane::Lod) {
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
        #[cfg(test)]
        note_queued(self.lane, self.jobs.len());
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

/// Lowers the calling thread, which runs beside the pool, to the background workers'
/// priority for good; a failure is logged and leaves it at normal priority.
pub(super) fn lower_beside_pool(name: &str) {
    if let Err(error) = priority::lower() {
        eprintln!("{name}: could not lower thread priority: {error}");
    }
}

#[cfg(test)]
thread_local! {
    /// Distant terrain jobs this thread queued on the Lod lane.
    pub(super) static LOD_JOBS_QUEUED: Cell<usize> = const { Cell::new(0) };
}

/// Counts `count` jobs this thread queued on `lane` in [`LOD_JOBS_QUEUED`] when it is the
/// Lod lane.
#[cfg(test)]
fn note_queued(lane: Lane, count: usize) {
    if lane == Lane::Lod {
        LOD_JOBS_QUEUED.with(|queued| queued.set(queued.get() + count));
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
