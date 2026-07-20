use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use thiserror::Error;

use crate::{LightChannel, LightStorageError, LightStoreSnapshot, SubChunkKey, SubChunkLight};

/// Global block coordinate used by the dependency-free light solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    fn checked_offset(self, offset: [i32; 3]) -> Option<Self> {
        Some(Self::new(
            self.x.checked_add(offset[0])?,
            self.y.checked_add(offset[1])?,
            self.z.checked_add(offset[2])?,
        ))
    }
}

impl From<[i32; 3]> for BlockPos {
    fn from(value: [i32; 3]) -> Self {
        Self::new(value[0], value[1], value[2])
    }
}

/// Explicit fixture/registry properties for one resident block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightProperties {
    emission: u8,
    filter: u8,
}

impl LightProperties {
    /// Creates properties without consulting names or guessed metadata.
    pub fn new(emission: u8, filter: u8) -> Result<Self, LightStorageError> {
        if emission > 15 {
            return Err(LightStorageError::ValueOutOfRange { value: emission });
        }
        if filter > 15 {
            return Err(LightStorageError::ValueOutOfRange { value: filter });
        }
        Ok(Self { emission, filter })
    }

    #[must_use]
    pub const fn emission(self) -> u8 {
        self.emission
    }

    #[must_use]
    pub const fn filter(self) -> u8 {
        self.filter
    }
}

/// Streaming-aware block sample. Unknown is intentionally not transparent air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightBlockSample {
    Unknown,
    KnownAir,
    Resident(LightProperties),
}

impl LightBlockSample {
    fn filter(self) -> Option<u8> {
        match self {
            Self::Unknown => None,
            Self::KnownAir => Some(0),
            Self::Resident(properties) => Some(properties.filter()),
        }
    }

    fn emission(self) -> u8 {
        match self {
            Self::Resident(properties) => properties.emission(),
            Self::Unknown | Self::KnownAir => 0,
        }
    }
}

/// Palette-native source interface used by pure worker-side solves.
pub trait LightBlockAccess {
    fn sample(&self, position: BlockPos) -> LightBlockSample;

    /// Explicit sky seed at this coordinate. Unknown cells are always ignored.
    fn sky_seed(&self, _position: BlockPos) -> u8 {
        0
    }
}

/// Dimension profile selected by the caller rather than inferred from blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionLightProfile {
    Overworld { direct_sky_down: bool },
    Nether,
    End,
}

impl DimensionLightProfile {
    const fn allows_sky(self) -> bool {
        matches!(self, Self::Overworld { .. })
    }

    const fn direct_sky_down(self) -> bool {
        matches!(
            self,
            Self::Overworld {
                direct_sky_down: true
            }
        )
    }
}

/// Inclusive bounded solve region in one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightBounds {
    dimension: i32,
    min: BlockPos,
    max: BlockPos,
}

impl LightBounds {
    pub fn new(dimension: i32, min: BlockPos, max: BlockPos) -> Result<Self, LightSolveError> {
        if min.x > max.x || min.y > max.y || min.z > max.z {
            return Err(LightSolveError::InvalidBounds);
        }
        Ok(Self {
            dimension,
            min,
            max,
        })
    }

    #[must_use]
    pub const fn dimension(self) -> i32 {
        self.dimension
    }

    #[must_use]
    pub const fn contains(self, position: BlockPos) -> bool {
        position.x >= self.min.x
            && position.x <= self.max.x
            && position.y >= self.min.y
            && position.y <= self.max.y
            && position.z >= self.min.z
            && position.z <= self.max.z
    }

    fn volume(self) -> Option<usize> {
        let x = i64::from(self.max.x) - i64::from(self.min.x) + 1;
        let y = i64::from(self.max.y) - i64::from(self.min.y) + 1;
        let z = i64::from(self.max.z) - i64::from(self.min.z) + 1;
        usize::try_from(x.checked_mul(y)?.checked_mul(z)?).ok()
    }

