use std::collections::BTreeMap;

use world::{
    BlockPos, BoundaryLightSample, DimensionLightProfile, LightBlockAccess, LightBlockSample,
    LightBounds, LightChannel, LightProperties, LightReadAccess, LightSolveOutput,
    LightSolverScratch, SUB_CHUNK_SIDE, SolverLimits, solve_light_with_scratch,
};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Cell {
    block: u8,
    sky: u8,
    direct: bool,
}

struct Fixture {
    bounds: LightBounds,
    salt: Option<u32>,
    local_sky: bool,
    trusted_halo: bool,
}

impl LightBlockAccess for Fixture {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        let Some(salt) = self.salt else {
            return LightBlockSample::KnownAir;
        };
        let pattern = (position.x as u32).wrapping_mul(73_856_093)
            ^ (position.y as u32).wrapping_mul(19_349_663)
            ^ (position.z as u32).wrapping_mul(83_492_791)
            ^ salt;
        match pattern % 13 {
            0 => LightBlockSample::Unknown,
            1 => LightBlockSample::Resident(LightProperties::new(9, 2).unwrap()),
            2 => LightBlockSample::Resident(LightProperties::new(15, 0).unwrap()),
            3 => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
            4 => LightBlockSample::Resident(LightProperties::new(0, 3).unwrap()),
            _ => LightBlockSample::KnownAir,
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if self.local_sky && position.y == self.bounds.max().y {
            15
        } else {
            0
        }
    }
}

struct Prior<'a> {
    output: Option<&'a LightSolveOutput>,
    fixture: &'a Fixture,
}

impl Prior<'_> {
    fn trusted_cell(&self, position: BlockPos) -> Option<Cell> {
        if !self.fixture.trusted_halo {
            return None;
        }
        if position.x < self.fixture.bounds.min().x {
            Some(Cell {
                block: 12,
                sky: 8,
                direct: false,
            })
        } else if position.y > self.fixture.bounds.max().y {
            Some(Cell {
                block: 0,
                sky: 15,
                direct: true,
            })
        } else {
            None
        }
    }
}

impl LightReadAccess for Prior<'_> {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        self.output
            .map_or(0, |output| output.read_light(dimension, position, channel))
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        self.output
            .is_some_and(|output| output.has_direct_sky_provenance(dimension, position))
    }

    fn boundary_light(
        &self,
        _dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if let Some(cell) = self.trusted_cell(position) {
            let (level, direct) = match channel {
                LightChannel::Block => (cell.block, false),
                LightChannel::Sky => (cell.sky, cell.direct),
            };
            BoundaryLightSample::trusted(level, direct).unwrap()
        } else if position.x > self.fixture.bounds.max().x {
            BoundaryLightSample::untrusted()
        } else {
            BoundaryLightSample::unknown()
        }
    }
}

fn properties(sample: LightBlockSample) -> Option<(u8, u8)> {
    match sample {
        LightBlockSample::Unknown => None,
        LightBlockSample::KnownAir => Some((0, 0)),
        LightBlockSample::Resident(properties) => {
            Some((properties.emission(), properties.filter()))
        }
    }
}

fn positions(bounds: LightBounds) -> impl Iterator<Item = BlockPos> {
    (bounds.min().x..=bounds.max().x).flat_map(move |x| {
        (bounds.min().y..=bounds.max().y).flat_map(move |y| {
            (bounds.min().z..=bounds.max().z).map(move |z| BlockPos::new(x, y, z))
        })
    })
}

fn neighbours(position: BlockPos) -> impl Iterator<Item = BlockPos> {
    [
        [-1, 0, 0],
        [1, 0, 0],
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
    ]
    .into_iter()
    .filter_map(move |[x, y, z]| {
        Some(BlockPos::new(
            position.x.checked_add(x)?,
            position.y.checked_add(y)?,
            position.z.checked_add(z)?,
        ))
    })
}

