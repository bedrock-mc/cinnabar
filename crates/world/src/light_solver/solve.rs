use crate::LightChannel;

use super::{
    cache::{CachedLightBlockAccess, CachedLightReadAccess, DensePositionSet},
    output::{LightSolveOutput, MutableOutput},
    queue::IncreaseQueue,
    scratch::LightSolverScratch,
    types::{
        BlockPos, DimensionLightProfile, LightBlockAccess, LightBounds, LightReadAccess,
        LightSolveError, LightSolveStats, SolverLimits,
    },
};

/// Queued positions retain the dense index already checked for the current solve.
#[derive(Debug, Clone, Copy)]
pub(super) struct IncreaseEntry {
    pub(super) position: BlockPos,
    pub(super) index: usize,
    pub(super) direct_sky: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DarkenEntry {
    pub(super) position: BlockPos,
    channel: LightChannel,
    old_level: u8,
    direct_sky: bool,
}

pub(super) const NEIGHBOURS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

/// Recomputes a bounded region using explicit darken then increase queues.
pub fn solve_light<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    generation: u64,
    profile: DimensionLightProfile,
    limits: SolverLimits,
) -> Result<LightSolveOutput, LightSolveError> {
    solve_light_with_scratch(
        blocks,
        prior,
        bounds,
        generation,
        profile,
        limits,
        &mut LightSolverScratch::default(),
    )
}

