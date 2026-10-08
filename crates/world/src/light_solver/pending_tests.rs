use super::*;

struct Sources {
    bounds: LightBounds,
    sky: bool,
}

impl LightBlockAccess for Sources {
    /// Two separated sources share the same pending neighbour.
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if !self.bounds.contains(position) {
            return LightBlockSample::Unknown;
        }
        if !self.sky && position.y == 0 {
            LightBlockSample::Resident(LightProperties::new(3, 0).unwrap())
        } else if !self.sky && position.y == 2 {
            LightBlockSample::Resident(LightProperties::new(15, 0).unwrap())
        } else {
            LightBlockSample::KnownAir
        }
    }

    /// The upper source upgrades pending attenuated sky to direct downward sky.
    fn sky_seed(&self, position: BlockPos) -> u8 {
        if !self.sky {
            return 0;
        }
        match position.y {
            0 => 14,
            2 => 15,
            _ => 0,
        }
    }
}

/// Checks final light and provenance while a stronger source replaces queued weaker work.
fn assert_pending_source_merge(sky: bool) {
    let bounds = LightBounds::new(0, BlockPos::new(0, 0, 0), BlockPos::new(0, 2, 0)).unwrap();
    let sources = Sources { bounds, sky };
    let output = solve_light(
        &sources,
        &EmptyLight,
        bounds,
        1,
        DimensionLightProfile::Overworld {
            direct_sky_down: true,
        },
        SolverLimits::new(3, 100),
    )
    .unwrap();
    for y in 0..=2 {
        let position = BlockPos::new(0, y, 0);
        assert_eq!(
            output.light_at(position, LightChannel::Block),
            if sky { 0 } else { 13 + y as u8 }
        );
        assert_eq!(
            output.light_at(position, LightChannel::Sky),
            if sky { 15 } else { 0 }
        );
        assert_eq!(output.has_direct_sky_provenance(0, position), sky);
    }
    assert_eq!(
        output.stats().increase_dequeued,
        4,
        "a pending cell must merge its later increase and direct-sky provenance"
    );
}

#[test]
fn pending_block_increases_merge_stronger_sources_before_visiting_neighbours() {
    assert_pending_source_merge(false);
}

#[test]
fn pending_sky_increases_merge_direct_provenance_before_visiting_neighbours() {
    assert_pending_source_merge(true);
}

#[test]
fn failed_increase_queue_can_be_reused_with_different_bounds() {
    let initial = LightBounds::new(0, BlockPos::new(0, 0, 0), BlockPos::new(0, 2, 0)).unwrap();
    let moved = LightBounds::new(1, BlockPos::new(-3, 0, 5), BlockPos::new(-3, 3, 5)).unwrap();
    let profile = DimensionLightProfile::Overworld {
        direct_sky_down: true,
    };
    for sky in [false, true] {
        let mut scratch = LightSolverScratch::default();
        assert!(matches!(
            solve_light_with_scratch(
                &Sources {
                    bounds: initial,
                    sky
                },
                &EmptyLight,
                initial,
                1,
                profile,
                SolverLimits::new(3, 1),
                &mut scratch,
            ),
            Err(LightSolveError::QueueLimitExceeded { .. })
        ));
        let sources = Sources { bounds: moved, sky };
        let limits = SolverLimits::new(4, 100);
        let actual = solve_light_with_scratch(
            &sources,
            &EmptyLight,
            moved,
            2,
            profile,
            limits,
            &mut scratch,
        )
        .unwrap();
        let expected = solve_light(&sources, &EmptyLight, moved, 2, profile, limits).unwrap();
        assert_eq!(actual.sub_chunks(), expected.sub_chunks());
        assert_eq!(actual.stats(), expected.stats());
        for position in moved.positions() {
            assert_eq!(
                actual.has_direct_sky_provenance(moved.dimension, position),
                expected.has_direct_sky_provenance(moved.dimension, position),
            );
        }
    }
}
