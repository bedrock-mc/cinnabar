use std::collections::BTreeMap;

use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Cell {
    block: u8,
    sky: u8,
    direct: bool,
}

struct Fixture {
    bounds: LightBounds,
    salt: u32,
}

impl LightBlockAccess for Fixture {
    /// Stable categories include unknown cells, emitters, opaque blocks and partial filters.
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        let pattern = (position.x as u32).wrapping_mul(73_856_093)
            ^ (position.y as u32).wrapping_mul(19_349_663)
            ^ (position.z as u32).wrapping_mul(83_492_791)
            ^ self.salt;
        match pattern % 13 {
            0 => LightBlockSample::Unknown,
            1 => LightBlockSample::Resident(LightProperties::new(9, 2).unwrap()),
            2 => LightBlockSample::Resident(LightProperties::new(15, 0).unwrap()),
            3 => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
            4 => LightBlockSample::Resident(LightProperties::new(0, 3).unwrap()),
            _ => LightBlockSample::KnownAir,
        }
    }

    /// The top layer supplies the local sky source for each profile.
    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position.y == self.bounds.max.y {
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

impl LightReadAccess for Prior<'_> {
    /// Retained interior samples exercise darkening after each edit.
    fn read_light(&self, _dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        self.output
            .map_or(0, |output| output.light_at(position, channel))
    }

    /// Retains downward-sky provenance from the previous solved field.
    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        self.output
            .is_some_and(|output| output.has_direct_sky_provenance(dimension, position))
    }

    /// Seeds side block light and downward sky without trusting other halo cells.
    fn boundary_light(
        &self,
        _dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if position.x < self.fixture.bounds.min.x {
            BoundaryLightSample::trusted(
                if channel == LightChannel::Block {
                    12
                } else {
                    8
                },
                false,
            )
            .unwrap()
        } else if position.y > self.fixture.bounds.max.y && channel == LightChannel::Sky {
            BoundaryLightSample::trusted(15, true).unwrap()
        } else {
            BoundaryLightSample::unknown()
        }
    }
}

/// Relaxes the complete field repeatedly, independently of queue ordering or deduplication.
fn scalar_fixed_point(
    fixture: &Fixture,
    prior: &Prior<'_>,
    profile: DimensionLightProfile,
) -> BTreeMap<BlockPos, Cell> {
    let mut field = BTreeMap::new();
    for position in fixture.bounds.positions() {
        let sample = fixture.sample(position);
        let Some(filter) = sample.filter() else {
            continue;
        };
        let sky = if profile.allows_sky() {
            fixture.sky_seed(position).saturating_sub(filter)
        } else {
            0
        };
        field.insert(
            position,
            Cell {
                block: sample.emission(),
                sky,
                direct: profile.direct_sky_down() && sky == 15 && filter == 0,
            },
        );
    }
    loop {
        let before = field.clone();
        for (&position, cell) in &mut field {
            let filter = fixture.sample(position).filter().unwrap();
            for offset in solve::NEIGHBOURS {
                let Some(source) = position.checked_offset(offset) else {
                    continue;
                };
                if fixture.sample(source).filter().is_none() {
                    continue;
                }
                let incoming = if fixture.bounds.contains(source) {
                    before.get(&source).copied().unwrap_or_default()
                } else {
                    let block = prior
                        .boundary_light(fixture.bounds.dimension, source, LightChannel::Block)
                        .trusted_parts()
                        .map_or(0, |(level, _)| level);
                    let (sky, direct) = prior
                        .boundary_light(fixture.bounds.dimension, source, LightChannel::Sky)
                        .trusted_parts()
                        .unwrap_or((0, false));
                    Cell { block, sky, direct }
                };
                cell.block = cell.block.max(incoming.block.saturating_sub(filter.max(1)));
                if !profile.allows_sky() {
                    continue;
                }
                let direct = profile.direct_sky_down()
                    && offset == [0, 1, 0]
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

#[test]
fn queued_propagation_matches_scalar_relaxation_across_edits_and_profiles() {
    for profile in [
        DimensionLightProfile::Nether,
        DimensionLightProfile::Overworld {
            direct_sky_down: false,
        },
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
    ] {
        for min in [
            BlockPos::new(-2, -3, -2),
            BlockPos::new(i32::MIN, i32::MAX - 4, -2),
        ] {
            let bounds =
                LightBounds::new(0, min, BlockPos::new(min.x + 3, min.y + 4, min.z + 3)).unwrap();
            let mut previous = None;
            let mut scratch = LightSolverScratch::default();
            for generation in 0..16 {
                let fixture = Fixture {
                    bounds,
                    salt: generation * 37,
                };
                let prior = Prior {
                    output: previous.as_ref(),
                    fixture: &fixture,
                };
                let expected = scalar_fixed_point(&fixture, &prior, profile);
                let output = solve_light_with_scratch(
                    &fixture,
                    &prior,
                    bounds,
                    u64::from(generation),
                    profile,
                    SolverLimits::new(80, 10_000),
                    &mut scratch,
                )
                .unwrap();
                for position in bounds.positions() {
                    let cell = expected.get(&position).copied().unwrap_or_default();
                    assert_eq!(
                        output.light_at(position, LightChannel::Block),
                        cell.block,
                        "block at {position:?}, generation {generation}, profile {profile:?}"
                    );
                    assert_eq!(
                        output.light_at(position, LightChannel::Sky),
                        cell.sky,
                        "sky at {position:?}, generation {generation}, profile {profile:?}"
                    );
                    assert_eq!(
                        output.has_direct_sky_provenance(0, position),
                        cell.direct,
                        "provenance at {position:?}, generation {generation}, profile {profile:?}"
                    );
                }
                previous = Some(output);
            }
        }
    }
}
