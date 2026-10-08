use super::super::*;

/// Highest section heights that may carry block or sky light into a column batch.
#[derive(Clone, Copy)]
pub(super) struct PrefixSources {
    block: Option<i32>,
    sky: Option<i32>,
}

impl PrefixSources {
    /// Resident non-air sections and retained non-zero light around every batch member.
    pub(super) fn of(jobs: &[PreparedLightJob]) -> Self {
        let channel = |channel: LightChannel| {
            jobs.iter()
                .flat_map(|job| {
                    job.key.mesh_dependents().filter_map(move |key| {
                        let light = job.prior.light.light(key)?;
                        (!light.channel(channel).is_uniform()
                            || light.get(channel, 0, 0, 0) != Some(0))
                        .then_some(key.y)
                    })
                })
                .max()
        };
        let resident = jobs
            .iter()
            .flat_map(|job| {
                job.blocks.blocks.iter().filter_map(|(key, blocks)| {
                    (!blocks.is_known_air(job.blocks.classifier)).then_some(key.y)
                })
            })
            .max();
        Self {
            block: resident.max(channel(LightChannel::Block)),
            sky: channel(LightChannel::Sky),
        }
    }

    /// An intervening section puts `key` beyond nibble-bounded propagation from every block source.
    pub(super) fn beyond_block_light(self, key: SubChunkKey) -> bool {
        beyond(key, self.block)
    }

    /// Dark air must also lie beyond any skylight that can spread up from the batch below.
    pub(super) fn beyond_sky_light(self, key: SubChunkKey) -> bool {
        beyond(key, self.sky)
    }
}

fn beyond(key: SubChunkKey, highest: Option<i32>) -> bool {
    highest.is_none_or(|highest| {
        key.y
            .checked_sub(highest)
            .is_some_and(|distance| distance >= 2)
    })
}
