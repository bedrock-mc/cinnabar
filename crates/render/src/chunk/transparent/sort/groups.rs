//! Per-sub-chunk water face orders, cached by allocation identity and face-order class.
//!
//! Each sub-chunk is drawn by its own phase item, so the snapshot lays groups out in key
//! order and only a group whose class or mesh changed is sorted again.
use super::state::{
    TransparentAllocationIdentity, TransparentSortError, TransparentSortWork,
    TransparentWorkerResult, changed_ref_spans,
};
use super::{MAX_TRANSPARENT_DRAW_REFS, PackedTransparentDrawRef};
use crate::chunk::transparent::face_metric::{FaceOrderClass, TransparentFaceMetric};
use crate::chunk::*;

/// The visible sub-chunks' sort inputs, in committed layout order.
pub(in crate::chunk) type TransparentGroups = Arc<[Arc<TransparentGroupInput>]>;

/// One visible sub-chunk's sort input; rebuilt only when its allocation or tint table changes.
#[derive(Debug)]
pub(in crate::chunk) struct TransparentGroupInput {
    pub(in crate::chunk) identity: TransparentAllocationIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    /// Indexed by local quad index.
    pub(in crate::chunk) centroids: Box<[Vec3]>,
    /// Sorted distinct water tint colours, for the diagnostic tint count.
    pub(in crate::chunk) tint_colors: Box<[[u32; 3]]>,
}

/// A sub-chunk's back-to-front order, valid while its allocation and class are unchanged.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::chunk) struct TransparentGroupOrder {
    pub(in crate::chunk) identity: TransparentAllocationIdentity,
    pub(in crate::chunk) class: FaceOrderClass,
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentGroupSort {
    pub(in crate::chunk) refs: Vec<PackedTransparentDrawRef>,
    /// Orders that were sorted for this result rather than reused.
    pub(in crate::chunk) fresh: Vec<TransparentGroupOrder>,
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

fn sort_group(
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

/// Concatenates group orders in input order, sorting only groups without a valid cached order.
pub(in crate::chunk) fn sort_transparent_groups(
    camera: Vec3,
    groups: &[Arc<TransparentGroupInput>],
    cached: &[Option<TransparentGroupOrder>],
) -> TransparentGroupSort {
    let metric = TransparentFaceMetric::new(camera);
    let mut sorted = TransparentGroupSort {
        refs: Vec::with_capacity(groups.iter().map(|group| group.centroids.len()).sum()),
        fresh: Vec::new(),
    };
    for (index, group) in groups.iter().enumerate() {
        let class = metric.class(group.identity.key);
        if let Some(Some(order)) = cached.get(index)
            && order.class == class
            && order.identity == group.identity
        {
            sorted.refs.extend_from_slice(&order.refs);
            continue;
        }
        let refs = sort_group(metric, group);
        sorted.refs.extend_from_slice(&refs);
        sorted.fresh.push(TransparentGroupOrder {
            identity: group.identity.clone(),
            class,
            refs,
        });
    }
    sorted
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
        let sorted = sort_transparent_groups(work.camera, &work.groups, &work.cached);
        let refs = Arc::<[PackedTransparentDrawRef]>::from(sorted.refs);
        let patch = work
            .base
            .filter(|base| base.len() == refs.len())
            .map(|base| {
                let spans = changed_ref_spans(&base, &refs);
                (base, spans)
            });
        let _ = sender.try_send(TransparentWorkerResult {
            generation: work.generation,
            requested_at: work.requested_at,
            key: work.key,
            refs: Ok(refs),
            patch,
            fresh: sorted.fresh,
            cpu_duration: started.elapsed(),
            distinct_tint_count: work.distinct_tint_count,
        });
    });
}
