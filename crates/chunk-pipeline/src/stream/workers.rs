mod priority;

use std::sync::LazyLock;

use rayon::{ThreadPool, ThreadPoolBuilder};

const DECODE_WORKERS: usize = 1;
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
        let minimum = super::MIN_EFFECTIVE_LIGHT_JOB_CAP + MIN_MESH_WORKERS + DECODE_WORKERS;
        let background = cores.saturating_sub(2).max(minimum);
        let light = (cores / 2).clamp(
            super::MIN_EFFECTIVE_LIGHT_JOB_CAP,
            background - MIN_MESH_WORKERS - DECODE_WORKERS,
        );
        Self {
            light: pool("world-light", light, true),
            mesh: pool("world-mesh", background - light - DECODE_WORKERS, false),
            decode: pool("world-decode", DECODE_WORKERS, false),
        }
    }
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
