//! Relighting a few changed cells of an already solved region. The prior field is the solved
//! light of the same blocks except at the changed cells, around the same boundary light, so
//! every other cell keeps the support and the level it had. Only the changed cells, their
//! neighbours and the light that depended on them are revisited, and the result is the one a
//! whole solve gives.

use crate::LightChannel;

use super::{
    cache::{CachedLightBlockAccess, CachedLightReadAccess, DensePositionSet},
    output::{LightSolveOutput, MutableOutput},
    scratch::LightSolverScratch,
    solve::{
        DarkenEntry, IncreaseEntry, NEIGHBOURS, enqueue_counted, local_base, prior_supports_level,
        propagate, read_prior,
    },
    types::{
        BlockPos, DimensionLightProfile, LightBlockAccess, LightBounds, LightReadAccess,
        LightSolveError, LightSolveStats, SolverLimits,
    },
};

/// [`super::solve_light_with_scratch`] for a `prior` that is the solved light of the same blocks
/// except at the `changed` cells, with the same light around the bounds.
#[allow(clippy::too_many_arguments)]
pub fn solve_light_changes_with_scratch<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    generation: u64,
    profile: DimensionLightProfile,
    limits: SolverLimits,
    scratch: &mut LightSolverScratch,
    changed: &[BlockPos],
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

    for (index, position) in bounds.positions().enumerate() {
        if blocks.sample_at_index(index).filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let old = read_prior(prior, index, position, channel)?;
            output.set_at_index(index, channel, old);
        }
    }

    // Away from the changed cells and their neighbours every cell keeps its support, so only
    // those can start darkening.
    let mut candidates = Vec::with_capacity(changed.len() * 7);
    {
        let mut seen = DensePositionSet::new(bounds, volume);
        for &position in changed {
            for offset in std::iter::once([0, 0, 0]).chain(NEIGHBOURS) {
                if let Some(cell) = position.checked_offset(offset)
                    && let Some(index) = output.index(cell)
                    && !seen.contains_at_index(index)
                {
                    seen.insert_at_index(index);
                    candidates.push((index, cell));
                }
            }
        }
    }
    scratch.darken.clear();
    scratch.block_increase.reset(volume);
    scratch.sky_increase.reset(volume);
    let darken = &mut scratch.darken;
    let mut darkened = Vec::new();
    for &(index, position) in &candidates {
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
            darkened.push((index, position));
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
        let entry_index = output
            .index(entry.position)
            .expect("darkened cells stay inside the solve bounds");
        for (direction, offset) in NEIGHBOURS.into_iter().enumerate() {
            let Some((next, index)) = output.neighbour(entry.position, entry_index, direction)
            else {
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
                darkened.push((index, next));
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

    // Elsewhere every level already reaches its own source.
    for &(index, _) in candidates.iter().chain(&darkened) {
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

    // New light can only spread from the changed cells, the darkened cells and the cells
    // around them; every other cell already lit its neighbours as far as they go.
    let mut spreading = DensePositionSet::new(bounds, volume);
    for &(index, _) in &candidates {
        spreading.insert_at_index(index);
    }
    for &(index, position) in &darkened {
        spreading.insert_at_index(index);
        for offset in NEIGHBOURS {
            if let Some(index) = position
                .checked_offset(offset)
                .and_then(|cell| output.index(cell))
            {
                spreading.insert_at_index(index);
            }
        }
    }
    let block_increase = &mut scratch.block_increase;
    let sky_increase = &mut scratch.sky_increase;
    let mut direct_sky = DensePositionSet::new(bounds, volume);
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
            if !spreading.contains_at_index(index) {
                continue;
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
    for (channel, queue) in [
        (LightChannel::Block, block_increase),
        (LightChannel::Sky, sky_increase),
    ] {
        propagate(
            blocks,
            prior,
            bounds,
            channel,
            profile,
            &mut output,
            queue,
            &mut direct_sky,
            limits,
            &mut queued_total,
            &mut stats,
        )?;
    }

    Ok(output.freeze(direct_sky, stats))
}