    fn positions(self) -> impl Iterator<Item = BlockPos> {
        (self.min.x..=self.max.x).flat_map(move |x| {
            (self.min.y..=self.max.y)
                .flat_map(move |y| (self.min.z..=self.max.z).map(move |z| BlockPos::new(x, y, z)))
        })
    }
}

/// Hard limits applied before and during every solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolverLimits {
    max_voxels: usize,
    max_queue_entries: usize,
}

impl SolverLimits {
    #[must_use]
    pub const fn new(max_voxels: usize, max_queue_entries: usize) -> Self {
        Self {
            max_voxels,
            max_queue_entries,
        }
    }
}

/// Deterministic bounded-solver failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LightSolveError {
    #[error("light solve bounds are inverted")]
    InvalidBounds,
    #[error("light solve volume {requested} exceeds limit {max}")]
    VoxelLimitExceeded { requested: usize, max: usize },
    #[error("light solve queue exceeded limit {max}")]
    QueueLimitExceeded { max: usize },
    #[error("light source value {value} exceeds the four-bit maximum of 15")]
    LightValueOutOfRange { value: u8 },
}

/// Generation-qualified light supplied only for an exact one-cell solve halo.
///
/// The representation is private so trusted samples always contain a valid
/// nibble and callers must state whether direct-sky provenance is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryLightSample {
    state: BoundaryLightState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryLightState {
    Unknown,
    Untrusted,
    Trusted { level: u8, direct_sky: bool },
}

impl BoundaryLightSample {
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            state: BoundaryLightState::Unknown,
        }
    }

    #[must_use]
    pub const fn untrusted() -> Self {
        Self {
            state: BoundaryLightState::Untrusted,
        }
    }

    pub fn trusted(level: u8, direct_sky: bool) -> Result<Self, LightStorageError> {
        if level > 15 {
            return Err(LightStorageError::ValueOutOfRange { value: level });
        }
        Ok(Self {
            state: BoundaryLightState::Trusted { level, direct_sky },
        })
    }

    const fn trusted_parts(self) -> Option<(u8, bool)> {
        match self.state {
            BoundaryLightState::Trusted { level, direct_sky } => Some((level, direct_sky)),
            BoundaryLightState::Unknown | BoundaryLightState::Untrusted => None,
        }
    }
}

/// Read-only old-light contract, implemented by store snapshots and solve output.
///
/// `read_light` and `has_direct_sky_provenance` are called only inside the
/// requested bounds. `boundary_light` is called only for face-adjacent cells
/// in the exact one-cell halo. The scheduler qualifies halo samples against
/// its block/light generations; the pure solver never searches beyond them.
pub trait LightReadAccess {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8;

    /// Returns direct-sky provenance for an interior retained sample.
    fn has_direct_sky_provenance(&self, _dimension: i32, _position: BlockPos) -> bool {
        false
    }

    /// Returns generation-qualified light for an exact one-cell halo sample.
    /// Unknown, untrusted, or dirty light must not seed the solve.
    fn boundary_light(
        &self,
        _dimension: i32,
        _position: BlockPos,
        _channel: LightChannel,
    ) -> BoundaryLightSample {
        BoundaryLightSample::unknown()
    }
}

/// Allocation-free empty old-light input.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyLight;

impl LightReadAccess for EmptyLight {
    fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
        0
    }
}

impl LightReadAccess for LightStoreSnapshot {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        let (key, [x, y, z]) = split_position(dimension, position);
        self.light(key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }
}

/// Queue-work counters used by deterministic and live budget checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightSolveStats {
    pub darken_seeded: usize,
    pub darken_dequeued: usize,
    pub increase_dequeued: usize,
    pub queue_peak: usize,
}

