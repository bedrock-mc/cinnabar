use std::{cell::RefCell, collections::BTreeMap};

use world::{
    BlockPos, BoundaryLightSample, DimensionLightProfile, LightBlockAccess, LightBlockSample,
    LightBounds, LightChannel, LightProperties, LightReadAccess, LightSolveStats, SolverLimits,
    SubChunkKey, SubChunkLight, solve_light,
};

const PRIOR: [(u8, u8, bool); 27] = [
    (15, 15, true),
    (14, 15, true),
    (13, 15, true),
    (14, 15, true),
    (13, 15, true),
    (12, 15, true),
    (13, 15, true),
    (12, 15, true),
    (11, 15, true),
    (14, 15, true),
    (13, 14, false),
    (12, 15, true),
    (13, 15, true),
    (12, 14, false),
    (11, 15, true),
    (12, 15, true),
    (0, 0, false),
    (10, 15, true),
    (13, 15, true),
    (12, 15, true),
    (11, 15, true),
    (12, 15, true),
    (11, 15, true),
    (10, 15, true),
    (11, 15, true),
    (10, 15, true),
    (9, 15, true),
];

const GOLDEN: [(u8, u8, bool); 27] = [
    (8, 14, false),
    (9, 15, true),
    (10, 15, true),
    (7, 14, false),
    (8, 15, true),
    (9, 15, true),
    (0, 0, false),
    (7, 15, true),
    (8, 15, true),
    (9, 15, true),
    (10, 15, true),
    (11, 15, true),
    (8, 15, true),
    (9, 15, true),
    (10, 15, true),
    (7, 15, true),
    (8, 15, true),
    (9, 15, true),
    (10, 15, true),
    (11, 15, true),
    (12, 15, true),
    (9, 15, true),
    (10, 15, true),
    (11, 15, true),
    (8, 15, true),
    (9, 15, true),
    (10, 15, true),
];

#[derive(Default)]
struct FixtureBlocks {
    samples: BTreeMap<BlockPos, LightBlockSample>,
    sky: BTreeMap<BlockPos, u8>,
}

impl FixtureBlocks {
    fn air_box(&mut self, min: BlockPos, max: BlockPos) {
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    self.samples
                        .insert(BlockPos::new(x, y, z), LightBlockSample::KnownAir);
                }
            }
        }
    }

    fn block(&mut self, position: BlockPos, emission: u8, filter: u8) {
        self.samples.insert(
            position,
            LightBlockSample::Resident(LightProperties::new(emission, filter).unwrap()),
        );
    }
}

impl LightBlockAccess for FixtureBlocks {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        self.samples
            .get(&position)
            .copied()
            .unwrap_or(LightBlockSample::Unknown)
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        self.sky.get(&position).copied().unwrap_or(0)
    }
}

struct GoldenPrior;

impl GoldenPrior {
    fn value(position: BlockPos) -> Option<(u8, u8, bool)> {
        if !(0..=2).contains(&position.x)
            || !(0..=2).contains(&position.y)
            || !(0..=2).contains(&position.z)
        {
            return None;
        }
        let index = usize::try_from(position.x * 9 + position.y * 3 + position.z).unwrap();
        Some(PRIOR[index])
    }
}

impl LightReadAccess for GoldenPrior {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        if dimension != 0 {
            return 0;
        }
        let Some((block, sky, _)) = Self::value(position) else {
            return 0;
        };
        match channel {
            LightChannel::Block => block,
            LightChannel::Sky => sky,
        }
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        dimension == 0 && Self::value(position).is_some_and(|(_, _, direct)| direct)
    }
}

struct CountingPrior<'a> {
    inner: &'a GoldenPrior,
    light_reads: RefCell<Vec<(BlockPos, LightChannel)>>,
    provenance_reads: RefCell<Vec<BlockPos>>,
}

impl<'a> CountingPrior<'a> {
    fn new(inner: &'a GoldenPrior) -> Self {
        Self {
            inner,
            light_reads: RefCell::new(Vec::new()),
            provenance_reads: RefCell::new(Vec::new()),
        }
    }

