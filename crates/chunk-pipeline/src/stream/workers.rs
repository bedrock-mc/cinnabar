mod priority;

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Cores left to the frame (main and render threads).
const FRAME_CORES: usize = 2;
const MIN_WORLD_THREADS: usize = 3;
/// A lower lane's oldest job jumps the priority order after waiting this long, so
/// sustained mesh load cannot starve the decode and light work that mesh depends on.
const DECODE_MAX_WAIT: Duration = Duration::from_millis(4);
const LIGHT_MAX_WAIT: Duration = Duration::from_millis(16);
/// Whether background workers run lowered only while a job runs, taking the queue lock at normal
/// priority. Windows locks have no priority inheritance: a lowered holder preempted on a busy
/// machine can wait seconds for its anti-starvation boost while the frame thread waits on the
/// lock. Elsewhere a thread cannot raise its niceness back, so workers stay lowered for life.
const LOWER_PER_JOB: bool = cfg!(windows);

/// Work classes in scheduling order: mesh gates chunks appearing, decode feeds it, light trails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Mesh,
    Decode,
    Light,
}

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
    fn take(&mut self, background: bool, now: Instant) -> Option<Job> {
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

/// Runs `work` on as many threads as the world pool, for work that finishes before a world
/// streams, such as a join's pack compile. The pool exists only for this call. Its width
/// already leaves the frame threads their cores, so it keeps normal priority: lowered threads
/// would let any busy background process stretch a join. `work` runs on the caller's pool
/// instead if those threads cannot start.
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
    jobs: Vec<(Instant, Job)>,
}

impl DispatchBatch<'_> {
    /// Adds an independent task without waking or locking the pool yet.
    pub(super) fn spawn(&mut self, job: impl FnOnce() + Send + 'static) {
        self.spawn_with_scratch(move |_| job());
    }

    /// Gives a solve exclusive access to this worker's retained scratch buffers.
    pub(super) fn spawn_with_scratch(
        &mut self,
        job: impl FnOnce(&mut world::LightSolverScratch) + Send + 'static,
    ) {
        self.jobs.push((Instant::now(), Box::new(job)));
    }
}

impl Drop for DispatchBatch<'_> {
    fn drop(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        let mut queues = self.pool.shared.lock();
        for (at, job) in self.jobs.drain(..) {
            queues.push(self.lane, at, job);
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
        let lowered = background && LOWER_PER_JOB && priority::lower().is_ok();
        // Matches rayon's default: a panicking world job aborts rather than losing its permits.
        if catch_unwind(AssertUnwindSafe(|| job(&mut scratch))).is_err() {
            std::process::abort();
        }
        if lowered && let Err(error) = priority::restore() {
            eprintln!("{name}: could not restore worker priority: {error}");
        }
        queues = shared.lock();
    }
}

#[cfg(test)]
mod tests;
