//! A full solve: light computed from the blocks, sky seeds and trusted halo alone. Light is a
//! fixed point of those inputs, so this equals [`super::solve_light_with_scratch`] for any prior,
//! without reading or darkening the retained interior light, which costs more than solving afresh.
//!
//! Cells live in a dense grid padded by one blocked cell on every side, so neighbour steps need
//! no bounds checks. Direct sky is one top-down pass per column; the rest of each channel spreads
//! from 15 down through per-level buckets, so a cell spreads once, at its final level.

use crate::{BLOCKS_PER_SUB_CHUNK, LightChannel, SUB_CHUNK_SIDE, SubChunkKey};

use super::{
    cache::DensePositionSet,
    output::{LightSolveOutput, MutableOutput},
    scratch::LightSolverScratch,
    solve::enqueue_counted,
    types::{
        BlockPos, DimensionLightProfile, LightBlockAccess, LightBlockSample, LightBounds,
        LightReadAccess, LightSolveError, LightSolveStats, SolverLimits, light_axis_len,
    },
};

/// Filter of a padded cell light never enters: outside the bounds, or an unknown block.
const BLOCKED: u8 = u8::MAX;

/// Light levels; a cell at level 1 or 0 lights no neighbour.
const LEVELS: usize = 16;

/// Worker-local grids and buckets for [`solve_light_full_with_scratch`].
#[derive(Default)]
pub(super) struct FullSolveScratch {
    filter: Vec<u8>,
    block: Vec<u8>,
    sky: Vec<u8>,
    direct: Vec<bool>,
    /// Per column of the top face: trusted direct sky enters it from above.
    direct_above: Vec<bool>,
    buckets: [Vec<u32>; LEVELS],
    /// One section's samples, from a source's whole-section read.
    section: Vec<LightBlockSample>,
}

/// The padded grid's shape: in-bounds extents and the index steps along each axis.
#[derive(Clone, Copy)]
struct Grid {
    x_len: usize,
    y_len: usize,
    z_len: usize,
    x_step: usize,
    y_step: usize,
}

impl Grid {
    fn new(bounds: LightBounds) -> Self {
        let x_len = light_axis_len(bounds.min().x, bounds.max().x);
        let y_len = light_axis_len(bounds.min().y, bounds.max().y);
        let z_len = light_axis_len(bounds.min().z, bounds.max().z);
        let y_step = z_len + 2;
        Self {
            x_len,
            y_len,
            z_len,
            x_step: (y_len + 2) * y_step,
            y_step,
        }
    }

    fn volume(self) -> usize {
        (self.x_len + 2) * self.x_step
    }

    /// The padded index of in-bounds cell `[x, y, z]`, counted from the bounds' minimum.
    #[inline]
    fn index(self, x: usize, y: usize, z: usize) -> usize {
        (x + 1) * self.x_step + (y + 1) * self.y_step + z + 1
    }

    /// The world position of in-bounds cell `[x, y, z]`, counted from the bounds' minimum.
    #[inline]
    fn position(self, bounds: LightBounds, x: usize, y: usize, z: usize) -> BlockPos {
        let min = bounds.min();
        // In-bounds offsets keep every coordinate within the bounds, so none wraps.
        BlockPos::new(
            min.x.wrapping_add(x as i32),
            min.y.wrapping_add(y as i32),
            min.z.wrapping_add(z as i32),
        )
    }

    /// Index offsets to the six face neighbours, in [`super::solve::NEIGHBOURS`] order.
    fn steps(self) -> [isize; 6] {
        let (x, y) = (self.x_step as isize, self.y_step as isize);
        [-x, x, -y, y, -1, 1]
    }
}

