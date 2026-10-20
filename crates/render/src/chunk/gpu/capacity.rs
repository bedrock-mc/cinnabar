use crate::chunk::*;
use render_scene::ChunkRenderInstance;

/// Bounds speculative preallocation; a known backlog can request more within adapter limits.
const SPECULATIVE_STREAM_BYTES: u64 = ARENA_MIGRATION_FRAME_BYTES;

/// Sizes one stream for the full backlog and the others for the next upload frame.
pub(super) fn initial_capacity_target(
    backlog: ArenaRequiredLengths,
    mut next_frame: ArenaRequiredLengths,
) -> ArenaRequiredLengths {
    let ArenaRequiredLengths {
        quads,
        geometry_stream_words,
        origins,
        biome_words,
    } = backlog;
    let streams = [
        (&mut next_frame.quads, quads, PACKED_QUAD_BYTES),
        (
            &mut next_frame.geometry_stream_words,
            geometry_stream_words,
            GEOMETRY_STREAM_WORD_BYTES,
        ),
        (&mut next_frame.origins, origins, CHUNK_ORIGIN_BYTES),
        (&mut next_frame.biome_words, biome_words, BIOME_WORD_BYTES),
    ];
    if let Some((initial, wanted, _)) = streams
        .into_iter()
        .max_by_key(|(_, wanted, item_bytes)| buffer_byte_len(*wanted, *item_bytes))
    {
        *initial = wanted;
    }
    next_frame
}

/// Estimates full-view storage from the queued sample while always covering the known backlog.
pub(super) fn backlog_capacity_target<'a>(
    arena: &ChunkGpuArena,
    instances: impl Iterator<Item = &'a ChunkRenderInstance>,
    section_budget: usize,
) -> ArenaRequiredLengths {
    let mut queued = ArenaRequiredLengths::default();
    let mut count = 0_usize;
    for instance in instances {
        let Some(streams) = instance_stream_counts(instance) else {
            continue;
        };
        let Some(words) = streams.shared_word_count() else {
            continue;
        };
        queued.quads = queued.quads.saturating_add(streams.cube as usize);
        queued.geometry_stream_words = queued
            .geometry_stream_words
            .saturating_add(words as usize + SHARED_GEOMETRY_ALIGNMENT_WORDS as usize - 1);
        queued.origins = queued.origins.saturating_add(1);
        if !biome_record_is_fallback(&instance.biome) {
            queued.biome_words = queued
                .biome_words
                .saturating_add(instance.biome.words().len());
        }
        count += 1;
    }
    let target = |used: usize, queued: usize, item_bytes: u64, limit: usize| {
        let estimate = queued.div_ceil(count.max(1)).saturating_mul(section_budget);
        used.saturating_add(queued)
            .max(estimate.min((SPECULATIVE_STREAM_BYTES / item_bytes) as usize))
            .min(limit)
    };
    ArenaRequiredLengths {
        quads: target(
            arena.quad_len,
            queued.quads,
            PACKED_QUAD_BYTES,
            arena.limits.max_quad_items,
        ),
        geometry_stream_words: target(
            arena.geometry_stream_len,
            queued.geometry_stream_words,
            GEOMETRY_STREAM_WORD_BYTES,
            arena.limits.max_geometry_stream_words,
        ),
        origins: target(
            arena.origin_len,
            queued.origins,
            CHUNK_ORIGIN_BYTES,
            arena.limits.max_origin_items,
        ),
        biome_words: target(
            arena.biome_len,
            queued.biome_words,
            BIOME_WORD_BYTES,
            arena.limits.max_biome_words,
        ),
    }
}

/// Converts stream lengths without admitting values outside the arena's address space.
fn instance_stream_counts(instance: &ChunkRenderInstance) -> Option<GeometryStreamCounts> {
    Some(GeometryStreamCounts {
        cube: instance.cube_quads.len().try_into().ok()?,
        cube_lighting: instance.cube_lighting.len().try_into().ok()?,
        model: instance.model_refs.len().try_into().ok()?,
        model_lighting: instance.model_lighting.len().try_into().ok()?,
        model_draw: instance.model_draw_refs.len().try_into().ok()?,
        transparent_model_draw: instance.transparent_model_draw_refs.len().try_into().ok()?,
        liquid: instance.liquid_quads.len().try_into().ok()?,
        liquid_lighting: instance.liquid_lighting.len().try_into().ok()?,
    })
}

/// Chooses a backlog-sized replacement and copies only the old stream's occupied prefix.
pub(super) fn backlog_growth(
    arena: &ChunkGpuArena,
    stream: ArenaStream,
    minimum: ArenaGrowthPlan,
    target: ArenaRequiredLengths,
) -> ArenaGrowthPlan {
    let (used, wanted, item_bytes, limit) = match stream {
        ArenaStream::Quads => (
            arena.quad_len,
            target.quads,
            PACKED_QUAD_BYTES,
            arena.limits.max_quad_items,
        ),
        ArenaStream::GeometryStreams => (
            arena.geometry_stream_len,
            target.geometry_stream_words,
            GEOMETRY_STREAM_WORD_BYTES,
            arena.limits.max_geometry_stream_words,
        ),
        ArenaStream::Origins => (
            arena.origin_len,
            target.origins,
            CHUNK_ORIGIN_BYTES,
            arena.limits.max_origin_items,
        ),
        ArenaStream::Biomes => (
            arena.biome_len,
            target.biome_words,
            BIOME_WORD_BYTES,
            arena.limits.max_biome_words,
        ),
    };
    ArenaGrowthPlan {
        new_capacity: round_arena_capacity(wanted.max(minimum.new_capacity), limit),
        gpu_copy_bytes: buffer_byte_len(used, item_bytes).min(minimum.gpu_copy_bytes),
    }
}

/// Charges the extra mirrored stream write to the same frame budget as the payload.
pub(super) fn mirrored_upload_bytes(
    arena: &ChunkGpuArena,
    streams: GeometryStreamCounts,
    biome_words: u32,
) -> u64 {
    arena
        .migration
        .as_ref()
        .map_or(0, |migration| match migration.stream {
            ArenaStream::Quads => u64::from(streams.cube) * PACKED_QUAD_BYTES,
            ArenaStream::GeometryStreams => {
                u64::from(streams.shared_word_count().unwrap_or(0)) * GEOMETRY_STREAM_WORD_BYTES
            }
            ArenaStream::Origins => CHUNK_ORIGIN_BYTES,
            ArenaStream::Biomes => u64::from(biome_words) * BIOME_WORD_BYTES,
        })
}
