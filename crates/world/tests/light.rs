use std::{collections::BTreeMap, sync::Arc};

use world::{
    BlockPos, BoundaryLightSample, ChunkKey, DimensionLightProfile, EmptyLight, LightBlockAccess,
    LightBlockSample, LightBounds, LightChannel, LightNibbleStorage, LightProperties,
    LightReadAccess, LightSolveError, LightSolveOutput, LightStore, LightSubChunkKind,
    SolverLimits, SubChunkKey, SubChunkLight, solve_light,
};

#[derive(Default)]
struct FixtureAccess {
    blocks: BTreeMap<BlockPos, LightBlockSample>,
    sky: BTreeMap<BlockPos, u8>,
}

impl FixtureAccess {
    fn air(&mut self, x: i32, y: i32, z: i32) {
        self.blocks
            .insert(BlockPos::new(x, y, z), LightBlockSample::KnownAir);
    }

    fn block(&mut self, x: i32, y: i32, z: i32, emission: u8, filter: u8) {
        self.blocks.insert(
            BlockPos::new(x, y, z),
            LightBlockSample::Resident(LightProperties::new(emission, filter).unwrap()),
        );
    }

    fn sky(&mut self, x: i32, y: i32, z: i32, value: u8) {
        self.sky.insert(BlockPos::new(x, y, z), value);
    }
}

impl LightBlockAccess for FixtureAccess {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        self.blocks
            .get(&position)
            .copied()
            .unwrap_or(LightBlockSample::Unknown)
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        self.sky.get(&position).copied().unwrap_or(0)
    }
}

fn bounds(min: [i32; 3], max: [i32; 3]) -> LightBounds {
    LightBounds::new(0, BlockPos::from(min), BlockPos::from(max)).unwrap()
}

fn limits() -> SolverLimits {
    SolverLimits::new(32_768, 1_000_000)
}

struct GuardedPrior<'a> {
    inner: &'a LightSolveOutput,
    min: BlockPos,
    max: BlockPos,
    boundary: Vec<(BlockPos, LightChannel, BoundaryLightSample)>,
}

impl<'a> GuardedPrior<'a> {
    fn new(inner: &'a LightSolveOutput, min: [i32; 3], max: [i32; 3]) -> Self {
        Self {
            inner,
            min: min.into(),
            max: max.into(),
            boundary: Vec::new(),
        }
    }

    fn trusted(
        mut self,
        position: [i32; 3],
        channel: LightChannel,
        level: u8,
        direct_sky: bool,
    ) -> Self {
        self.boundary.push((
            position.into(),
            channel,
            BoundaryLightSample::trusted(level, direct_sky).unwrap(),
        ));
        self
    }

    fn untrusted(mut self, position: [i32; 3], channel: LightChannel) -> Self {
        self.boundary
            .push((position.into(), channel, BoundaryLightSample::untrusted()));
        self
    }

    fn contains(&self, position: BlockPos) -> bool {
        position.x >= self.min.x
            && position.x <= self.max.x
            && position.y >= self.min.y
            && position.y <= self.max.y
            && position.z >= self.min.z
            && position.z <= self.max.z
    }

    fn in_exact_halo(&self, position: BlockPos) -> bool {
        let axis_distance = |value: i32, min: i32, max: i32| {
            if value < min {
                min - value
            } else if value > max {
                value - max
            } else {
                0
            }
        };
        axis_distance(position.x, self.min.x, self.max.x)
            + axis_distance(position.y, self.min.y, self.max.y)
            + axis_distance(position.z, self.min.z, self.max.z)
            == 1
    }
}

impl LightReadAccess for GuardedPrior<'_> {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        assert!(
            self.contains(position),
            "interior read escaped solve bounds: {position:?}"
        );
        self.inner.read_light(dimension, position, channel)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        assert!(
            self.contains(position),
            "provenance read escaped solve bounds: {position:?}"
        );
        self.inner.has_direct_sky_provenance(dimension, position)
    }

    fn boundary_light(
        &self,
        _dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        assert!(
            self.in_exact_halo(position),
            "boundary read escaped exact one-cell halo: {position:?}"
        );
        self.boundary
            .iter()
            .find_map(|&(candidate, candidate_channel, sample)| {
                (candidate == position && candidate_channel == channel).then_some(sample)
            })
            .unwrap_or_else(BoundaryLightSample::unknown)
    }
}