/// Solves `bounds` from `blocks` and the trusted halo of `prior`, ignoring `prior`'s interior
/// light; the result matches [`super::solve_light_with_scratch`] with the same inputs.
#[allow(clippy::too_many_arguments)]
pub fn solve_light_full_with_scratch<A: LightBlockAccess, P: LightReadAccess>(
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
    let grid = Grid::new(bounds);
    let full = &mut scratch.full;
    reset(&mut full.filter, grid.volume(), BLOCKED);
    reset(&mut full.block, grid.volume(), 0);
    reset(&mut full.sky, grid.volume(), 0);
    reset(&mut full.direct, grid.volume(), false);
    reset(&mut full.direct_above, grid.x_len * grid.z_len, false);

    read_blocks(blocks, bounds, grid, profile, full)?;
    seed_halo(blocks, prior, bounds, grid, profile, full);
    if profile.direct_sky_down() {
        fill_direct_sky(grid, full);
    }

    let mut stats = LightSolveStats::default();
    let mut queued_total = 0_usize;
    let FullSolveScratch {
        filter,
        block,
        sky,
        buckets,
        ..
    } = full;
    spread(
        grid,
        filter,
        block,
        buckets,
        limits,
        &mut queued_total,
        &mut stats,
    )?;
    if profile.allows_sky() {
        spread(
            grid,
            filter,
            sky,
            buckets,
            limits,
            &mut queued_total,
            &mut stats,
        )?;
    }

    let mut output = MutableOutput::new(bounds, generation, volume, &mut scratch.output);
    let mut direct_sky = DensePositionSet::new(bounds, volume);
    let full = &scratch.full;
    let mut dense = 0;
    for x in 0..grid.x_len {
        for y in 0..grid.y_len {
            let row = grid.index(x, y, 0);
            for cell in row..row + grid.z_len {
                if full.filter[cell] != BLOCKED {
                    output.set_at_index(dense, LightChannel::Block, full.block[cell]);
                    output.set_at_index(dense, LightChannel::Sky, full.sky[cell]);
                    if full.direct[cell] {
                        direct_sky.insert_at_index(dense);
                    }
                }
                dense += 1;
            }
        }
    }
    Ok(output.freeze(direct_sky, stats))
}

/// Empties `values` and refills it with `volume` copies of `fill`, keeping its allocation.
fn reset<T: Copy>(values: &mut Vec<T>, volume: usize, fill: T) {
    values.clear();
    values.resize(volume, fill);
}

