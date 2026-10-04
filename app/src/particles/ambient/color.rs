//! Native biome/season colour policy for leaves.
//! Height/regional climate eligibility still uses the shared, incomplete biome gate.

use assets::{BIOME_TINT_FLAG_SEASONAL_FOLIAGE, LinearBiomeTints, seasonal_foliage_palette_index};
use client_world::WorldStream;

use super::super::{
    tiles::{biome_tint, linear_to_srgb},
    world_adapter::StreamParticleWorld,
};

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
    let exposed = protocol::vanilla_dimension_range(stream.current_dimension())
        .and_then(|range| {
            let minimum = range.base_sub_chunk_y.checked_shl(4)?;
            let maximum = range
                .base_sub_chunk_y
                .checked_add(i32::try_from(range.sub_chunk_count).ok()?)?
                .checked_shl(4)?;
            Some(column_exposed(block[1], minimum, maximum, |y| {
                world.seasonal_cell_shelters([block[0], y, block[2]])
            }))
        })
        .unwrap_or(false);
    seasonal_tint(record, flags, exposed)
}

fn seasonal_tint(record: &LinearBiomeTints, flags: u32, exposed: bool) -> [f32; 4] {
    let linear = record.seasonal_foliage[seasonal_foliage_palette_index(flags, exposed)];
    [
        linear_to_srgb(linear[0]),
        linear_to_srgb(linear[1]),
        linear_to_srgb(linear[2]),
        1.0,
    ]
}

/// The native height map bounds the upward scan after the last non-air block.
/// Scanning to the bounded dimension roof is equivalent for resident air, and
/// conservatively covers missing cells rather than guessing an unknown height map.
fn column_exposed(
    start: i32,
    minimum: i32,
    maximum: i32,
    mut shelters: impl FnMut(i32) -> bool,
) -> bool {
    start >= minimum && start < maximum && !(start..maximum).any(&mut shelters)
}

#[cfg(test)]
mod tests;