// Repeated whole-field relaxation is independent of the solver's queue and dense indexing.
fn scalar_fixed_point(
    fixture: &Fixture,
    prior: &Prior<'_>,
    profile: DimensionLightProfile,
) -> BTreeMap<BlockPos, Cell> {
    let allows_sky = matches!(profile, DimensionLightProfile::Overworld { .. });
    let direct_sky_down = matches!(
        profile,
        DimensionLightProfile::Overworld {
            direct_sky_down: true
        }
    );
    let mut field = BTreeMap::new();
    for position in positions(fixture.bounds) {
        let Some((block, filter)) = properties(fixture.sample(position)) else {
            continue;
        };
        let sky = if allows_sky {
            fixture.sky_seed(position).saturating_sub(filter)
        } else {
            0
        };
        field.insert(
            position,
            Cell {
                block,
                sky,
                direct: direct_sky_down && sky == 15 && filter == 0,
            },
        );
    }
    loop {
        let before = field.clone();
        for (&position, cell) in &mut field {
            let (_, filter) = properties(fixture.sample(position)).unwrap();
            for source in neighbours(position) {
                if properties(fixture.sample(source)).is_none() {
                    continue;
                }
                let incoming = if fixture.bounds.contains(source) {
                    before.get(&source).copied().unwrap_or_default()
                } else {
                    prior.trusted_cell(source).unwrap_or_default()
                };
                cell.block = cell.block.max(incoming.block.saturating_sub(filter.max(1)));
                if !allows_sky {
                    continue;
                }
                let direct = direct_sky_down
                    && source.y > position.y
                    && incoming.sky == 15
                    && incoming.direct
                    && filter == 0;
                cell.sky = cell.sky.max(if direct {
                    15
                } else {
                    incoming.sky.saturating_sub(filter.max(1))
                });
                cell.direct |= direct;
            }
        }
        if field == before {
            return field;
        }
    }
}

fn assert_matches_oracle(
    fixture: &Fixture,
    previous: Option<&LightSolveOutput>,
    profile: DimensionLightProfile,
    generation: u64,
    scratch: &mut LightSolverScratch,
) -> LightSolveOutput {
    let prior = Prior {
        output: previous,
        fixture,
    };
    let expected = scalar_fixed_point(fixture, &prior, profile);
    let output = solve_light_with_scratch(
        fixture,
        &prior,
        fixture.bounds,
        generation,
        profile,
        SolverLimits::new(4096, 1_000_000),
        scratch,
    )
    .unwrap();
    for position in positions(fixture.bounds) {
        let actual = Cell {
            block: output.light_at(position, LightChannel::Block),
            sky: output.light_at(position, LightChannel::Sky),
            direct: output.has_direct_sky_provenance(fixture.bounds.dimension(), position),
        };
        assert_eq!(
            actual,
            expected.get(&position).copied().unwrap_or_default(),
            "at {position:?}, generation {generation}, profile {profile:?}"
        );
    }
    output
}

#[test]
fn propagation_matches_scalar_relaxation_across_edits_profiles_and_coordinate_edges() {
    let side = SUB_CHUNK_SIDE as i32;
    for profile in [
        DimensionLightProfile::Nether,
        DimensionLightProfile::End,
        DimensionLightProfile::Overworld {
            direct_sky_down: false,
        },
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
    ] {
        for min in [
            BlockPos::new(-2, -3, -2),
            BlockPos::new(side - 2, side - 2, side - 2),
            BlockPos::new(i32::MIN, i32::MAX - 4, -2),
            BlockPos::new(i32::MAX - 3, i32::MIN, i32::MAX - 3),
        ] {
            let bounds =
                LightBounds::new(0, min, BlockPos::new(min.x + 3, min.y + 4, min.z + 3)).unwrap();
            let mut previous = None;
            let mut scratch = LightSolverScratch::default();
            for generation in 0..16_u32 {
                let fixture = Fixture {
                    bounds,
                    salt: Some(generation * 37),
                    local_sky: generation % 3 != 1,
                    trusted_halo: generation % 4 != 2,
                };
                previous = Some(assert_matches_oracle(
                    &fixture,
                    previous.as_ref(),
                    profile,
                    u64::from(generation),
                    &mut scratch,
                ));
            }
        }
    }
}

#[test]
fn top_only_direct_sky_crosses_section_seams_and_clears_when_source_is_removed() {
    let side = SUB_CHUNK_SIDE as i32;
    let bounds = LightBounds::new(
        0,
        BlockPos::new(-1, -side - 1, -1),
        BlockPos::new(1, side + 1, 1),
    )
    .unwrap();
    let profile = DimensionLightProfile::Overworld {
        direct_sky_down: true,
    };
    let mut previous = None;
    let mut scratch = LightSolverScratch::default();
    for (generation, local_sky) in [true, false, true].into_iter().enumerate() {
        let fixture = Fixture {
            bounds,
            salt: None,
            local_sky,
            trusted_halo: false,
        };
        let output = assert_matches_oracle(
            &fixture,
            previous.as_ref(),
            profile,
            generation as u64,
            &mut scratch,
        );
        assert_eq!(
            output.light_at(bounds.min(), LightChannel::Sky),
            if local_sky { 15 } else { 0 }
        );
        assert_eq!(output.has_direct_sky_provenance(0, bounds.min()), local_sky);
        previous = Some(output);
    }
}