/// Copies each known cell's filter, emission and sky seed less its filter into the grid, a
/// section at a time through the source's whole-section read where it has one.
fn read_blocks<A: LightBlockAccess>(
    blocks: &A,
    bounds: LightBounds,
    grid: Grid,
    profile: DimensionLightProfile,
    full: &mut FullSolveScratch,
) -> Result<(), LightSolveError> {
    let side = SUB_CHUNK_SIDE as i64;
    let (min, max) = (bounds.min(), bounds.max());
    let lower = |value: i32| i64::from(value).div_euclid(side);
    full.section.clear();
    full.section
        .resize(BLOCKS_PER_SUB_CHUNK, LightBlockSample::Unknown);
    let section: &mut [LightBlockSample; BLOCKS_PER_SUB_CHUNK] = (&mut full.section[..])
        .try_into()
        .expect("the section buffer holds one sub-chunk");
    for section_x in lower(min.x)..=lower(max.x) {
        for section_y in lower(min.y)..=lower(max.y) {
            for section_z in lower(min.z)..=lower(max.z) {
                // Section indices of in-bounds blocks fit an i32 sub-chunk coordinate.
                let key = SubChunkKey::new(
                    bounds.dimension(),
                    section_x as i32,
                    section_y as i32,
                    section_z as i32,
                );
                let bulk = blocks.sample_section(key, section);
                let origin = [section_x * side, section_y * side, section_z * side];
                let clip = |axis: usize, low: i32, high: i32| {
                    i64::from(low).max(origin[axis])..=i64::from(high).min(origin[axis] + side - 1)
                };
                for world_x in clip(0, min.x, max.x) {
                    for world_y in clip(1, min.y, max.y) {
                        for world_z in clip(2, min.z, max.z) {
                            // Every clipped coordinate lies within the bounds.
                            let position =
                                BlockPos::new(world_x as i32, world_y as i32, world_z as i32);
                            let sample = if bulk {
                                let local =
                                    |value: i64, axis: usize| (value - origin[axis]) as usize;
                                section[local(world_x, 0) << 8
                                    | local(world_z, 2) << 4
                                    | local(world_y, 1)]
                            } else {
                                blocks.sample(position)
                            };
                            let Some(filter) = sample.filter() else {
                                continue;
                            };
                            let cell = grid.index(
                                (world_x - i64::from(min.x)) as usize,
                                (world_y - i64::from(min.y)) as usize,
                                (world_z - i64::from(min.z)) as usize,
                            );
                            full.filter[cell] = filter;
                            full.block[cell] = sample.emission();
                            let seed = blocks.sky_seed(position);
                            if seed > 15 {
                                return Err(LightSolveError::LightValueOutOfRange { value: seed });
                            }
                            if profile.allows_sky() {
                                full.sky[cell] = seed.saturating_sub(filter);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Raises each face cell to the light its trusted halo neighbours send in, recording the top
/// face's columns that direct sky enters from above.
fn seed_halo<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    grid: Grid,
    profile: DimensionLightProfile,
    full: &mut FullSolveScratch,
) {
    let last = |len: usize| len - 1;
    // Each face: the outward direction and its cells, as in-bounds offsets.
    for direction in 0..6 {
        let (x_range, y_range, z_range) = match direction {
            0 => (0..1, 0..grid.y_len, 0..grid.z_len),
            1 => (last(grid.x_len)..grid.x_len, 0..grid.y_len, 0..grid.z_len),
            2 => (0..grid.x_len, 0..1, 0..grid.z_len),
            3 => (0..grid.x_len, last(grid.y_len)..grid.y_len, 0..grid.z_len),
            4 => (0..grid.x_len, 0..grid.y_len, 0..1),
            _ => (0..grid.x_len, 0..grid.y_len, last(grid.z_len)..grid.z_len),
        };
        let offset = super::solve::NEIGHBOURS[direction];
        for x in x_range {
            for y in y_range.clone() {
                for z in z_range.clone() {
                    let cell = grid.index(x, y, z);
                    let filter = full.filter[cell];
                    if filter == BLOCKED {
                        continue;
                    }
                    let position = grid.position(bounds, x, y, z);
                    let Some(neighbour) = position.checked_offset(offset) else {
                        continue;
                    };
                    debug_assert!(!bounds.contains(neighbour));
                    if blocks.sample(neighbour).filter().is_none() {
                        continue;
                    }
                    for channel in [LightChannel::Block, LightChannel::Sky] {
                        if channel == LightChannel::Sky && !profile.allows_sky() {
                            continue;
                        }
                        let Some((level, direct)) = prior
                            .boundary_light(bounds.dimension(), neighbour, channel)
                            .trusted_parts()
                        else {
                            continue;
                        };
                        if level == 0 {
                            continue;
                        }
                        let values = match channel {
                            LightChannel::Block => &mut full.block,
                            LightChannel::Sky => &mut full.sky,
                        };
                        let incoming = level.saturating_sub(filter.max(1));
                        values[cell] = values[cell].max(incoming);
                        if channel == LightChannel::Sky
                            && direction == 3
                            && profile.direct_sky_down()
                            && level == 15
                            && direct
                            && filter == 0
                        {
                            full.direct_above[x * grid.z_len + z] = true;
                        }
                    }
                }
            }
        }
    }
}

/// Marks direct sky down each column: an open cell is direct under a direct cell, under trusted
/// direct sky entering the top face, or where its own sky seed is full. Direct cells are at 15.
fn fill_direct_sky(grid: Grid, full: &mut FullSolveScratch) {
    for x in 0..grid.x_len {
        for z in 0..grid.z_len {
            let mut lit_from_above = full.direct_above[x * grid.z_len + z];
            for y in (0..grid.y_len).rev() {
                let cell = grid.index(x, y, z);
                let direct = full.filter[cell] == 0 && (lit_from_above || full.sky[cell] == 15);
                if direct {
                    full.sky[cell] = 15;
                    full.direct[cell] = true;
                }
                lit_from_above = direct;
            }
        }
    }
}

/// Spreads `values` from every lit cell through open neighbours, each step losing the
/// destination's filter (at least one level), until no cell can rise.
fn spread(
    grid: Grid,
    filter: &[u8],
    values: &mut [u8],
    buckets: &mut [Vec<u32>; LEVELS],
    limits: SolverLimits,
    queued_total: &mut usize,
    stats: &mut LightSolveStats,
) -> Result<(), LightSolveError> {
    let mut pending = 0_usize;
    for bucket in buckets.iter_mut() {
        bucket.clear();
    }
    for x in 0..grid.x_len {
        for y in 0..grid.y_len {
            let row = grid.index(x, y, 0);
            for cell in row..row + grid.z_len {
                let level = values[cell];
                if level > 1 && filter[cell] != BLOCKED {
                    enqueue_counted(queued_total, 1, limits.max_queue_entries)?;
                    buckets[usize::from(level)].push(cell as u32);
                    pending += 1;
                }
            }
        }
    }
    stats.queue_peak = stats.queue_peak.max(pending);
    let steps = grid.steps();
    for level in (2..LEVELS).rev() {
        while let Some(cell) = buckets[level].pop() {
            pending -= 1;
            let cell = cell as usize;
            if usize::from(values[cell]) != level {
                continue;
            }
            stats.increase_dequeued += 1;
            for step in steps {
                let next = cell.wrapping_add_signed(step);
                let next_filter = filter[next];
                if next_filter == BLOCKED {
                    continue;
                }
                let candidate = (level as u8).saturating_sub(next_filter.max(1));
                if candidate > values[next] {
                    values[next] = candidate;
                    if candidate > 1 {
                        enqueue_counted(queued_total, 1, limits.max_queue_entries)?;
                        buckets[usize::from(candidate)].push(next as u32);
                        pending += 1;
                        stats.queue_peak = stats.queue_peak.max(pending);
                    }
                }
            }
        }
    }
    Ok(())
}
