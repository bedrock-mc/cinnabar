use std::collections::VecDeque;

use super::{
    cache::{BlockCacheScratch, PriorCacheScratch},
    output::MutableOutputScratch,
    queue::IncreaseQueue,
    solve::DarkenEntry,
};

/// Reusable worker-local buffers whose capacity follows the largest bounded solve.
/// Completed results own their data and never borrow these buffers.
#[derive(Default)]
pub struct LightSolverScratch {
    pub(super) blocks: BlockCacheScratch,
    pub(super) prior: PriorCacheScratch,
    pub(super) output: MutableOutputScratch,
    pub(super) darken: VecDeque<DarkenEntry>,
    pub(super) block_increase: IncreaseQueue,
    pub(super) sky_increase: IncreaseQueue,
}

#[cfg(test)]
mod tests {
    use crate::{
        BlockPos, DimensionLightProfile, EmptyLight, LightBlockAccess, LightBlockSample,
        LightBounds, LightChannel, LightProperties, LightReadAccess, LightSolveError,
        LightSolveOutput, SolverLimits, solve_light, solve_light_with_scratch,
    };

    use super::*;

    struct Fixture {
        bounds: LightBounds,
        salt: u32,
    }

    impl LightBlockAccess for Fixture {
        fn sample(&self, position: BlockPos) -> LightBlockSample {
            if !self.bounds.contains(position) {
                return LightBlockSample::Unknown;
            }
            let value = (position.x as u32).wrapping_mul(73_856_093)
                ^ (position.y as u32).wrapping_mul(19_349_663)
                ^ (position.z as u32).wrapping_mul(83_492_791)
                ^ self.salt;
            match value % 11 {
                0 => LightBlockSample::Unknown,
                1 => LightBlockSample::Resident(LightProperties::new(13, 0).unwrap()),
                2 => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
                _ => LightBlockSample::KnownAir,
            }
        }

        fn sky_seed(&self, position: BlockPos) -> u8 {
            if position.y == self.bounds.max().y {
                15
            } else {
                0
            }
        }
    }

    /// Compares both light channels, provenance, output generations, and queue work.
    fn assert_same(actual: &LightSolveOutput, expected: &LightSolveOutput) {
        assert_eq!(actual.sub_chunks(), expected.sub_chunks());
        assert_eq!(actual.stats(), expected.stats());
        for position in actual.bounds.positions() {
            assert_eq!(
                actual.has_direct_sky_provenance(actual.dimension, position),
                expected.has_direct_sky_provenance(expected.dimension, position),
            );
        }
    }

    /// Records retained buffer addresses and capacities without allocating.
    fn buffers(scratch: &LightSolverScratch) -> [(usize, usize); 11] {
        [
            (
                scratch.blocks.samples.as_ptr() as usize,
                scratch.blocks.samples.capacity(),
            ),
            (
                scratch.blocks.sky_seeds.as_ptr() as usize,
                scratch.blocks.sky_seeds.capacity(),
            ),
            (
                scratch.prior.light.as_ptr() as usize,
                scratch.prior.light.capacity(),
            ),
            (
                scratch.prior.direct_sky.as_ptr() as usize,
                scratch.prior.direct_sky.capacity(),
            ),
            (
                scratch.output.values.as_ptr() as usize,
                scratch.output.values.capacity(),
            ),
            (
                scratch.output.known.as_ptr() as usize,
                scratch.output.known.capacity(),
            ),
            (0, scratch.darken.capacity()),
            scratch.block_increase.buffers()[0],
            scratch.block_increase.buffers()[1],
            scratch.sky_increase.buffers()[0],
            scratch.sky_increase.buffers()[1],
        ]
    }