#[test]
fn nibble_storage_rejects_values_above_fifteen() {
    assert!(LightNibbleStorage::uniform(15).is_ok());
    assert!(LightNibbleStorage::uniform(16).is_err());

    let mut storage = LightNibbleStorage::uniform(0).unwrap();
    assert!(storage.set(0, 15).is_ok());
    assert!(storage.set(0, 16).is_err());
}

#[test]
fn trusted_boundary_samples_are_checked_and_non_forgeable() {
    assert!(BoundaryLightSample::trusted(15, true).is_ok());
    assert!(BoundaryLightSample::trusted(16, false).is_err());
}

#[test]
fn packed_nibbles_preserve_every_exact_level_and_collapse_after_last_difference() {
    let mut storage = LightNibbleStorage::uniform(7).unwrap();
    for value in 0..=15 {
        storage.set(usize::from(value), value).unwrap();
    }
    for value in 0..=15 {
        assert_eq!(storage.get(usize::from(value)), Some(value));
    }
    for index in 0..16 {
        storage.set(index, 7).unwrap();
    }
    assert!(storage.is_uniform());
    assert_eq!(storage.allocated_bytes(), 0);
}

#[test]
fn nibble_storage_is_copy_on_write_and_collapses_to_uniform() {
    let mut original = LightNibbleStorage::uniform(3).unwrap();
    original.set(7, 9).unwrap();
    let snapshot = original.clone();

    assert!(original.shares_packed_bytes_with(&snapshot));
    original.set(8, 10).unwrap();
    assert!(!original.shares_packed_bytes_with(&snapshot));
    assert_eq!(snapshot.get(7), Some(9));
    assert_eq!(snapshot.get(8), Some(3));

    original.fill(4).unwrap();
    assert!(original.is_uniform());
    assert_eq!(original.get(0), Some(4));
    assert_eq!(original.allocated_bytes(), 0);
}

#[test]
fn subchunk_light_keeps_independent_channels_and_generation() {
    let mut light = SubChunkLight::dark(41);
    light.set(LightChannel::Block, 1, 2, 3, 12).unwrap();
    light.set(LightChannel::Sky, 1, 2, 3, 7).unwrap();

    assert_eq!(light.generation(), 41);
    assert_eq!(light.get(LightChannel::Block, 1, 2, 3), Some(12));
    assert_eq!(light.get(LightChannel::Sky, 1, 2, 3), Some(7));
}

#[test]
fn light_store_distinguishes_boundaries_snapshots_and_eviction() {
    let unknown = SubChunkKey::new(0, 9, 2, -4);
    let air = SubChunkKey::new(0, 10, 2, -4);
    let resident = SubChunkKey::new(0, 11, 2, -4);
    let mut store = LightStore::default();

    assert_eq!(store.kind(unknown), LightSubChunkKind::Unknown);
    store.insert_known_air(air, SubChunkLight::dark(1));
    store.insert_resident(resident, SubChunkLight::dark(2));
    assert_eq!(store.kind(air), LightSubChunkKind::KnownAir);
    assert_eq!(store.kind(resident), LightSubChunkKind::Resident);

    let snapshot = store.snapshot();
    let replacement = SubChunkLight::dark(3);
    assert!(store.commit_if_generation(resident, Some(2), replacement));
    assert_eq!(snapshot.light(resident).unwrap().generation(), 2);
    assert_eq!(store.light(resident).unwrap().generation(), 3);
    assert!(!store.commit_if_generation(resident, Some(2), SubChunkLight::dark(4)));

    let removed = store.evict_chunk(ChunkKey::new(0, 11, -4));
    assert_eq!(removed, vec![resident]);
    assert_eq!(store.kind(resident), LightSubChunkKind::Unknown);
    assert!(Arc::strong_count(snapshot.light(resident).unwrap()) >= 1);
}