/// Generation-tagged sparse result for every intersected sub-chunk.
#[derive(Debug, Clone)]
pub struct LightSolveOutput {
    dimension: i32,
    bounds: LightBounds,
    sub_chunks: BTreeMap<SubChunkKey, Arc<SubChunkLight>>,
    direct_sky: BTreeSet<BlockPos>,
    stats: LightSolveStats,
}

impl LightSolveOutput {
    #[must_use]
    pub fn light_at(&self, position: BlockPos, channel: LightChannel) -> u8 {
        self.read_light(self.dimension, position, channel)
    }

    #[must_use]
    pub const fn stats(&self) -> LightSolveStats {
        self.stats
    }

    #[must_use]
    pub const fn sub_chunks(&self) -> &BTreeMap<SubChunkKey, Arc<SubChunkLight>> {
        &self.sub_chunks
    }
}

impl LightReadAccess for LightSolveOutput {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        if dimension != self.dimension || !self.bounds.contains(position) {
            return 0;
        }
        let (key, [x, y, z]) = split_position(dimension, position);
        self.sub_chunks
            .get(&key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        dimension == self.dimension
            && self.bounds.contains(position)
            && self.direct_sky.contains(&position)
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if dimension != self.dimension || !self.bounds.contains(position) {
            return BoundaryLightSample::unknown();
        }
        let level = self.read_light(dimension, position, channel);
        if level == 0 {
            BoundaryLightSample::unknown()
        } else {
            BoundaryLightSample::untrusted()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IncreaseEntry {
    position: BlockPos,
    direct_sky: bool,
}

#[derive(Debug, Clone, Copy)]
struct DarkenEntry {
    position: BlockPos,
    channel: LightChannel,
    old_level: u8,
    direct_sky: bool,
}

const NEIGHBOURS: [[i32; 3]; 6] = [
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

    let mut output = MutableOutput::new(bounds.dimension, generation);
    let mut stats = LightSolveStats::default();
    let mut queued_total = 0_usize;

    // Load the previous bounded field first. The darken queue is seeded only
    // at values that no current source or neighbour can support; each entry
    // carries its old level and propagates removal through dependent values.
    let mut darken = VecDeque::new();
    for position in bounds.positions() {
        let sample = blocks.sample(position);
        if sample.filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let old = read_prior(prior, bounds.dimension, position, channel)?;
            output.set(position, channel, old);
        }
    }

    for position in bounds.positions() {
        if blocks.sample(position).filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let old = output.get(position, channel);
            let base = local_base(blocks, position, channel, profile)?;
            if old == 0
                || old <= supported_prior_level(blocks, prior, bounds, position, channel, profile)?
            {
                continue;
            }
            output.set(position, channel, base);
            enqueue_counted(&mut queued_total, 1, limits.max_queue_entries)?;
            darken.push_back(DarkenEntry {
                position,
                channel,
                old_level: old,
                direct_sky: channel == LightChannel::Sky
                    && prior.has_direct_sky_provenance(bounds.dimension, position),
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
            if !bounds.contains(next) || blocks.sample(next).filter().is_none() {
                continue;
            }
            let current = output.get(next, entry.channel);
            let base = local_base(blocks, next, entry.channel, profile)?;
            let depended_on_removed = current < entry.old_level
                || (entry.channel == LightChannel::Sky
                    && profile.direct_sky_down()
                    && entry.direct_sky
                    && offset == [0, -1, 0]
                    && current == 15
                    && entry.old_level == 15);
            if current > base && depended_on_removed {
                output.set(next, entry.channel, base);
                enqueue_counted(&mut queued_total, 1, limits.max_queue_entries)?;
                darken.push_back(DarkenEntry {
                    position: next,
                    channel: entry.channel,
                    old_level: current,
                    direct_sky: entry.channel == LightChannel::Sky
                        && prior.has_direct_sky_provenance(bounds.dimension, next),
                });
                stats.queue_peak = stats.queue_peak.max(darken.len());
            }
        }
    }

    for position in bounds.positions() {
        if blocks.sample(position).filter().is_none() {
            continue;
        }
        for channel in [LightChannel::Block, LightChannel::Sky] {
            let base = local_base(blocks, position, channel, profile)?;
            if base > output.get(position, channel) {
                output.set(position, channel, base);
            }
        }
    }

    let mut block_increase = VecDeque::new();
    let mut sky_increase = VecDeque::new();
    let mut direct_sky = BTreeSet::new();
    for position in bounds.positions() {
        let Some(filter) = blocks.sample(position).filter() else {
            continue;
        };
        for (channel, queue) in [
            (LightChannel::Block, &mut block_increase),
            (LightChannel::Sky, &mut sky_increase),
        ] {
            let level = output.get(position, channel);
            if level == 0 {
                continue;
            }
            let is_direct = channel == LightChannel::Sky
                && profile.direct_sky_down()
                && level == 15
                && filter == 0
                && (local_base(blocks, position, channel, profile)? == 15
                    || prior.has_direct_sky_provenance(bounds.dimension, position));
            if is_direct {
                direct_sky.insert(position);
            }
            enqueue_counted(&mut queued_total, 1, limits.max_queue_entries)?;
            queue.push_back(IncreaseEntry {
                position,
                direct_sky: is_direct,
            });
        }
    }
    propagate(
        blocks,
        prior,
        bounds,
        LightChannel::Block,
        profile,
        &mut output,
        &mut block_increase,
        &mut BTreeSet::new(),
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
        &mut sky_increase,
        &mut direct_sky,
        limits,
        &mut queued_total,
        &mut stats,
    )?;

    Ok(output.freeze(bounds, direct_sky, stats))
}

fn read_prior<P: LightReadAccess>(
    prior: &P,
    dimension: i32,
    position: BlockPos,
    channel: LightChannel,
) -> Result<u8, LightSolveError> {
    let value = prior.read_light(dimension, position, channel);
    if value > 15 {
        Err(LightSolveError::LightValueOutOfRange { value })
    } else {
        Ok(value)
    }
}

fn local_base<A: LightBlockAccess>(
    blocks: &A,
    position: BlockPos,
    channel: LightChannel,
    profile: DimensionLightProfile,
) -> Result<u8, LightSolveError> {
    let sample = blocks.sample(position);
    let Some(filter) = sample.filter() else {
        return Ok(0);
    };
    match channel {
        LightChannel::Block => Ok(sample.emission()),
        LightChannel::Sky => {
            let seed = blocks.sky_seed(position);
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

fn supported_prior_level<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    position: BlockPos,
    channel: LightChannel,
    profile: DimensionLightProfile,
) -> Result<u8, LightSolveError> {
    let Some(filter) = blocks.sample(position).filter() else {
        return Ok(0);
    };
    let mut supported = local_base(blocks, position, channel, profile)?;
    if channel == LightChannel::Sky && !profile.allows_sky() {
        return Ok(supported);
    }
    for offset in NEIGHBOURS {
        let Some(neighbour) = position.checked_offset(offset) else {
            continue;
        };
        if blocks.sample(neighbour).filter().is_none() {
            continue;
        }
        let (neighbour_level, direct_sky) = if bounds.contains(neighbour) {
            (
                read_prior(prior, bounds.dimension, neighbour, channel)?,
                channel == LightChannel::Sky
                    && prior.has_direct_sky_provenance(bounds.dimension, neighbour),
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
        supported = supported.max(incoming_level(
            neighbour_level,
            filter,
            channel,
            profile,
            offset,
            direct_sky,
        ));
    }
    Ok(supported)
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
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    output: &mut MutableOutput,
    queue: &mut VecDeque<IncreaseEntry>,
    direct_positions: &mut BTreeSet<BlockPos>,
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
        let source = output.get(entry.position, channel);
        for offset in NEIGHBOURS {
            let Some(next) = entry.position.checked_offset(offset) else {
                continue;
            };
            if !bounds.contains(next) {
                continue;
            }
            let Some(filter) = blocks.sample(next).filter() else {
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
            let current = output.get(next, channel);
            let gains_direct = continues_direct && !direct_positions.contains(&next);
            if candidate > current || (candidate == current && gains_direct) {
                if candidate > current {
                    output.set(next, channel, candidate);
                }
                if continues_direct {
                    direct_positions.insert(next);
                }
                enqueue_counted(queued_total, 1, limits.max_queue_entries)?;
                queue.push_back(IncreaseEntry {
                    position: next,
                    direct_sky: continues_direct,
                });
                stats.queue_peak = stats.queue_peak.max(queue.len());
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn seed_boundary_from_halo<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    output: &mut MutableOutput,
    queue: &mut VecDeque<IncreaseEntry>,
    direct_positions: &mut BTreeSet<BlockPos>,
    limits: SolverLimits,
    queued_total: &mut usize,
) -> Result<(), LightSolveError> {
    if channel == LightChannel::Sky && !profile.allows_sky() {
        return Ok(());
    }
    for position in bounds.positions() {
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
        let current = output.get(position, channel);
        let gains_direct = candidate_is_direct && !direct_positions.contains(&position);
        if candidate > current || (candidate == current && candidate != 0 && gains_direct) {
            if candidate > current {
                output.set(position, channel, candidate);
            }
            if candidate_is_direct {
                direct_positions.insert(position);
            }
            enqueue_counted(queued_total, 1, limits.max_queue_entries)?;
            queue.push_back(IncreaseEntry {
                position,
                direct_sky: candidate_is_direct,
            });
        }
    }
    Ok(())
}

fn enqueue_counted(total: &mut usize, amount: usize, max: usize) -> Result<(), LightSolveError> {
    *total = total.saturating_add(amount);
    if *total > max {
        Err(LightSolveError::QueueLimitExceeded { max })
    } else {
        Ok(())
    }
}

struct MutableOutput {
    dimension: i32,
    generation: u64,
    sub_chunks: BTreeMap<SubChunkKey, SubChunkLight>,
}

impl MutableOutput {
    fn new(dimension: i32, generation: u64) -> Self {
        Self {
            dimension,
            generation,
            sub_chunks: BTreeMap::new(),
        }
    }

    fn get(&self, position: BlockPos, channel: LightChannel) -> u8 {
        let (key, [x, y, z]) = split_position(self.dimension, position);
        self.sub_chunks
            .get(&key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }

    fn set(&mut self, position: BlockPos, channel: LightChannel, value: u8) {
        let (key, [x, y, z]) = split_position(self.dimension, position);
        let light = self
            .sub_chunks
            .entry(key)
            .or_insert_with(|| SubChunkLight::dark(self.generation));
        light
            .set(channel, x, y, z, value)
            .expect("solver only emits validated nibble values");
    }

    fn freeze(
        self,
        bounds: LightBounds,
        direct_sky: BTreeSet<BlockPos>,
        stats: LightSolveStats,
    ) -> LightSolveOutput {
        LightSolveOutput {
            dimension: self.dimension,
            bounds,
            sub_chunks: self
                .sub_chunks
                .into_iter()
                .map(|(key, light)| (key, Arc::new(light)))
                .collect(),
            direct_sky,
            stats,
        }
    }
}

fn split_position(dimension: i32, position: BlockPos) -> (SubChunkKey, [u8; 3]) {
    let key = SubChunkKey::new(
        dimension,
        position.x.div_euclid(16),
        position.y.div_euclid(16),
        position.z.div_euclid(16),
    );
    let local = [
        position.x.rem_euclid(16) as u8,
        position.y.rem_euclid(16) as u8,
        position.z.rem_euclid(16) as u8,
    ];
    (key, local)
}
