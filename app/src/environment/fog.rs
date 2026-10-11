mod bindings;
pub(crate) use bindings::{BiomeProfileIndex, FogBindings, FogProfileIndex};
use chunk_pipeline::WorldStream;
use render::PRECIPITATION_SAMPLE_OFFSETS;

/// The fog and weather renderers use the same native precipitation lattice.
fn sample_positions(position: [f32; 3]) -> [[f32; 3]; PRECIPITATION_SAMPLE_OFFSETS.len()] {
    let origin = position.map(f32::floor);
    PRECIPITATION_SAMPLE_OFFSETS
        .map(|offset| std::array::from_fn(|axis| origin[axis] + offset[axis] as f32))
}

/// Samples the current client's 27-position biome layer.
pub(crate) fn fog_biome_samples(
    stream: &WorldStream,
    position: [f32; 3],
    bindings: &FogBindings,
) -> [Option<FogProfileIndex>; PRECIPITATION_SAMPLE_OFFSETS.len()] {
    sample_positions(position)
        .map(|position| bindings.fog(stream.camera_biome_id(position), stream.current_dimension()))
}

/// Counts admitted samples without renormalizing absent columns.
fn count_precipitation_samples(samples: impl IntoIterator<Item = Option<bool>>) -> Option<usize> {
    let mut known = false;
    let mut count = 0;
    for eligible in samples.into_iter().flatten() {
        known = true;
        count += usize::from(eligible);
    }
    known.then_some(count)
}

/// Samples the session registry on the same bounded lattice as weather.
pub(crate) fn fog_precipitation_count(stream: &WorldStream, position: [f32; 3]) -> Option<usize> {
    let registry = stream.authority().resolved_biome_tints();
    count_precipitation_samples(sample_positions(position).map(|position| {
        let id = stream.camera_biome_id(position)?;
        Some(registry.biome(id)?.precipitation_eligible())
    }))
}

#[cfg(test)]
mod tests;
