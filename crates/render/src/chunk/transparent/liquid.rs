use crate::chunk::*;

pub(in crate::chunk) fn transparent_liquid_phase_distance(
    rangefinder: &ViewRangefinder3d,
    key: SubChunkKey,
) -> f32 {
    transparent_model_phase_distance(rangefinder, key)
}

pub(in crate::chunk) fn transparent_frame_draws(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
) -> Vec<(Entity, FrameAllocationIdentity)> {
    let active = arena
        .allocations
        .iter()
        .map(|(&entity, allocation)| (entity, &allocation.gpu));
    let retired = arena
        .retired_allocations
        .iter()
        .map(|allocation| (allocation.entity, &allocation.identity));
    active
        .chain(retired)
        .filter_map(|(entity, allocation)| {
            transparent_snapshot_references_allocation(snapshot, allocation).then_some((
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            ))
        })
        .collect()
}

pub(in crate::chunk) fn transparent_frame_draw_for_range(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
    ref_range: Range<u32>,
) -> Option<(Entity, FrameAllocationIdentity)> {
    let start = usize::try_from(ref_range.start).ok()?;
    let end = usize::try_from(ref_range.end).ok()?;
    let refs = snapshot.refs().get(start..end)?;
    let metadata_index = refs.first()?.metadata_index();
    if refs
        .iter()
        .any(|draw_ref| draw_ref.metadata_index() != metadata_index)
    {
        return None;
    }
    let active = arena
        .allocations
        .iter()
        .map(|(&entity, allocation)| (entity, &allocation.gpu));
    let retired = arena
        .retired_allocations
        .iter()
        .map(|allocation| (allocation.entity, &allocation.identity));
    active
        .chain(retired)
        .find(|(_, allocation)| {
            allocation.metadata_index == metadata_index
                && transparent_snapshot_references_allocation(snapshot, allocation)
        })
        .map(|(entity, allocation)| {
            (
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            )
        })
}