#[test]
fn propagation_crosses_subchunk_boundaries_and_applies_filter() {
    let mut access = FixtureAccess::default();
    access.block(15, 0, 0, 15, 0);
    access.air(16, 0, 0);
    access.block(17, 0, 0, 0, 2);

    let solved = solve_light(
        &access,
        &EmptyLight,
        bounds([15, 0, 0], [17, 0, 0]),
        1,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();

    assert_eq!(
        solved.light_at(BlockPos::new(15, 0, 0), LightChannel::Block),
        15
    );
    assert_eq!(
        solved.light_at(BlockPos::new(16, 0, 0), LightChannel::Block),
        14
    );
    assert_eq!(
        solved.light_at(BlockPos::new(17, 0, 0), LightChannel::Block),
        12
    );
    assert_eq!(solved.sub_chunks().len(), 2);
}

#[test]
fn one_cell_halo_imports_known_boundary_light_without_opening_unknown_space() {
    let mut access = FixtureAccess::default();
    access.block(-1, 0, 0, 15, 0);
    access.air(0, 0, 0);
    access.air(1, 0, 0);
    let source = solve_light(
        &access,
        &EmptyLight,
        bounds([-1, 0, 0], [-1, 0, 0]),
        10,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();
    let prior = GuardedPrior::new(&source, [0, 0, 0], [1, 0, 0]).trusted(
        [-1, 0, 0],
        LightChannel::Block,
        15,
        false,
    );

    let solved = solve_light(
        &access,
        &prior,
        bounds([0, 0, 0], [1, 0, 0]),
        11,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();

    assert_eq!(
        solved.light_at(BlockPos::new(0, 0, 0), LightChannel::Block),
        14
    );
    assert_eq!(
        solved.light_at(BlockPos::new(1, 0, 0), LightChannel::Block),
        13
    );
}

#[test]
fn trusted_propagated_block_boundary_seeds_without_ancestry_reads() {
    let mut access = FixtureAccess::default();
    access.block(-2, 0, 0, 15, 0);
    access.air(-1, 0, 0);
    access.air(0, 0, 0);
    let outer = solve_light(
        &access,
        &EmptyLight,
        bounds([-2, 0, 0], [-1, 0, 0]),
        12,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();
    let prior = GuardedPrior::new(&outer, [0, 0, 0], [0, 0, 0]).trusted(
        [-1, 0, 0],
        LightChannel::Block,
        14,
        false,
    );

    let inner = solve_light(
        &access,
        &prior,
        bounds([0, 0, 0], [0, 0, 0]),
        13,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();

    assert_eq!(
        inner.light_at(BlockPos::new(0, 0, 0), LightChannel::Block),
        13
    );
}

#[test]
fn unknown_cells_block_emission_and_never_accept_sky_seeds() {
    let mut access = FixtureAccess::default();
    access.block(0, 0, 0, 15, 0);
    // x=1 remains Unknown.
    access.air(2, 0, 0);
    access.sky(1, 0, 0, 15);

    let solved = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 0, 0], [2, 0, 0]),
        2,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();

    assert_eq!(
        solved.light_at(BlockPos::new(1, 0, 0), LightChannel::Block),
        0
    );
    assert_eq!(
        solved.light_at(BlockPos::new(2, 0, 0), LightChannel::Block),
        0
    );
    assert_eq!(
        solved.light_at(BlockPos::new(1, 0, 0), LightChannel::Sky),
        0
    );
}

#[test]
fn emitter_removal_runs_darken_then_reseed_increase() {
    let mut lit = FixtureAccess::default();
    lit.block(0, 0, 0, 15, 0);
    lit.air(1, 0, 0);
    lit.air(2, 0, 0);
    let region = bounds([0, 0, 0], [2, 0, 0]);
    let first = solve_light(
        &lit,
        &EmptyLight,
        region,
        3,
        DimensionLightProfile::End,
        limits(),
    )
    .unwrap();

    let mut removed = FixtureAccess::default();
    removed.air(0, 0, 0);
    removed.air(1, 0, 0);
    removed.air(2, 0, 0);
    let second = solve_light(
        &removed,
        &first,
        region,
        4,
        DimensionLightProfile::End,
        limits(),
    )
    .unwrap();

    assert_eq!(
        second.light_at(BlockPos::new(0, 0, 0), LightChannel::Block),
        0
    );
    assert_eq!(
        second.light_at(BlockPos::new(2, 0, 0), LightChannel::Block),
        0
    );
    assert_eq!(second.stats().darken_seeded, 1);
    assert!(second.stats().darken_dequeued >= 3);
    assert_eq!(second.stats().increase_dequeued, 0);
}

#[test]
fn removed_halo_emitter_cannot_reseed_stale_prior_light() {
    let mut lit = FixtureAccess::default();
    lit.block(-1, 0, 0, 15, 0);
    lit.air(0, 0, 0);
    let first = solve_light(
        &lit,
        &EmptyLight,
        bounds([-1, 0, 0], [0, 0, 0]),
        20,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();

    let mut removed = FixtureAccess::default();
    removed.air(-1, 0, 0);
    removed.air(0, 0, 0);
    let prior =
        GuardedPrior::new(&first, [0, 0, 0], [0, 0, 0]).untrusted([-1, 0, 0], LightChannel::Block);
    let second = solve_light(
        &removed,
        &prior,
        bounds([0, 0, 0], [0, 0, 0]),
        21,
        DimensionLightProfile::Nether,
        limits(),
    )
    .unwrap();

    assert_eq!(
        second.light_at(BlockPos::new(0, 0, 0), LightChannel::Block),
        0
    );
}

#[test]
fn opaque_roof_blocks_sky_while_a_known_shaft_carries_direct_light() {
    let mut access = FixtureAccess::default();
    for y in 0..=3 {
        access.air(0, y, 0);
        access.air(1, y, 0);
    }
    access.block(0, 3, 0, 0, 15);
    access.sky(0, 3, 0, 15);
    access.sky(1, 3, 0, 15);

    let solved = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 0, 0], [1, 3, 0]),
        5,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();

    assert_eq!(
        solved.light_at(BlockPos::new(0, 3, 0), LightChannel::Sky),
        0
    );
    assert_eq!(
        solved.light_at(BlockPos::new(1, 0, 0), LightChannel::Sky),
        15
    );
    assert_eq!(
        solved.light_at(BlockPos::new(0, 0, 0), LightChannel::Sky),
        14
    );
}

#[test]
fn nether_and_end_profiles_disable_even_explicit_sky_seeds() {
    let mut access = FixtureAccess::default();
    access.air(0, 0, 0);
    access.sky(0, 0, 0, 15);
    let region = bounds([0, 0, 0], [0, 0, 0]);

    for profile in [DimensionLightProfile::Nether, DimensionLightProfile::End] {
        let solved = solve_light(&access, &EmptyLight, region, 6, profile, limits()).unwrap();
        assert_eq!(
            solved.light_at(BlockPos::new(0, 0, 0), LightChannel::Sky),
            0
        );
    }
}

#[test]
fn no_sky_profiles_never_import_prior_sky_from_the_halo() {
    let mut access = FixtureAccess::default();
    access.air(0, 1, 0);
    access.air(0, 0, 0);
    access.sky(0, 1, 0, 15);
    let upper = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 1, 0], [0, 1, 0]),
        22,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();
    let prior = GuardedPrior::new(&upper, [0, 0, 0], [0, 0, 0]).trusted(
        [0, 1, 0],
        LightChannel::Sky,
        15,
        true,
    );

    for profile in [DimensionLightProfile::Nether, DimensionLightProfile::End] {
        let lower = solve_light(
            &access,
            &prior,
            bounds([0, 0, 0], [0, 0, 0]),
            23,
            profile,
            limits(),
        )
        .unwrap();
        assert_eq!(lower.light_at(BlockPos::new(0, 0, 0), LightChannel::Sky), 0);
    }
}

