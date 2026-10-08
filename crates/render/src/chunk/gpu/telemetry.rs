//! Capacity-change telemetry, not a per-frame allocation or driver-memory estimate.

use crate::chunk::*;

pub(in crate::chunk::gpu) fn log_initial_arena_capacity(
    arena: &ChunkGpuArena,
    render_device: &RenderDevice,
) {
    let limits = render_device.limits();
    bevy::log::info!(
        max_buffer_size = limits.max_buffer_size,
        max_storage_buffer_binding_size = limits.max_storage_buffer_binding_size,
        "chunk GPU arena device limits (per buffer, not an aggregate memory budget)"
    );
    log_arena_capacity(arena, "initialized");
}

/// Logs only creation or capacity changes. Reclaimed ranges do not shrink buffers.
pub(in crate::chunk) fn log_arena_capacity(arena: &ChunkGpuArena, event: &'static str) {
    let quad_bytes = arena.quad_buffer.size();
    let geometry_bytes = arena.geometry_stream_buffer.size();
    let origin_bytes = arena.origin_buffer.size();
    let biome_bytes = arena.biome_buffer.size();
    let auxiliary_bytes = total_capacity_bytes([
        arena.index_buffer.size(),
        arena.model_index_buffer.size(),
        arena.indirect_buffer.size(),
        arena.transparent_indirect_buffer.size(),
        arena.transparent_ref_buffer.size(),
    ]);
    let migration_bytes = arena
        .migration
        .as_ref()
        .map_or(0, |migration| migration.buffer.size());
    let tracked_capacity_bytes = total_capacity_bytes([
        quad_bytes,
        geometry_bytes,
        origin_bytes,
        biome_bytes,
        auxiliary_bytes,
        migration_bytes,
    ]);
    bevy::log::info!(
        event,
        quad_bytes,
        geometry_bytes,
        origin_bytes,
        biome_bytes,
        auxiliary_bytes,
        migration_bytes,
        tracked_capacity_bytes,
        allocations = arena.allocations.len(),
        retired_allocations = arena.retired_allocations.len(),
        retired_bytes = arena.retirement_budget.bytes,
        pending_removals = arena.pending_removals.len(),
        "chunk GPU arena capacity changed (excludes driver-retained submitted resources)"
    );
}

fn total_capacity_bytes<const N: usize>(bytes: [u64; N]) -> u64 {
    bytes.into_iter().fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_accounting_includes_migration_without_overflow() {
        assert_eq!(total_capacity_bytes([8, 16, 32, 64, 128, 256]), 504);
        assert_eq!(total_capacity_bytes([u64::MAX, 1]), u64::MAX);
    }
}