/// Recomputes light while retaining bounded intermediate buffers for the worker's next solve.
#[allow(clippy::too_many_arguments)]
pub fn solve_light_with_scratch<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    generation: u64,
    profile: DimensionLightProfile,
    limits: SolverLimits,
    scratch: &mut LightSolverScratch,
) -> Result<LightSolveOutput, LightSolveError> {
    let volume = bounds.volume().ok_or(LightSolveError::VoxelLimitExceeded {
        requested: usize::MAX,
        max: limits.max_voxels,
    })?;
    if volume > limits.max_voxels {
        return Err(LightSolveError::VoxelLimitExceeded {
            requested: volume,
            max: limits.max_voxels,
        });
    }

    let cached_blocks = CachedLightBlockAccess::new(blocks, bounds, volume, &mut scratch.blocks);
    let blocks = &cached_blocks;
    let cached_prior = CachedLightReadAccess::new(prior, bounds, volume, &mut scratch.prior);
    let prior = &cached_prior;
    let mut output = MutableOutput::new(bounds, generation, volume, &mut scratch.output);
    let mut stats = LightSolveStats::default();
    let mut queued_total = 0_usize;

    // Load the previous bounded field first. The darken queue is seeded only
    // at values that no current source or neighbour can support; each entry
    // carries its old level and propagates removal through dependent values.
    scratch.darken.clear();
    scratch.block_increase.reset(volume);
    scratch.sky_increase.reset(volume);
    let darken = &mut scratch.darken;
    for (index, position) in bounds.positions().enumerate() {
        let sample = blocks.sample_at_index(index);
        if sample.filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let old = read_prior(prior, index, position, channel)?;
            output.set_at_index(index, channel, old);
        }
    }

    for (index, position) in bounds.positions().enumerate() {
        if blocks.sample_at_index(index).filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let old = output.get_at_index(index, channel);
            let base = local_base(blocks, index, channel, profile)?;
            if old == 0
                || prior_supports_level(
                    blocks, prior, bounds, position, index, channel, profile, old,
                )?
            {
                continue;
            }
            output.set_at_index(index, channel, base);
            enqueue_counted(&mut queued_total, 1, limits.max_queue_entries)?;
            darken.push_back(DarkenEntry {
                position,
                channel,
                old_level: old,
                direct_sky: channel == LightChannel::Sky
                    && prior.direct_sky_at_index(index, position),
            });
            stats.darken_seeded += 1;
        }
    }
    stats.queue_peak = stats.queue_peak.max(darken.len());
    while let Some(entry) = darken.pop_front() {
        stats.darken_dequeued += 1;
        for offset in NEIGHBOURS {
            let Some(next) = entry.position.checked_offset(offset) else {
                continue;
            };
            let Some(index) = output.index(next) else {
                continue;
            };
            if blocks.sample_at_index(index).filter().is_none() {
                continue;
            }
            let current = output.get_at_index(index, entry.channel);
            let base = local_base(blocks, index, entry.channel, profile)?;
            let depended_on_removed = current < entry.old_level
                || (entry.channel == LightChannel::Sky
                    && profile.direct_sky_down()
                    && entry.direct_sky
                    && offset == [0, -1, 0]
                    && current == 15
                    && entry.old_level == 15);
            if current > base && depended_on_removed {
                output.set_at_index(index, entry.channel, base);
                enqueue_counted(&mut queued_total, 1, limits.max_queue_entries)?;
                darken.push_back(DarkenEntry {
                    position: next,
                    channel: entry.channel,
                    old_level: current,
                    direct_sky: entry.channel == LightChannel::Sky
                        && prior.direct_sky_at_index(index, next),
                });
                stats.queue_peak = stats.queue_peak.max(darken.len());
            }
        }
    }

    for index in 0..volume {
        if blocks.sample_at_index(index).filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let base = local_base(blocks, index, channel, profile)?;
            if base > output.get_at_index(index, channel) {
                output.set_at_index(index, channel, base);
            }
        }
    }

    let block_increase = &mut scratch.block_increase;
    let sky_increase = &mut scratch.sky_increase;
    let mut direct_sky = DensePositionSet::new(bounds, volume);
    // The bounds iterator has the same dense order as the cache and output buffers.
    for (index, position) in bounds.positions().enumerate() {
        let Some(filter) = blocks.sample_at_index(index).filter() else {
            continue;
        };
        for (channel, queue) in [
            (LightChannel::Block, &mut *block_increase),
            (LightChannel::Sky, &mut *sky_increase),
        ] {
            let level = output.get_at_index(index, channel);
            if level == 0 {
                continue;
            }
            let is_direct = channel == LightChannel::Sky
                && profile.direct_sky_down()
                && level == 15
                && filter == 0
                && (local_base(blocks, index, channel, profile)? == 15
                    || prior.direct_sky_at_index(index, position));
            if is_direct {
                direct_sky.insert_at_index(index);
            }
            queue.push_back(
                IncreaseEntry {
                    position,
                    index,
                    direct_sky: is_direct,
                },
                &mut queued_total,
                limits.max_queue_entries,
            )?;
        }
    }
    propagate(
        blocks,
        prior,
        bounds,
        LightChannel::Block,
        profile,
        &mut output,
        block_increase,
        &mut direct_sky,
        limits,
        &mut queued_total,
        &mut stats,
    )?;
    propagate(
        blocks,
        prior,
        bounds,
        LightChannel::Sky,
        profile,
        &mut output,
        sky_increase,
        &mut direct_sky,
        limits,
        &mut queued_total,
        &mut stats,
    )?;

    Ok(output.freeze(direct_sky, stats))
}

/// Validates a lazily cached raw prior value at an already mapped interior cell.
fn read_prior<P: LightReadAccess>(
    prior: &CachedLightReadAccess<'_, P>,
    index: usize,
    position: BlockPos,
    channel: LightChannel,
) -> Result<u8, LightSolveError> {
    let value = prior.read_at_index(index, position, channel);
    if value > 15 {
        Err(LightSolveError::LightValueOutOfRange { value })
    } else {
        Ok(value)
    }
}

