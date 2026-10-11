use std::collections::BTreeMap;

use super::*;

/// Blocks from a stable pattern with edited cells on top.
struct Edited {
    bounds: LightBounds,
    edits: BTreeMap<BlockPos, LightBlockSample>,
}

impl LightBlockAccess for Edited {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if let Some(sample) = self.edits.get(&position) {
            return *sample;
        }
        let pattern = (position.x as u32).wrapping_mul(73_856_093)
            ^ (position.y as u32).wrapping_mul(19_349_663)
            ^ (position.z as u32).wrapping_mul(83_492_791);
        match pattern % 29 {
            0 => LightBlockSample::Unknown,
            1..=4 => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
            5 => LightBlockSample::Resident(LightProperties::new(0, 3).unwrap()),
            6 => LightBlockSample::Resident(LightProperties::new(9, 2).unwrap()),
            _ => LightBlockSample::KnownAir,
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position.y == self.bounds.max.y {
            15
        } else {
            0
        }
    }
}

/// The previous solve inside the bounds; block light from the west and sky from above outside.
struct Prior<'a> {
    output: Option<&'a LightSolveOutput>,
    bounds: LightBounds,
}

impl LightReadAccess for Prior<'_> {
    fn read_light(&self, _dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        self.output
            .map_or(0, |output| output.light_at(position, channel))
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
        if position.x < self.bounds.min.x && channel == LightChannel::Block {
            BoundaryLightSample::trusted(11, false).unwrap()
        } else if position.y > self.bounds.max.y && channel == LightChannel::Sky {
            BoundaryLightSample::trusted(15, true).unwrap()
        } else {
            BoundaryLightSample::unknown()
        }
    }
}

#[test]
fn solving_only_changed_cells_matches_a_whole_solve_after_each_edit() {
    let kinds = [
        LightBlockSample::KnownAir,
        LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
        LightBlockSample::Resident(LightProperties::new(15, 0).unwrap()),
        LightBlockSample::Resident(LightProperties::new(14, 15).unwrap()),
        LightBlockSample::Resident(LightProperties::new(0, 3).unwrap()),
        LightBlockSample::Resident(LightProperties::new(9, 2).unwrap()),
    ];
    // Two sections stacked, so edits cross the section border at y = 0 as well as the bounds.
    let bounds = LightBounds::new(0, BlockPos::new(0, -16, 0), BlockPos::new(15, 15, 15)).unwrap();
    let limits = SolverLimits::new(16 * 32 * 16, 1_000_000);
    let mut random = 0x2545_f491_u32;
    let mut next = move |below: u32| {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        random % below
    };
    for profile in [
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        DimensionLightProfile::Overworld {
            direct_sky_down: false,
        },
        DimensionLightProfile::Nether,
    ] {
        let mut blocks = Edited {
            bounds,
            edits: BTreeMap::new(),
        };
        let mut scratch = LightSolverScratch::default();
        let mut previous = solve_light_with_scratch(
            &blocks,
            &Prior {
                output: None,
                bounds,
            },
            bounds,
            0,
            profile,
            limits,
            &mut scratch,
        )
        .unwrap();
        for generation in 1..=40 {
            let mut changed = Vec::new();
            for _ in 0..=next(3) {
                // Cells on a face, the section border or anywhere, about evenly.
                let mut axis = |len: u32| match next(3) {
                    0 => 0,
                    1 => len - 1,
                    _ => next(len),
                };
                let mut position = BlockPos::new(axis(16) as i32, 0, axis(16) as i32);
                position.y = match next(3) {
                    0 => -1 + next(2) as i32,
                    _ => next(32) as i32 - 16,
                };
                blocks
                    .edits
                    .insert(position, kinds[next(kinds.len() as u32) as usize]);
                changed.push(position);
            }
            let prior = Prior {
                output: Some(&previous),
                bounds,
            };
            let whole = solve_light_with_scratch(
                &blocks,
                &prior,
                bounds,
                generation,
                profile,
                limits,
                &mut scratch,
            )
            .unwrap();
            let seeded = solve_light_changes_with_scratch(
                &blocks,
                &prior,
                bounds,
                generation,
                profile,
                limits,
                &mut scratch,
                &changed,
            )
            .unwrap();
            for position in bounds.positions() {
                for channel in [LightChannel::Block, LightChannel::Sky] {
                    assert_eq!(
                        seeded.light_at(position, channel),
                        whole.light_at(position, channel),
                        "{channel:?} at {position:?} after edit {generation} {changed:?}, {profile:?}"
                    );
                }
                assert_eq!(
                    seeded.has_direct_sky_provenance(0, position),
                    whole.has_direct_sky_provenance(0, position),
                    "direct sky at {position:?} after edit {generation}, {profile:?}"
                );
            }
            previous = whole;
        }
    }
}
