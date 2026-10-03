use super::super::*;

const MAX_MESH_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

/// Reserves packed output before jobs enter Rayon; oversized work runs alone.
pub(in crate::stream) struct MeshMemoryBudget {
    pub(in crate::stream) retained: Arc<AtomicU64>,
    bounds: ::meshing::MeshOutputBounds,
}

impl MeshMemoryBudget {
    /// Precomputes model bounds once for the stream's immutable assets.
    pub(in crate::stream) fn new(assets: &RuntimeAssets) -> Self {
        Self {
            retained: Arc::new(AtomicU64::new(0)),
            bounds: ::meshing::MeshOutputBounds::new(assets),
        }
    }

    /// Admits without blocking a worker or discarding a completed current mesh.
    pub(in crate::stream) fn try_admit(
        &self,
        source: &SubChunk,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
    ) -> Option<MeshMemoryPermit> {
        MeshMemoryPermit::reserve(
            &self.retained,
            self.bounds.for_sub_chunk(source, assets, mode)
                + std::mem::size_of::<MeshCompletion>() as u64,
        )
    }
}

/// Retains CPU output credit until publication or the caller releases the mesh change.
#[derive(Debug)]
pub struct MeshMemoryPermit {
    retained: Arc<AtomicU64>,
    bytes: u64,
}

impl MeshMemoryPermit {
    /// Allows one oversized reservation only when no other output is retained.
    fn reserve(retained: &Arc<AtomicU64>, bytes: u64) -> Option<Self> {
        retained
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                let total = used.checked_add(bytes)?;
                (used == 0 || total <= MAX_MESH_OUTPUT_BYTES).then_some(total)
            })
            .ok()?;
        Some(Self {
            retained: Arc::clone(retained),
            bytes,
        })
    }

    /// Returns unused conservative credit after the final packed streams are known.
    pub(in crate::stream) fn reconcile(&mut self, mesh: &ChunkMesh, biome: &PackedBiomeRecord) {
        let actual = ::meshing::mesh_output_byte_len(mesh, biome)
            + std::mem::size_of::<MeshCompletion>() as u64;
        assert!(
            actual <= self.bytes,
            "packed mesh exceeded its conservative output reservation"
        );
        self.retained
            .fetch_sub(self.bytes - actual, Ordering::AcqRel);
        self.bytes = actual;
    }
}

impl Drop for MeshMemoryPermit {
    fn drop(&mut self) {
        self.retained.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_release_on_cancellation_and_allow_one_oversized_output() {
        let retained = Arc::new(AtomicU64::new(0));
        let first = MeshMemoryPermit::reserve(&retained, MAX_MESH_OUTPUT_BYTES).unwrap();
        assert!(MeshMemoryPermit::reserve(&retained, 1).is_none());
        drop(first);
        let large = MeshMemoryPermit::reserve(&retained, MAX_MESH_OUTPUT_BYTES + 1).unwrap();
        assert!(MeshMemoryPermit::reserve(&retained, 1).is_none());
        drop(large);
        assert_eq!(retained.load(Ordering::Acquire), 0);
    }

    /// Compares count-only retention with byte admission for dense synthetic outputs.
    #[test]
    #[ignore = "offline mesh output pressure fixture"]
    fn mesh_output_retention_timing() {
        let payload_bytes = 16 * 1024 * 1024;
        let count_limit = super::super::admission::mesh_job_cap(rayon::current_num_threads());
        let retained = Arc::new(AtomicU64::new(0));
        let before = (0..count_limit)
            .map(|_| vec![7_u8; payload_bytes as usize])
            .collect::<Vec<_>>();
        let before_bytes = before.iter().map(Vec::len).sum::<usize>();
        std::hint::black_box(&before);
        drop(before);
        let started = Instant::now();
        let permits = (0..count_limit)
            .filter_map(|_| MeshMemoryPermit::reserve(&retained, payload_bytes))
            .map(|permit| (permit, vec![7_u8; payload_bytes as usize]))
            .collect::<Vec<_>>();
        std::hint::black_box(&permits);
        println!(
            "mesh_output_retention count_only_bytes={} byte_limited_bytes={} admitted={} allocation_us={}",
            before_bytes,
            retained.load(Ordering::Acquire),
            permits.len(),
            started.elapsed().as_micros()
        );
        assert_eq!(
            retained.load(Ordering::Acquire),
            (count_limit as u64 * payload_bytes).min(MAX_MESH_OUTPUT_BYTES)
        );
        drop(permits);
        assert_eq!(retained.load(Ordering::Acquire), 0);
    }
}
