//! What the snapshot sorts: resident water whose faces need order, chosen without the frustum.
//!
//! A sub-chunk's faces sort by camera position alone and each sub-chunk is its own phase
//! item, so a sorted snapshot of every resident stays valid however the camera turns. The
//! queue then draws whichever of its groups are visible.
use super::groups::{TransparentGroupInput, TransparentGroups, build_transparent_group};
use super::state::{TransparentAllocationIdentity, TransparentSortRuntime, ViewSortKey};
use crate::chunk::transparent::face_metric::TransparentFaceMetric;
use crate::chunk::transparent::residents::{TransparentLiquidResident, select_sorted_residents};
use crate::chunk::*;

const CEILING_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// The last manifest and the inputs it was selected from.
#[derive(Debug)]
pub(in crate::chunk) struct TransparentManifest {
    revision: u64,
    include_order_independent: bool,
    tint_identity: ChunkBiomeTintIdentity,
    /// The camera's sub-chunk, kept only while the ref ceiling makes the choice depend on it.
    camera_chunk: Option<[i32; 3]>,
    allocations: Arc<[TransparentAllocationIdentity]>,
    /// Each allocation's sort input, parallel to `allocations`.
    groups: TransparentGroups,
    /// Whether any allocation lies in the cached near box.
    near: Option<(([i32; 3], [i32; 3]), bool)>,
}

/// Whether the view displaces water surfaces, so even flat water can overlap itself on screen.
pub(in crate::chunk) fn view_displaces_water(enhanced: bool) -> bool {
    render_model::enhanced_rendering_enabled() && enhanced
}

/// Builds `resident`'s sort input from its instance, provided the instance still describes
/// exactly the allocation the resident was recorded from.
pub(in crate::chunk) fn build_resident_group(
    resident: &TransparentLiquidResident,
    instances: &Query<&ChunkRenderInstance>,
    arena: &ChunkGpuArena,
    biome_tints: &ChunkBiomeTints,
) -> Option<TransparentGroupInput> {
    let allocation = &arena.allocations.get(&resident.entity)?.gpu;
    let instance = instances.get(resident.entity).ok()?;
    if !transparent_allocation_is_exact(&resident.identity, allocation)
        || !transparent_allocation_matches(instance, allocation, biome_tints.table_identity())
    {
        return None;
    }
    build_transparent_group(instance, resident.identity.clone(), biome_tints).ok()
}

/// Whether every allocation `key` sorts can still be read: a resident of the same key
/// contains it, or a retired allocation matches it exactly.
pub(in crate::chunk) fn sorted_addresses_are_resident(
    key: &ViewSortKey,
    arena: &ChunkGpuArena,
    texture_identity: ChunkTextureAssetIdentity,
    tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    // A reloaded chunk can briefly share its key with an entity that awaits removal.
    let resident = key
        .sorted_allocations
        .iter()
        .filter_map(|identity| arena.transparent_liquids.get(identity.key))
        .map(|resident| resident.entity)
        .chain(arena.pending_removals.iter().copied())
        .filter_map(|entity| arena.allocations.get(&entity))
        .map(|allocation| &allocation.gpu);
    transparent_snapshot_addresses_are_resident(
        key,
        resident,
        arena
            .retired_allocations
            .iter()
            .map(|allocation| &allocation.identity),
        texture_identity,
        tint_identity,
    )
}

impl TransparentSortRuntime {
    /// The resident allocations to sort, in key order.
    ///
    /// The choice is rebuilt only when the residents or tint table change, or, while the ref
    /// ceiling admits only the nearest water, when the camera enters another sub-chunk. A
    /// resident whose sort input cannot be built yet is left to draw unsorted until its
    /// pending upload changes the residents.
    pub(in crate::chunk) fn resident_manifest(
        &mut self,
        arena: &ChunkGpuArena,
        include_order_independent: bool,
        tint_identity: ChunkBiomeTintIdentity,
        camera_chunk: [i32; 3],
        mut build: impl FnMut(&TransparentLiquidResident) -> Option<TransparentGroupInput>,
        metrics: &TransparentSortMetrics,
    ) -> Arc<[TransparentAllocationIdentity]> {
        let revision = arena
            .transparent_liquids
            .revision(include_order_independent);
        if let Some(manifest) = self.manifest.as_ref().filter(|manifest| {
            manifest.revision == revision
                && manifest.include_order_independent == include_order_independent
                && manifest.tint_identity == tint_identity
                && manifest
                    .camera_chunk
                    .is_none_or(|chunk| chunk == camera_chunk)
        }) {
            return Arc::clone(&manifest.allocations);
        }
        let selection = select_sorted_residents(
            arena
                .transparent_liquids
                .sortable(include_order_independent),
            tint_identity,
            camera_chunk,
            self.ref_ceiling,
        );
        // Both lists are in key order, so the previous inputs are reused by a merge.
        let previous = self.manifest.take();
        let (previous_allocations, previous_groups) =
            previous.as_ref().map_or((&[][..], &[][..]), |manifest| {
                (&manifest.allocations[..], &manifest.groups[..])
            });
        let mut cursor = 0;
        let mut allocations = Vec::with_capacity(selection.residents.len());
        let mut groups = Vec::with_capacity(selection.residents.len());
        for resident in &selection.residents {
            let key = resident.identity.key;
            while cursor < previous_allocations.len() && previous_allocations[cursor].key < key {
                cursor += 1;
            }
            let reused = previous_groups
                .get(cursor)
                .filter(|group| {
                    group.identity == resident.identity && group.tint_identity == tint_identity
                })
                .map(Arc::clone);
            let Some(group) = reused.or_else(|| build(resident).map(Arc::new)) else {
                continue;
            };
            allocations.push(resident.identity.clone());
            groups.push(group);
        }
        if selection.excluded != 0 {
            metrics.update(|snapshot| {
                snapshot.ceiling_reject_count = snapshot.ceiling_reject_count.saturating_add(1);
            });
            let now = Instant::now();
            if self
                .last_ceiling_log
                .is_none_or(|last| now.duration_since(last) >= CEILING_LOG_INTERVAL)
            {
                self.last_ceiling_log = Some(now);
                bevy::log::warn!(
                    excluded = selection.excluded,
                    sorted = allocations.len(),
                    "transparent water exceeds the sort ceiling; the farthest sub-chunks draw unsorted"
                );
            }
        }
        let allocations = Arc::<[TransparentAllocationIdentity]>::from(allocations);
        self.manifest = Some(TransparentManifest {
            revision,
            include_order_independent,
            tint_identity,
            camera_chunk: selection.camera_dependent.then_some(camera_chunk),
            allocations: Arc::clone(&allocations),
            groups: groups.into(),
            near: None,
        });
        allocations
    }

    /// The sort inputs of the current manifest, parallel to its allocations.
    pub(in crate::chunk) fn manifest_groups(&self) -> TransparentGroups {
        self.manifest
            .as_ref()
            .map_or_else(|| Arc::from([]), |manifest| Arc::clone(&manifest.groups))
    }

    /// Whether any manifest allocation is near `metric`'s camera, rescanned only when the
    /// near box or the manifest changes.
    pub(in crate::chunk) fn manifest_has_near(&mut self, metric: TransparentFaceMetric) -> bool {
        let Some(manifest) = self.manifest.as_mut() else {
            return false;
        };
        let bounds = metric.near_bounds();
        if let Some((cached, near)) = manifest.near
            && cached == bounds
        {
            return near;
        }
        let near = manifest
            .allocations
            .iter()
            .any(|identity| metric.is_near(identity.key));
        manifest.near = Some((bounds, near));
        near
    }
}
