//! Per-sub-chunk water face orders.
//!
//! Each sub-chunk is drawn by its own phase item, so only a group whose class or mesh
//! changed is sorted again; the rest keep their range of the committed slot.
use super::layout::plan_transparent_slot;
use super::state::{
    TransparentAllocationIdentity, TransparentSortError, TransparentSortWork,
    TransparentWorkerResult,
};
use super::{MAX_TRANSPARENT_DRAW_REFS, PackedTransparentDrawRef};
use crate::chunk::transparent::face_metric::TransparentFaceMetric;
use crate::chunk::*;

/// The manifest's sort inputs, parallel to its key-sorted allocations.
pub(in crate::chunk) type TransparentGroups = Arc<[Arc<TransparentGroupInput>]>;

/// One sub-chunk's sort input; rebuilt only when its allocation or tint table changes.
#[derive(Debug)]
pub(in crate::chunk) struct TransparentGroupInput {
    pub(in crate::chunk) identity: TransparentAllocationIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    /// Indexed by local quad index.
    pub(in crate::chunk) centroids: Box<[Vec3]>,
    /// Sorted distinct water tint colours, for the diagnostic tint count.
    pub(in crate::chunk) tint_colors: Box<[[u32; 3]]>,
}

pub(in crate::chunk) fn build_transparent_group(
    instance: &ChunkRenderInstance,
    identity: TransparentAllocationIdentity,
    biome_tints: &ChunkBiomeTints,
) -> Result<TransparentGroupInput, TransparentSortError> {
    let transparent_end = instance
        .depth_liquid_start
        .map_or(instance.liquid_quads.len(), |start| start as usize);
    let quads = instance
        .liquid_quads
        .get(..transparent_end)
        .unwrap_or_default();
    let ceiling = TransparentSortError::ReferenceCeiling {
        requested: quads.len(),
        ceiling: MAX_TRANSPARENT_DRAW_REFS,
    };
    if quads.len() > MAX_TRANSPARENT_DRAW_REFS
        || (identity.liquid_range.start / 4)
            .checked_add(u32::try_from(quads.len()).map_err(|_| ceiling)?)
            .is_none()
    {
        return Err(ceiling);
    }
    let mut tint_colors = Vec::new();
    let centroids = quads
        .iter()
        .map(|&quad| {
            let local = quad.origin();
            if let Some(tint_index) = instance.biome.tint_index(local[0], local[1], local[2])
                && let Some(tint) = biome_tints.entries().get(tint_index as usize)
            {
                tint_colors.push(tint.water.map(f32::to_bits));
            }
            Vec3::from_array(liquid_quad_centroid(instance.origin, quad))
        })
        .collect();
    tint_colors.sort_unstable();
    tint_colors.dedup();
    Ok(TransparentGroupInput {
        identity,
        tint_identity: biome_tints.table_identity(),
        centroids,
        tint_colors: tint_colors.into(),
    })
}

/// Distinct water tints across groups that each carry a sorted, deduplicated set.
pub(in crate::chunk) fn distinct_tint_count(groups: &[Arc<TransparentGroupInput>]) -> usize {
    let mut colors = groups
        .iter()
        .flat_map(|group| group.tint_colors.iter().copied())
        .collect::<Vec<_>>();
    colors.sort_unstable();
    colors.dedup();
    colors.len()
}

/// One sub-chunk's faces back to front for `metric`'s camera, as absolute draw refs.
pub(in crate::chunk) fn sort_group(
    metric: TransparentFaceMetric,
    group: &TransparentGroupInput,
) -> Arc<[PackedTransparentDrawRef]> {
    let metric = metric.for_chunk(group.identity.key);
    let mut keyed = group
        .centroids
        .iter()
        .enumerate()
        .map(|(index, &centroid)| (metric.distance(centroid), index as u32))
        .collect::<Vec<_>>();
    // Local indices are unique, so the unstable sort is fully determined.
    keyed.sort_unstable_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));
    let record_start = group.identity.liquid_range.start / 4;
    keyed
        .into_iter()
        .map(|(_, index)| {
            PackedTransparentDrawRef::new(record_start + index, group.identity.metadata_index)
        })
        .collect()
}

pub(in crate::chunk) fn spawn_transparent_sort(
    sender: SyncSender<TransparentWorkerResult>,
    work: TransparentSortWork,
    profiler: Option<RuntimeStageProfiler>,
) {
    rayon::spawn(move || {
        let _timer = profiler
            .as_ref()
            .map(|profiler| profiler.time(RuntimeStage::TransparentWorker));
        let started = Instant::now();
        let output = plan_transparent_slot(
            work.camera,
            &work.key.sorted_allocations,
            &work.groups,
            work.base.as_ref(),
            work.upload_cap,
        );
        let _ = sender.try_send(TransparentWorkerResult {
            generation: work.generation,
            requested_at: work.requested_at,
            key: work.key,
            output: Ok(output),
            cpu_duration: started.elapsed(),
            distinct_tint_count: distinct_tint_count(&work.groups),
        });
    });
}
