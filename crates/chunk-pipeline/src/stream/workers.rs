mod priority;

use std::sync::LazyLock;

use rayon::{ThreadPool, ThreadPoolBuilder};

/// Only one decode lane counts against the background budget; extra lanes are bursty and
/// preempt the lowered-priority light pool rather than taking its threads.
const RESERVED_DECODE_WORKERS: usize = 1;
const MAX_DECODE_WORKERS: usize = 3;
const MIN_MESH_WORKERS: usize = 1;

/// Separate queues keep column solves from holding up decode and ready geometry.
pub(super) struct WorldWorkers {
    pub(super) light: ThreadPool,
    pub(super) mesh: ThreadPool,
    pub(super) decode: ThreadPool,
}

pub(super) static WORKERS: LazyLock<WorldWorkers> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    WorldWorkers::new(cores)
});

impl WorldWorkers {
    /// Limits world workers below CPU capacity when at least six processors are available.
    fn new(cores: usize) -> Self {
        let minimum =
            super::MIN_EFFECTIVE_LIGHT_JOB_CAP + MIN_MESH_WORKERS + RESERVED_DECODE_WORKERS;
        let background = cores.saturating_sub(2).max(minimum);
        let light = (cores / 2).clamp(
            super::MIN_EFFECTIVE_LIGHT_JOB_CAP,
            background - MIN_MESH_WORKERS - RESERVED_DECODE_WORKERS,
        );
        Self {
            light: pool("world-light", light, true),
            mesh: pool(
                "world-mesh",
                background - light - RESERVED_DECODE_WORKERS,
                false,
            ),
            decode: pool("world-decode", decode_workers(cores), false),
        }
    }
}

/// Completions re-sequence in the ordered commit state, so decode width never reorders commits.
fn decode_workers(cores: usize) -> usize {
    (cores / 4).clamp(RESERVED_DECODE_WORKERS, MAX_DECODE_WORKERS)
}

/// Names workers so captured thread samples identify the service that owns them.
fn pool(name: &'static str, threads: usize, background: bool) -> ThreadPool {
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .start_handler(move |_| {
            if background && let Err(error) = priority::lower() {
                eprintln!("{name}: could not lower worker priority: {error}");
            }
        })
        .thread_name(move |index| format!("{name}-{index}"))
        .build()
        .expect("world worker pool could not start")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Decode widens with the machine but stays small and leaves the light and mesh lanes intact.
    #[test]
    fn decode_lanes_scale_with_cores_up_to_a_small_cap() {
        assert_eq!(decode_workers(1), 1);
        assert_eq!(decode_workers(4), 1);
        assert!(decode_workers(8) >= 2);
        assert_eq!(decode_workers(64), MAX_DECODE_WORKERS);
        let workers = WorldWorkers::new(12);
        assert!(workers.decode.current_num_threads() >= 2);
        assert!(workers.mesh.current_num_threads() >= MIN_MESH_WORKERS);
        assert!(workers.light.current_num_threads() >= super::super::MIN_EFFECTIVE_LIGHT_JOB_CAP);
    }

    /// Saturating every light worker leaves both latency-sensitive lanes available.
    #[test]
    fn saturated_lighting_cannot_queue_ahead_of_mesh_or_decode() {
        let workers = WorldWorkers::new(8);
        let (started_tx, started_rx) = crossbeam_channel::unbounded();
        let (release_tx, release_rx) = crossbeam_channel::unbounded();
        for _ in 0..workers.light.current_num_threads() {
            let started = started_tx.clone();
            let release = release_rx.clone();
            workers.light.spawn(move || {
                started.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(5)).unwrap();
            });
        }
        for _ in 0..workers.light.current_num_threads() {
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let (done_tx, done_rx) = crossbeam_channel::unbounded();
        let mesh_done = done_tx.clone();
        workers.mesh.spawn(move || mesh_done.send(()).unwrap());
        workers.decode.spawn(move || done_tx.send(()).unwrap());
        let completed = (0..2).all(|_| done_rx.recv_timeout(Duration::from_secs(2)).is_ok());
        for _ in 0..workers.light.current_num_threads() {
            release_tx.send(()).unwrap();
        }
        assert!(completed, "lighting blocked another worker lane");
    }
}