/// Reads local emission and validates sky seeds without remapping an interior cell.
fn local_base<A: LightBlockAccess>(
    blocks: &CachedLightBlockAccess<'_, A>,
    index: usize,
    channel: LightChannel,
    profile: DimensionLightProfile,
) -> Result<u8, LightSolveError> {
    let sample = blocks.sample_at_index(index);
    let Some(filter) = sample.filter() else {
        return Ok(0);
    };
    match channel {
        LightChannel::Block => Ok(sample.emission()),
        LightChannel::Sky => {
            let seed = blocks.sky_seed_at_index(index);
            if seed > 15 {
                return Err(LightSolveError::LightValueOutOfRange { value: seed });
            }
            Ok(if profile.allows_sky() {
                seed.saturating_sub(filter)
            } else {
                0
            })
        }
    }
}

/// Stops once cached neighbour support reaches the retained light level.
#[allow(clippy::too_many_arguments)]
fn prior_supports_level<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &CachedLightBlockAccess<'_, A>,
    prior: &CachedLightReadAccess<'_, P>,
    bounds: LightBounds,
    position: BlockPos,
    index: usize,
    channel: LightChannel,
    profile: DimensionLightProfile,
    required: u8,
) -> Result<bool, LightSolveError> {
    let Some(filter) = blocks.sample_at_index(index).filter() else {
        return Ok(false);
    };
    if local_base(blocks, index, channel, profile)? >= required {
        return Ok(true);
    }
    if channel == LightChannel::Sky && !profile.allows_sky() {
        return Ok(false);
    }
    // Direct sky commonly has its full support immediately above the retained cell.
    let neighbours = if channel == LightChannel::Sky {
        [
            NEIGHBOURS[3],
            NEIGHBOURS[0],
            NEIGHBOURS[1],
            NEIGHBOURS[2],
            NEIGHBOURS[4],
            NEIGHBOURS[5],
        ]
    } else {
        NEIGHBOURS
    };
    for offset in neighbours {
        let Some(neighbour) = position.checked_offset(offset) else {
            continue;
        };
        let neighbour_index = blocks.index(neighbour);
        let sample = neighbour_index.map_or_else(
            || blocks.sample(neighbour),
            |index| blocks.sample_at_index(index),
        );
        if sample.filter().is_none() {
            continue;
        }
        let (neighbour_level, direct_sky) = if let Some(index) = neighbour_index {
            (
                read_prior(prior, index, neighbour, channel)?,
                channel == LightChannel::Sky && prior.direct_sky_at_index(index, neighbour),
            )
        } else {
            let Some((level, direct_sky)) = prior
                .boundary_light(bounds.dimension, neighbour, channel)
                .trusted_parts()
            else {
                continue;
            };
            (level, direct_sky)
        };
        if incoming_level(
            neighbour_level,
            filter,
            channel,
            profile,
            offset,
            direct_sky,
        ) >= required
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn incoming_level(
    source: u8,
    destination_filter: u8,
    channel: LightChannel,
    profile: DimensionLightProfile,
    destination_to_source: [i32; 3],
    source_is_direct_sky: bool,
) -> u8 {
    if channel == LightChannel::Sky
        && profile.direct_sky_down()
        && destination_to_source == [0, 1, 0]
        && source == 15
        && source_is_direct_sky
        && destination_filter == 0
    {
        15
    } else {
        source.saturating_sub(destination_filter.max(1))
    }
}

#[allow(clippy::too_many_arguments)]
fn propagate<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &CachedLightBlockAccess<'_, A>,
    prior: &P,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    output: &mut MutableOutput,
    queue: &mut IncreaseQueue,
    direct_positions: &mut DensePositionSet,
    limits: SolverLimits,
    queued_total: &mut usize,
    stats: &mut LightSolveStats,
) -> Result<(), LightSolveError> {
    seed_boundary_from_halo(
        blocks,
        prior,
        bounds,
        channel,
        profile,
        output,
        queue,
        direct_positions,
        limits,
        queued_total,
    )?;
    stats.queue_peak = stats.queue_peak.max(queue.len());

    while let Some(entry) = queue.pop_front() {
        stats.increase_dequeued += 1;
        let source = output.get_at_index(entry.index, channel);
        for offset in NEIGHBOURS {
            let Some(next) = entry.position.checked_offset(offset) else {
                continue;
            };
            let Some(index) = output.index(next) else {
                continue;
            };
            let Some(filter) = blocks.sample_at_index(index).filter() else {
                continue;
            };
            let continues_direct = channel == LightChannel::Sky
                && profile.direct_sky_down()
                && entry.direct_sky
                && offset == [0, -1, 0]
                && source == 15
                && filter == 0;
            let candidate = if continues_direct {
                15
            } else {
                source.saturating_sub(filter.max(1))
            };
            let current = output.get_at_index(index, channel);
            let gains_direct = continues_direct && !direct_positions.contains_at_index(index);
            if candidate > current || (candidate == current && gains_direct) {
                if candidate > current {
                    output.set_at_index(index, channel, candidate);
                }
                if continues_direct {
                    direct_positions.insert_at_index(index);
                }
                queue.push_back(
                    IncreaseEntry {
                        position: next,
                        index,
                        direct_sky: continues_direct,
                    },
                    queued_total,
                    limits.max_queue_entries,
                )?;
                stats.queue_peak = stats.queue_peak.max(queue.len());
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn seed_boundary_from_halo<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    output: &mut MutableOutput,
    queue: &mut IncreaseQueue,
    direct_positions: &mut DensePositionSet,
    limits: SolverLimits,
    queued_total: &mut usize,
) -> Result<(), LightSolveError> {
    if channel == LightChannel::Sky && !profile.allows_sky() {
        return Ok(());
    }
    for position in bounds.positions() {
        if position.x != bounds.min.x
            && position.x != bounds.max.x
            && position.y != bounds.min.y
            && position.y != bounds.max.y
            && position.z != bounds.min.z
            && position.z != bounds.max.z
        {
            continue;
        }
        let Some(filter) = blocks.sample(position).filter() else {
            continue;
        };
        let mut candidate = 0;
        let mut candidate_is_direct = false;
        for offset in NEIGHBOURS {
            let Some(neighbour) = position.checked_offset(offset) else {
                continue;
            };
            if bounds.contains(neighbour) || blocks.sample(neighbour).filter().is_none() {
                continue;
            }
            let Some((prior_level, boundary_is_direct)) = prior
                .boundary_light(bounds.dimension, neighbour, channel)
                .trusted_parts()
            else {
                continue;
            };
            if prior_level == 0 {
                continue;
            }
            let direct = channel == LightChannel::Sky
                && profile.direct_sky_down()
                && offset == [0, 1, 0]
                && prior_level == 15
                && boundary_is_direct
                && filter == 0;
            let incoming = if direct {
                15
            } else {
                prior_level.saturating_sub(filter.max(1))
            };
            if incoming > candidate || (incoming == candidate && direct) {
                candidate = incoming;
                candidate_is_direct = direct;
            }
        }
        let index = output
            .index(position)
            .expect("boundary positions stay inside the solve bounds");
        let current = output.get_at_index(index, channel);
        let gains_direct = candidate_is_direct && !direct_positions.contains(&position);
        if candidate > current || (candidate == current && candidate != 0 && gains_direct) {
            if candidate > current {
                output.set_at_index(index, channel, candidate);
            }
            if candidate_is_direct {
                direct_positions.insert(position);
            }
            queue.push_back(
                IncreaseEntry {
                    position,
                    index,
                    direct_sky: candidate_is_direct,
                },
                queued_total,
                limits.max_queue_entries,
            )?;
        }
    }
    Ok(())
}

pub(super) fn enqueue_counted(
    total: &mut usize,
    amount: usize,
    max: usize,
) -> Result<(), LightSolveError> {
    *total = total.saturating_add(amount);
    if *total > max {
        Err(LightSolveError::QueueLimitExceeded { max })
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "support_tests.rs"]
mod tests;
