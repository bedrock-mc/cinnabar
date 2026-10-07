//! Native biome/season colour policy for leaves.
//! Height/regional climate eligibility still uses the shared, incomplete biome gate.

use assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE;
use chunk_pipeline::WorldStream;
use particles::tiles::{column_exposed, seasonal_tint};

use super::super::{tiles::biome_tint, world_adapter::StreamParticleWorld};

#[cfg(test)]
mod tests;

pub(super) fn leaf_tint(
    stream: &WorldStream,
    world: &StreamParticleWorld<'_>,
    flags: u32,
    block: [i32; 3],
) -> [f32; 4] {
    let center = block.map(|component| component as f32 + 0.5);
    let Some(raw) = stream.camera_biome_id(center) else {
        return biome_tint(stream, flags, block);
    };
    let tints = stream.resolved_biome_tints_snapshot();
    let Some(record) = tints.records.get(tints.dense_index(raw) as usize) else {
        return biome_tint(stream, flags, block);
    };
    if record.flags & BIOME_TINT_FLAG_SEASONAL_FOLIAGE == 0 {
        return biome_tint(stream, flags, block);
    }
    let exposed = leaf_column_exposed(stream, block[1], |y| {
        world.seasonal_cell_shelters([block[0], y, block[2]])
    });
    seasonal_tint(record, flags, exposed)
}

fn leaf_column_exposed(
    stream: &WorldStream,
    start: i32,
    shelters: impl FnMut(i32) -> bool,
) -> bool {
    stream
        .dimension_range(stream.current_dimension())
        .and_then(|range| {
            let minimum = range.base_sub_chunk_y.checked_shl(4)?;
            let maximum = range
                .base_sub_chunk_y
                .checked_add(i32::try_from(range.sub_chunk_count).ok()?)?
                .checked_shl(4)?;
            Some(column_exposed(start, minimum, maximum, shelters))
        })
        .unwrap_or(false)
}