#[test]
fn direct_sky_fifteen_crosses_a_vertical_solve_seam_without_attenuation() {
    let mut access = FixtureAccess::default();
    access.air(0, 16, 0);
    access.air(0, 15, 0);
    access.sky(0, 16, 0, 15);
    let upper = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 16, 0], [0, 16, 0]),
        24,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();
    let prior = GuardedPrior::new(&upper, [0, 15, 0], [0, 15, 0]).trusted(
        [0, 16, 0],
        LightChannel::Sky,
        15,
        true,
    );
    let lower = solve_light(
        &access,
        &prior,
        bounds([0, 15, 0], [0, 15, 0]),
        25,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();

    assert_eq!(
        lower.light_at(BlockPos::new(0, 15, 0), LightChannel::Sky),
        15
    );
}

#[test]
fn propagated_direct_sky_halo_retains_fifteen_below_the_seam() {
    let mut access = FixtureAccess::default();
    for y in 14..=17 {
        access.air(0, y, 0);
    }
    access.sky(0, 17, 0, 15);
    let upper = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 16, 0], [0, 17, 0]),
        27,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();
    assert_eq!(
        upper.light_at(BlockPos::new(0, 16, 0), LightChannel::Sky),
        15
    );
    let upper_prior = GuardedPrior::new(&upper, [0, 15, 0], [0, 15, 0]).trusted(
        [0, 16, 0],
        LightChannel::Sky,
        15,
        true,
    );
    let retained = solve_light(
        &access,
        &upper_prior,
        bounds([0, 15, 0], [0, 15, 0]),
        28,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();
    assert_eq!(
        retained.light_at(BlockPos::new(0, 15, 0), LightChannel::Sky),
        15
    );

    let retained_prior = GuardedPrior::new(&retained, [0, 14, 0], [0, 15, 0]).trusted(
        [0, 16, 0],
        LightChannel::Sky,
        15,
        true,
    );
    let lower = solve_light(
        &access,
        &retained_prior,
        bounds([0, 14, 0], [0, 15, 0]),
        29,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();

    assert_eq!(
        lower.light_at(BlockPos::new(0, 15, 0), LightChannel::Sky),
        15
    );
    assert_eq!(
        lower.light_at(BlockPos::new(0, 14, 0), LightChannel::Sky),
        15
    );

    let mut removed = FixtureAccess::default();
    for y in 14..=17 {
        removed.air(0, y, 0);
    }
    let stale_prior = GuardedPrior::new(&retained, [0, 14, 0], [0, 15, 0])
        .untrusted([0, 16, 0], LightChannel::Sky);
    let stale = solve_light(
        &removed,
        &stale_prior,
        bounds([0, 14, 0], [0, 15, 0]),
        30,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();
    assert_eq!(
        stale.light_at(BlockPos::new(0, 15, 0), LightChannel::Sky),
        0
    );
}

#[test]
fn solver_enforces_voxel_and_queue_limits() {
    let access = FixtureAccess::default();
    let too_large = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 0, 0], [2, 2, 2]),
        7,
        DimensionLightProfile::Nether,
        SolverLimits::new(26, 100),
    );
    assert_eq!(
        too_large.unwrap_err(),
        LightSolveError::VoxelLimitExceeded {
            requested: 27,
            max: 26
        }
    );

    let mut emitter = FixtureAccess::default();
    emitter.block(0, 0, 0, 15, 0);
    emitter.air(1, 0, 0);
    let queue_limited = solve_light(
        &emitter,
        &EmptyLight,
        bounds([0, 0, 0], [1, 0, 0]),
        8,
        DimensionLightProfile::Nether,
        SolverLimits::new(2, 0),
    );
    assert_eq!(
        queue_limited.unwrap_err(),
        LightSolveError::QueueLimitExceeded { max: 0 }
    );
}

