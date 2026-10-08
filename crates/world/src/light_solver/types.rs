use thiserror::Error;

use crate::{LightChannel, LightStorageError, LightStoreSnapshot, SubChunkKey};

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

    pub(super) fn checked_offset(self, offset: [i32; 3]) -> Option<Self> {
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
    pub(super) fn filter(self) -> Option<u8> {
        match self {
            Self::Unknown => None,
            Self::KnownAir => Some(0),
            Self::Resident(properties) => Some(properties.filter()),
        }
    }

    pub(super) fn emission(self) -> u8 {
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
    pub(super) const fn allows_sky(self) -> bool {
        matches!(self, Self::Overworld { .. })
    }

    pub(super) const fn direct_sky_down(self) -> bool {
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
    pub(super) dimension: i32,
    pub(super) min: BlockPos,
    pub(super) max: BlockPos,
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
    pub const fn min(self) -> BlockPos {
        self.min
    }

    #[must_use]
    pub const fn max(self) -> BlockPos {
        self.max
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

    pub(super) fn volume(self) -> Option<usize> {
        let x = i64::from(self.max.x) - i64::from(self.min.x) + 1;
        let y = i64::from(self.max.y) - i64::from(self.min.y) + 1;
        let z = i64::from(self.max.z) - i64::from(self.min.z) + 1;
        usize::try_from(x.checked_mul(y)?.checked_mul(z)?).ok()
    }

    pub(super) fn positions(self) -> impl Iterator<Item = BlockPos> {
        (self.min.x..=self.max.x).flat_map(move |x| {
            (self.min.y..=self.max.y)
                .flat_map(move |y| (self.min.z..=self.max.z).map(move |z| BlockPos::new(x, y, z)))
        })
    }
}

/// Hard limits applied before and during every solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolverLimits {
    pub(super) max_voxels: usize,
    pub(super) max_queue_entries: usize,
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

    pub(super) const fn trusted_parts(self) -> Option<(u8, bool)> {
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

pub(super) fn light_axis_len(min: i32, max: i32) -> usize {
    usize::try_from(i64::from(max) - i64::from(min) + 1)
        .expect("validated light bounds have a representable axis extent")
}

pub(super) fn light_dense_index(
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    position: BlockPos,
) -> Option<usize> {
    if !bounds.contains(position) {
        return None;
    }
    let x = usize::try_from(i64::from(position.x) - i64::from(bounds.min.x)).ok()?;
    let y = usize::try_from(i64::from(position.y) - i64::from(bounds.min.y)).ok()?;
    let z = usize::try_from(i64::from(position.z) - i64::from(bounds.min.z)).ok()?;
    x.checked_mul(y_len)?
        .checked_add(y)?
        .checked_mul(z_len)?
        .checked_add(z)
}

pub(super) const fn light_channel_index(channel: LightChannel) -> usize {
    match channel {
        LightChannel::Block => 0,
        LightChannel::Sky => 1,
    }
}

pub(super) fn split_position(dimension: i32, position: BlockPos) -> (SubChunkKey, [u8; 3]) {
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