    fn light_read_count(&self, position: BlockPos, channel: LightChannel) -> usize {
        self.light_reads
            .borrow()
            .iter()
            .filter(|&&(read_position, read_channel)| {
                read_position == position && read_channel == channel
            })
            .count()
    }

    fn provenance_read_count(&self, position: BlockPos) -> usize {
        self.provenance_reads
            .borrow()
            .iter()
            .filter(|&&read_position| read_position == position)
            .count()
    }
}

impl LightReadAccess for CountingPrior<'_> {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        self.light_reads.borrow_mut().push((position, channel));
        self.inner.read_light(dimension, position, channel)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        self.provenance_reads.borrow_mut().push(position);
        self.inner.has_direct_sky_provenance(dimension, position)
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        self.inner.boundary_light(dimension, position, channel)
    }
}

fn region() -> LightBounds {
    LightBounds::new(0, BlockPos::new(0, 0, 0), BlockPos::new(2, 2, 2)).unwrap()
}

fn limits() -> SolverLimits {
    SolverLimits::new(27, 10_000)
}

fn positions() -> impl Iterator<Item = BlockPos> {
    (0..=2).flat_map(|x| (0..=2).flat_map(move |y| (0..=2).map(move |z| BlockPos::new(x, y, z))))
}

#[test]
fn interior_prior_reads_are_lazy_bounded_and_output_equivalent() {
    let mut replacement = FixtureBlocks::default();
    replacement.air_box(BlockPos::new(0, 0, 0), BlockPos::new(2, 2, 2));
    replacement.block(BlockPos::new(2, 0, 2), 12, 0);
    replacement.block(BlockPos::new(0, 2, 0), 0, 15);
    for x in 0..=2 {
        for z in 0..=2 {
            replacement.sky.insert(BlockPos::new(x, 2, z), 15);
        }
    }
    let prior = GoldenPrior;
    let counting_prior = CountingPrior::new(&prior);
    let actual = solve_light(
        &replacement,
        &counting_prior,
        region(),
        2,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        limits(),
    )
    .unwrap();

    let mut golden_sub_chunk = SubChunkLight::dark(2);
    for (position, (block, sky, direct_sky)) in positions().zip(GOLDEN) {
        let x = u8::try_from(position.x).unwrap();
        let y = u8::try_from(position.y).unwrap();
        let z = u8::try_from(position.z).unwrap();
        golden_sub_chunk
            .set(LightChannel::Block, x, y, z, block)
            .unwrap();
        golden_sub_chunk
            .set(LightChannel::Sky, x, y, z, sky)
            .unwrap();
        assert_eq!(actual.light_at(position, LightChannel::Block), block);
        assert_eq!(actual.light_at(position, LightChannel::Sky), sky);
        assert_eq!(actual.has_direct_sky_provenance(0, position), direct_sky);
        for channel in [LightChannel::Block, LightChannel::Sky] {
            assert!(
                counting_prior.light_read_count(position, channel) <= 1,
                "prior {channel:?} at {position:?} was read more than once"
            );
        }
        assert!(
            counting_prior.provenance_read_count(position) <= 1,
            "prior provenance at {position:?} was read more than once"
        );
    }
    assert_eq!(actual.sub_chunks().len(), 1);
    assert_eq!(
        actual.sub_chunks()[&SubChunkKey::new(0, 0, 0, 0)].as_ref(),
        &golden_sub_chunk
    );
    assert_eq!(
        actual.stats(),
        LightSolveStats {
            darken_seeded: 3,
            darken_dequeued: 28,
            increase_dequeued: 54,
            queue_peak: 24,
        }
    );
    assert_eq!(
        counting_prior.light_reads.borrow().len(),
        27 * 2,
        "every known interior light value is loaded exactly once"
    );
    assert!(
        (1..=27).contains(&counting_prior.provenance_reads.borrow().len()),
        "provenance must be read on demand at most once per interior position"
    );
}
