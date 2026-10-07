use super::super::*;

/// Keeps worker occupancy charged until its result is consumed or discarded.
#[derive(Debug)]
pub(in crate::stream) struct MeshJobPermit {
    occupied: Arc<AtomicUsize>,
}

impl MeshJobPermit {
    /// Reserves one job until its completion leaves the worker-result path.
    pub(in crate::stream) fn new(occupied: &Arc<AtomicUsize>) -> Self {
        occupied.fetch_add(1, Ordering::AcqRel);
        Self {
            occupied: Arc::clone(occupied),
        }
    }
}

impl Drop for MeshJobPermit {
    fn drop(&mut self) {
        self.occupied.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Bounds queued work and retained results using the machine worker count.
pub(in crate::stream) fn mesh_job_cap(worker_threads: usize) -> usize {
    worker_threads
        .saturating_mul(2)
        .clamp(2, WORK_RESULT_CAPACITY)
}

/// Mesh may run on every world worker, so its cap follows the whole pool.
pub(in crate::stream) fn mesh_worker_cap() -> usize {
    mesh_job_cap(workers::WORKERS.size().threads())
}

impl WorldStream {
    /// Stops a superseded job before its next expensive worker phase.
    pub(in crate::stream) fn cancel_mesh_job(&mut self, key: SubChunkKey) {
        if let Some(cancelled) = self.mesh_cancellations.remove(&key) {
            cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_keep_evicted_results_charged_until_drop() {
        let occupied = Arc::new(AtomicUsize::new(0));
        let pending_result = MeshJobPermit::new(&occupied);
        assert_eq!(occupied.load(Ordering::Acquire), 1);
        drop(pending_result);
        assert_eq!(occupied.load(Ordering::Acquire), 0);
    }

    #[test]
    fn admission_scales_with_workers_without_a_large_spawn_backlog() {
        assert_eq!(mesh_job_cap(1), 2);
        assert_eq!(mesh_job_cap(12), 24);
        assert_eq!(mesh_job_cap(32), 64);
        assert_eq!(mesh_job_cap(usize::MAX), WORK_RESULT_CAPACITY);
    }

    /// The cap reads the world pool, not whichever rayon pool the caller runs in.
    #[test]
    fn mesh_cap_reads_the_world_pool() {
        let expected = mesh_job_cap(workers::WORKERS.size().threads());
        let foreign = rayon::ThreadPoolBuilder::new()
            .num_threads(37)
            .build()
            .unwrap();
        assert_eq!(foreign.install(mesh_worker_cap), expected);
        assert_eq!(mesh_worker_cap(), expected);
    }
}
