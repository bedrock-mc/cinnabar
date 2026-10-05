use super::super::*;

/// An intervening air section puts the prefix beyond nibble-bounded block light propagation.
pub(super) fn highest_mixed_block_source(jobs: &[PreparedLightJob]) -> Option<i32> {
    jobs.iter()
        .flat_map(|job| {
            let resident = job.blocks.blocks.iter().filter_map(|(key, blocks)| {
                (!blocks.is_known_air(job.blocks.classifier)).then_some(key.y)
            });
            let retained = job.key.mesh_dependents().filter_map(|key| {
                let light = job.prior.light.light(key)?;
                (!light.channel(LightChannel::Block).is_uniform()
                    || light.get(LightChannel::Block, 0, 0, 0) != Some(0))
                .then_some(key.y)
            });
            resident.chain(retained)
        })
        .max()
}

pub(super) fn above_mixed_block_sources(key: SubChunkKey, highest: Option<i32>) -> bool {
    highest.is_none_or(|highest| {
        key.y
            .checked_sub(highest)
            .is_some_and(|distance| distance >= 2)
    })
}