#[test]
fn queue_limit_is_global_across_block_and_sky_channels() {
    let mut access = FixtureAccess::default();
    access.block(0, 0, 0, 15, 0);
    access.sky(0, 0, 0, 15);
    let result = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 0, 0], [0, 0, 0]),
        31,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        SolverLimits::new(1, 1),
    );

    assert_eq!(
        result.unwrap_err(),
        LightSolveError::QueueLimitExceeded { max: 1 }
    );
}

#[test]
fn solver_rejects_out_of_range_trait_sky_values_without_panicking() {
    let mut access = FixtureAccess::default();
    access.air(0, 0, 0);
    access.sky(0, 0, 0, 16);
    let result = solve_light(
        &access,
        &EmptyLight,
        bounds([0, 0, 0], [0, 0, 0]),
        9,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    );

    assert_eq!(
        result.unwrap_err(),
        LightSolveError::LightValueOutOfRange { value: 16 }
    );
}

struct InvalidInteriorPrior;

impl world::LightReadAccess for InvalidInteriorPrior {
    fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
        16
    }
}

#[test]
fn solver_rejects_out_of_range_interior_prior_light_without_panicking() {
    let mut access = FixtureAccess::default();
    access.air(0, 0, 0);
    let result = solve_light(
        &access,
        &InvalidInteriorPrior,
        bounds([0, 0, 0], [0, 0, 0]),
        26,
        DimensionLightProfile::Nether,
        limits(),
    );

    assert_eq!(
        result.unwrap_err(),
        LightSolveError::LightValueOutOfRange { value: 16 }
    );
}