    #[test]
    fn warm_solves_retain_allocations_and_keep_returned_results_independent() {
        let bounds = LightBounds::new(0, BlockPos::new(-2, 1, -1), BlockPos::new(3, 6, 4)).unwrap();
        let fixture = Fixture { bounds, salt: 41 };
        let profile = DimensionLightProfile::Overworld {
            direct_sky_down: true,
        };
        let limits = SolverLimits::new(216, 10_000);
        let mut scratch = LightSolverScratch::default();
        let retained = solve_light_with_scratch(
            &fixture,
            &EmptyLight,
            bounds,
            1,
            profile,
            limits,
            &mut scratch,
        )
        .unwrap();
        let owned_before = retained.sub_chunks().clone();
        let before = buffers(&scratch);
        for _ in 0..8 {
            let actual = solve_light_with_scratch(
                &fixture,
                &EmptyLight,
                bounds,
                2,
                profile,
                limits,
                &mut scratch,
            )
            .unwrap();
            let expected = solve_light(&fixture, &EmptyLight, bounds, 2, profile, limits).unwrap();
            assert_same(&actual, &expected);
            assert_eq!(buffers(&scratch), before, "a warmed buffer was reallocated");
        }
        assert_eq!(retained.sub_chunks(), &owned_before);
    }

    #[test]
    fn reused_scratch_matches_fresh_solves_across_edits_bounds_and_dimensions() {
        let mut scratch = LightSolverScratch::default();
        let mut prior = None;
        let mut state = 0x9347_abd1_u32;
        for generation in 0..40 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let extent = 2 + (state % 4) as i32;
            let min = BlockPos::new((state % 7) as i32 - 3, -extent, 2);
            let bounds = LightBounds::new(
                (state % 3) as i32,
                min,
                BlockPos::new(min.x + extent, 0, 2 + extent),
            )
            .unwrap();
            let fixture = Fixture {
                bounds,
                salt: state,
            };
            let profile = match state % 3 {
                0 => DimensionLightProfile::Nether,
                1 => DimensionLightProfile::Overworld {
                    direct_sky_down: false,
                },
                _ => DimensionLightProfile::Overworld {
                    direct_sky_down: true,
                },
            };
            let limits = SolverLimits::new(216, 30_000);
            let (actual, expected) = if let Some(prior) = &prior {
                (
                    solve_light_with_scratch(
                        &fixture,
                        prior,
                        bounds,
                        generation,
                        profile,
                        limits,
                        &mut scratch,
                    )
                    .unwrap(),
                    solve_light(&fixture, prior, bounds, generation, profile, limits).unwrap(),
                )
            } else {
                (
                    solve_light_with_scratch(
                        &fixture,
                        &EmptyLight,
                        bounds,
                        generation,
                        profile,
                        limits,
                        &mut scratch,
                    )
                    .unwrap(),
                    solve_light(&fixture, &EmptyLight, bounds, generation, profile, limits)
                        .unwrap(),
                )
            };
            assert_same(&actual, &expected);
            prior = Some(actual);
        }
    }

    struct InvalidPrior;

    impl LightReadAccess for InvalidPrior {
        fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
            16
        }
    }

    #[test]
    fn failed_solves_do_not_leak_cached_values_or_queued_work() {
        let bounds = LightBounds::new(0, BlockPos::new(0, 0, 0), BlockPos::new(3, 3, 3)).unwrap();
        let fixture = Fixture { bounds, salt: 41 };
        let profile = DimensionLightProfile::Overworld {
            direct_sky_down: true,
        };
        let limits = SolverLimits::new(64, 10_000);
        let mut scratch = LightSolverScratch::default();
        assert!(matches!(
            solve_light_with_scratch(
                &fixture,
                &EmptyLight,
                bounds,
                1,
                profile,
                SolverLimits::new(64, 3),
                &mut scratch
            ),
            Err(LightSolveError::QueueLimitExceeded { .. })
        ));
        assert!(matches!(
            solve_light_with_scratch(
                &fixture,
                &InvalidPrior,
                bounds,
                1,
                profile,
                limits,
                &mut scratch
            ),
            Err(LightSolveError::LightValueOutOfRange { value: 16 })
        ));
        let actual = solve_light_with_scratch(
            &fixture,
            &EmptyLight,
            bounds,
            2,
            profile,
            limits,
            &mut scratch,
        )
        .unwrap();
        let expected = solve_light(&fixture, &EmptyLight, bounds, 2, profile, limits).unwrap();
        assert_same(&actual, &expected);
    }
}
