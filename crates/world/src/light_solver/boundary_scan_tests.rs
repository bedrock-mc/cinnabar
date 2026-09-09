use std::{
    cell::RefCell,
    collections::BTreeMap,
    hint::black_box,
    time::{Duration, Instant},
};

use super::*;

#[derive(Default)]
struct RecordingBlocks {
    samples: BTreeMap<BlockPos, LightBlockSample>,
    reads: RefCell<Vec<BlockPos>>,
}

impl RecordingBlocks {
    fn fill_air(&mut self, bounds: LightBounds) {
        for position in bounds.positions() {
            self.samples.insert(position, LightBlockSample::KnownAir);
        }
    }
}

impl LightBlockAccess for RecordingBlocks {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        self.reads.borrow_mut().push(position);
        self.samples
            .get(&position)
            .copied()
            .unwrap_or(LightBlockSample::Unknown)
    }
}

#[derive(Default)]
struct RecordingPrior {
    boundary: BTreeMap<(i32, BlockPos, usize), BoundaryLightSample>,
    reads: RefCell<Vec<(i32, BlockPos, LightChannel)>>,
}

impl LightReadAccess for RecordingPrior {
    fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
        0
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        self.reads.borrow_mut().push((dimension, position, channel));
        self.boundary
            .get(&(dimension, position, light_channel_index(channel)))
            .copied()
            .unwrap_or_else(BoundaryLightSample::unknown)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SeedSnapshot {
    values: Vec<(BlockPos, u8, bool)>,
    queue: Vec<(BlockPos, bool)>,
    queued_total: usize,
}

fn is_boundary(bounds: LightBounds, position: BlockPos) -> bool {
    position.x == bounds.min.x
        || position.x == bounds.max.x
        || position.y == bounds.min.y
        || position.y == bounds.max.y
        || position.z == bounds.min.z
        || position.z == bounds.max.z
}

fn snapshot_seed(
    bounds: LightBounds,
    channel: LightChannel,
    output: &MutableOutput,
    direct: &DensePositionSet,
    queue: &VecDeque<IncreaseEntry>,
    queued_total: usize,
) -> SeedSnapshot {
    SeedSnapshot {
        values: bounds
            .positions()
            .map(|position| {
                (
                    position,
                    output.get(position, channel),
                    direct.contains(&position),
                )
            })
            .collect(),
        queue: queue
            .iter()
            .map(|entry| (entry.position, entry.direct_sky))
            .collect(),
        queued_total,
    }
}

#[allow(clippy::too_many_arguments)]
fn seed_boundary_full_volume_oracle<A: LightBlockAccess, P: LightReadAccess>(
    blocks: &A,
    prior: &P,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    output: &mut MutableOutput,
    queue: &mut VecDeque<IncreaseEntry>,
    direct_positions: &mut DensePositionSet,
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

fn run_seed(
    oracle: bool,
    blocks: &RecordingBlocks,
    prior: &RecordingPrior,
    bounds: LightBounds,
    channel: LightChannel,
    profile: DimensionLightProfile,
    queue_cap: usize,
) -> Result<SeedSnapshot, LightSolveError> {
    let volume = bounds.volume().unwrap();
    let mut output = MutableOutput::new(bounds, 41, volume);
    let mut queue = VecDeque::new();
    let mut direct = DensePositionSet::new(bounds, volume);
    let limits = SolverLimits::new(volume, queue_cap);
    let mut queued_total = 0;
    if oracle {
        seed_boundary_full_volume_oracle(
            blocks,
            prior,
            bounds,
            channel,
            profile,
            &mut output,
            &mut queue,
            &mut direct,
            limits,
            &mut queued_total,
        )?;
    } else {
        seed_boundary_from_halo(
            blocks,
            prior,
            bounds,
            channel,
            profile,
            &mut output,
            &mut queue,
            &mut direct,
            limits,
            &mut queued_total,
        )?;
    }
    Ok(snapshot_seed(
        bounds,
        channel,
        &output,
        &direct,
        &queue,
        queued_total,
    ))
}

fn boundary_fixture(bounds: LightBounds) -> (RecordingBlocks, RecordingPrior) {
    let mut blocks = RecordingBlocks::default();
    blocks.fill_air(bounds);
    let mut prior = RecordingPrior::default();
    let block_halo = BlockPos::new(bounds.min.x - 1, bounds.min.y, bounds.min.z);
    blocks
        .samples
        .insert(block_halo, LightBlockSample::KnownAir);
    prior.boundary.insert(
        (
            bounds.dimension,
            block_halo,
            light_channel_index(LightChannel::Block),
        ),
        BoundaryLightSample::trusted(12, false).unwrap(),
    );
    let sky_halo = BlockPos::new(bounds.max.x, bounds.max.y + 1, bounds.max.z);
    blocks.samples.insert(sky_halo, LightBlockSample::KnownAir);
    prior.boundary.insert(
        (
            bounds.dimension,
            sky_halo,
            light_channel_index(LightChannel::Sky),
        ),
        BoundaryLightSample::trusted(15, true).unwrap(),
    );
    let untrusted_halo = BlockPos::new(bounds.max.x + 1, bounds.min.y, bounds.min.z);
    blocks
        .samples
        .insert(untrusted_halo, LightBlockSample::KnownAir);
    prior.boundary.insert(
        (
            bounds.dimension,
            untrusted_halo,
            light_channel_index(LightChannel::Block),
        ),
        BoundaryLightSample::untrusted(),
    );
    let zero_halo = BlockPos::new(bounds.min.x, bounds.min.y - 1, bounds.max.z);
    blocks.samples.insert(zero_halo, LightBlockSample::KnownAir);
    prior.boundary.insert(
        (
            bounds.dimension,
            zero_halo,
            light_channel_index(LightChannel::Block),
        ),
        BoundaryLightSample::trusted(0, false).unwrap(),
    );
    (blocks, prior)
}

#[test]
fn boundary_seed_matches_full_scan_oracle_without_sampling_interior_cells() {
    let bounds =
        LightBounds::new(7, BlockPos::new(-18, -5, 31), BlockPos::new(-15, -2, 34)).unwrap();

    for (channel, profile) in [
        (LightChannel::Block, DimensionLightProfile::Nether),
        (
            LightChannel::Sky,
            DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
        ),
    ] {
        let (oracle_blocks, oracle_prior) = boundary_fixture(bounds);
        let expected = run_seed(
            true,
            &oracle_blocks,
            &oracle_prior,
            bounds,
            channel,
            profile,
            100,
        )
        .unwrap();
        assert!(!expected.queue.is_empty());
        assert!(!oracle_prior.reads.borrow().is_empty());
        assert!(
            oracle_blocks
                .reads
                .borrow()
                .iter()
                .filter(|&&position| bounds.contains(position))
                .any(|&position| !is_boundary(bounds, position)),
            "the full-volume oracle must preserve legacy interior sampling"
        );

        let (actual_blocks, actual_prior) = boundary_fixture(bounds);
        let actual = run_seed(
            false,
            &actual_blocks,
            &actual_prior,
            bounds,
            channel,
            profile,
            100,
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(*actual_prior.reads.borrow(), *oracle_prior.reads.borrow());
        assert!(!actual_prior.reads.borrow().is_empty());
        assert!(
            actual_blocks
                .reads
                .borrow()
                .iter()
                .filter(|&&position| bounds.contains(position))
                .all(|&position| is_boundary(bounds, position)),
            "boundary seeding sampled an interior cell"
        );
    }
}

#[test]
fn boundary_seed_positive_cases_preserve_levels_provenance_and_semantic_filters() {
    let bounds =
        LightBounds::new(7, BlockPos::new(-18, -5, 31), BlockPos::new(-15, -2, 34)).unwrap();
    let block_target = bounds.min;
    let block_halo = BlockPos::new(block_target.x - 1, block_target.y, block_target.z);
    let sky_target = bounds.max;
    let sky_halo = BlockPos::new(sky_target.x, sky_target.y + 1, sky_target.z);
    let unknown_boundary = BlockPos::new(bounds.max.x, bounds.min.y, bounds.max.z);
    let unknown_boundary_halo = BlockPos::new(
        unknown_boundary.x,
        unknown_boundary.y,
        unknown_boundary.z + 1,
    );
    let unknown_interior = BlockPos::new(bounds.min.x + 1, bounds.min.y + 1, bounds.min.z + 1);

    let (mut block_blocks, mut block_prior) = boundary_fixture(bounds);
    block_blocks.samples.insert(
        block_target,
        LightBlockSample::Resident(LightProperties::new(0, 3).unwrap()),
    );
    block_blocks.samples.remove(&unknown_boundary);
    block_blocks.samples.remove(&unknown_interior);
    block_blocks
        .samples
        .insert(unknown_boundary_halo, LightBlockSample::KnownAir);
    block_prior.boundary.insert(
        (
            bounds.dimension,
            unknown_boundary_halo,
            light_channel_index(LightChannel::Block),
        ),
        BoundaryLightSample::trusted(14, false).unwrap(),
    );
    let block_expected = run_seed(
        true,
        &block_blocks,
        &block_prior,
        bounds,
        LightChannel::Block,
        DimensionLightProfile::Nether,
        100,
    )
    .unwrap();
    let block_actual = run_seed(
        false,
        &block_blocks,
        &block_prior,
        bounds,
        LightChannel::Block,
        DimensionLightProfile::Nether,
        100,
    )
    .unwrap();
    assert_eq!(block_actual, block_expected);
    assert_eq!(block_actual.queue, vec![(block_target, false)]);
    assert_eq!(block_actual.queued_total, 1);
    assert_eq!(
        block_actual
            .values
            .iter()
            .find(|(position, _, _)| *position == block_target),
        Some(&(block_target, 9, false))
    );
    assert!(block_prior.reads.borrow().contains(&(
        bounds.dimension,
        block_halo,
        LightChannel::Block
    )));
    assert!(block_prior.reads.borrow().contains(&(
        bounds.dimension,
        BlockPos::new(bounds.max.x + 1, bounds.min.y, bounds.min.z),
        LightChannel::Block
    )));
    assert!(block_prior.reads.borrow().contains(&(
        bounds.dimension,
        BlockPos::new(bounds.min.x, bounds.min.y - 1, bounds.max.z),
        LightChannel::Block
    )));
    assert!(!block_prior.reads.borrow().contains(&(
        bounds.dimension,
        unknown_boundary_halo,
        LightChannel::Block
    )));
    assert_eq!(
        block_actual
            .values
            .iter()
            .find(|(position, _, _)| *position == unknown_boundary),
        Some(&(unknown_boundary, 0, false))
    );
    assert!(block_blocks.reads.borrow().contains(&unknown_boundary));
    assert!(block_blocks.reads.borrow().contains(&unknown_interior));

    let (sky_blocks, sky_prior) = boundary_fixture(bounds);
    let sky_expected = run_seed(
        true,
        &sky_blocks,
        &sky_prior,
        bounds,
        LightChannel::Sky,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        100,
    )
    .unwrap();
    let sky_actual = run_seed(
        false,
        &sky_blocks,
        &sky_prior,
        bounds,
        LightChannel::Sky,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        100,
    )
    .unwrap();
    assert_eq!(sky_actual, sky_expected);
    assert_eq!(sky_actual.queue, vec![(sky_target, true)]);
    assert_eq!(sky_actual.queued_total, 1);
    assert_eq!(
        sky_actual
            .values
            .iter()
            .find(|(position, _, _)| *position == sky_target),
        Some(&(sky_target, 15, true))
    );
    assert!(
        sky_prior
            .reads
            .borrow()
            .contains(&(bounds.dimension, sky_halo, LightChannel::Sky))
    );
}

#[test]
fn boundary_seed_oracle_covers_degenerate_axes_profiles_and_queue_cap() {
    for (dimension, min, max) in [
        (-2, BlockPos::new(-3, 5, 9), BlockPos::new(-3, 5, 9)),
        (3, BlockPos::new(2, -7, 4), BlockPos::new(2, -4, 6)),
        (9, BlockPos::new(-8, 2, 11), BlockPos::new(-6, 2, 13)),
    ] {
        let bounds = LightBounds::new(dimension, min, max).unwrap();
        let (oracle_blocks, oracle_prior) = boundary_fixture(bounds);
        let (actual_blocks, actual_prior) = boundary_fixture(bounds);
        for (channel, profile) in [
            (LightChannel::Block, DimensionLightProfile::End),
            (
                LightChannel::Sky,
                DimensionLightProfile::Overworld {
                    direct_sky_down: false,
                },
            ),
            (LightChannel::Sky, DimensionLightProfile::Nether),
        ] {
            let expected = run_seed(
                true,
                &oracle_blocks,
                &oracle_prior,
                bounds,
                channel,
                profile,
                100,
            );
            let actual = run_seed(
                false,
                &actual_blocks,
                &actual_prior,
                bounds,
                channel,
                profile,
                100,
            );
            assert_eq!(actual, expected);
        }
    }

    let bounds = LightBounds::new(5, BlockPos::new(0, 0, 0), BlockPos::new(0, 0, 0)).unwrap();
    let (oracle_blocks, oracle_prior) = boundary_fixture(bounds);
    let (actual_blocks, actual_prior) = boundary_fixture(bounds);
    assert_eq!(
        run_seed(
            false,
            &actual_blocks,
            &actual_prior,
            bounds,
            LightChannel::Block,
            DimensionLightProfile::Nether,
            0,
        ),
        Err(LightSolveError::QueueLimitExceeded { max: 0 })
    );
    assert_eq!(
        run_seed(
            true,
            &oracle_blocks,
            &oracle_prior,
            bounds,
            LightChannel::Block,
            DimensionLightProfile::Nether,
            0,
        ),
        Err(LightSolveError::QueueLimitExceeded { max: 0 })
    );
}

struct BaselineBlocks;

impl LightBlockAccess for BaselineBlocks {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if (position.x.wrapping_mul(3) ^ position.y ^ position.z.wrapping_mul(5)) & 31 == 0 {
            LightBlockSample::Resident(LightProperties::new(9, 2).unwrap())
        } else {
            LightBlockSample::KnownAir
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position.y.rem_euclid(23) == 0 {
            15
        } else {
            0
        }
    }
}

fn fingerprint(output: &LightSolveOutput) -> (u64, u64, usize, LightSolveStats) {
    let mut light_hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut provenance_hash = light_hash;
    for position in output.bounds.positions() {
        for channel in [LightChannel::Block, LightChannel::Sky] {
            light_hash ^= u64::from(output.light_at(position, channel));
            light_hash = light_hash.wrapping_mul(0x100_0000_01b3);
        }
        provenance_hash ^= u64::from(output.direct_sky.contains(&position));
        provenance_hash = provenance_hash.wrapping_mul(0x100_0000_01b3);
    }
    for (key, light) in output.sub_chunks() {
        for value in [key.dimension, key.x, key.y, key.z] {
            light_hash ^= value as u64;
            light_hash = light_hash.wrapping_mul(0x100_0000_01b3);
        }
        light_hash ^= light.generation();
        light_hash = light_hash.wrapping_mul(0x100_0000_01b3);
    }
    (
        light_hash,
        provenance_hash,
        output.sub_chunks().len(),
        output.stats(),
    )
}

struct BenchmarkBlocks {
    bounds: LightBounds,
    block_halo: BlockPos,
    sky_halo: BlockPos,
}

impl LightBlockAccess for BenchmarkBlocks {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if self.bounds.contains(position)
            || position == self.block_halo
            || position == self.sky_halo
        {
            LightBlockSample::KnownAir
        } else {
            LightBlockSample::Unknown
        }
    }
}

struct BenchmarkPrior {
    dimension: i32,
    block_halo: BlockPos,
    sky_halo: BlockPos,
}

impl LightReadAccess for BenchmarkPrior {
    fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
        0
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if dimension != self.dimension {
            return BoundaryLightSample::unknown();
        }
        match (position, channel) {
            (position, LightChannel::Block) if position == self.block_halo => {
                BoundaryLightSample::trusted(12, false).unwrap()
            }
            (position, LightChannel::Sky) if position == self.sky_halo => {
                BoundaryLightSample::trusted(15, true).unwrap()
            }
            _ => BoundaryLightSample::unknown(),
        }
    }
}

fn measure_seed_scans(bounds: LightBounds, full_volume: bool, rounds: usize) -> Duration {
    let volume = bounds.volume().unwrap();
    let block_halo = BlockPos::new(bounds.min.x - 1, bounds.min.y, bounds.min.z);
    let sky_halo = BlockPos::new(bounds.max.x, bounds.max.y + 1, bounds.max.z);
    let blocks = BenchmarkBlocks {
        bounds,
        block_halo,
        sky_halo,
    };
    let prior = BenchmarkPrior {
        dimension: bounds.dimension,
        block_halo,
        sky_halo,
    };
    let mut output = MutableOutput::new(bounds, 41, volume);
    let mut block_queue = VecDeque::new();
    let mut sky_queue = VecDeque::new();
    let mut direct = DensePositionSet::new(bounds, volume);
    let limits = SolverLimits::new(volume, usize::MAX);
    let mut queued_total = 0;
    let profile = DimensionLightProfile::Overworld {
        direct_sky_down: true,
    };

    let started = Instant::now();
    for _ in 0..rounds {
        for (channel, queue) in [
            (LightChannel::Block, &mut block_queue),
            (LightChannel::Sky, &mut sky_queue),
        ] {
            if full_volume {
                seed_boundary_full_volume_oracle(
                    &blocks,
                    &prior,
                    bounds,
                    channel,
                    profile,
                    &mut output,
                    queue,
                    &mut direct,
                    limits,
                    &mut queued_total,
                )
                .unwrap();
            } else {
                seed_boundary_from_halo(
                    &blocks,
                    &prior,
                    bounds,
                    channel,
                    profile,
                    &mut output,
                    queue,
                    &mut direct,
                    limits,
                    &mut queued_total,
                )
                .unwrap();
            }
            queue.clear();
        }
        black_box(output.get(bounds.min, LightChannel::Block));
        black_box(output.get(bounds.max, LightChannel::Sky));
    }
    started.elapsed()
}

fn median_nanos(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
#[ignore = "release-only matched boundary seed scan benchmark"]
fn release_boundary_seed_scan_benchmark() {
    for (label, bounds, rounds) in [
        (
            "16x16x16",
            LightBounds::new(0, BlockPos::new(-16, 0, 17), BlockPos::new(-1, 15, 32)).unwrap(),
            512,
        ),
        (
            "16x384x16",
            LightBounds::new(0, BlockPos::new(-16, -64, 17), BlockPos::new(-1, 319, 32)).unwrap(),
            32,
        ),
    ] {
        let mut full_volume = Vec::new();
        let mut boundary_only = Vec::new();
        for sample in 0..9 {
            if sample % 2 == 0 {
                full_volume.push(measure_seed_scans(bounds, true, rounds).as_nanos());
                boundary_only.push(measure_seed_scans(bounds, false, rounds).as_nanos());
            } else {
                boundary_only.push(measure_seed_scans(bounds, false, rounds).as_nanos());
                full_volume.push(measure_seed_scans(bounds, true, rounds).as_nanos());
            }
        }
        let full_volume_ns = median_nanos(full_volume);
        let boundary_only_ns = median_nanos(boundary_only);
        eprintln!(
            "{label} rounds={rounds}: full-volume={full_volume_ns}ns boundary-only={boundary_only_ns}ns"
        );
    }
}

#[test]
#[ignore = "release-only deterministic boundary-scan baseline"]
fn release_boundary_scan_full_solve_fingerprints() {
    for (label, bounds) in [
        (
            "16x16x16",
            LightBounds::new(0, BlockPos::new(-16, 0, 17), BlockPos::new(-1, 15, 32)).unwrap(),
        ),
        (
            "16x384x16",
            LightBounds::new(0, BlockPos::new(-16, -64, 17), BlockPos::new(-1, 319, 32)).unwrap(),
        ),
    ] {
        let volume = bounds.volume().unwrap();
        let output = solve_light(
            &BaselineBlocks,
            &EmptyLight,
            bounds,
            0x1234_5678,
            DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
            SolverLimits::new(volume, volume.saturating_mul(64)),
        )
        .unwrap();
        let expected = match label {
            "16x16x16" => (
                11_701_639_296_605_938_954,
                3_729_786_652_408_525_661,
                2,
                LightSolveStats {
                    darken_seeded: 0,
                    darken_dequeued: 0,
                    increase_dequeued: 7_818,
                    queue_peak: 1_305,
                },
            ),
            "16x384x16" => (
                12_412_752_408_638_411_408,
                17_288_014_161_954_932_521,
                48,
                LightSolveStats {
                    darken_seeded: 0,
                    darken_dequeued: 0,
                    increase_dequeued: 263_425,
                    queue_peak: 31_173,
                },
            ),
            _ => unreachable!(),
        };
        assert_eq!(fingerprint(&output), expected, "{label}");
    }
}
