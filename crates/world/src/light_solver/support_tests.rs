use std::cell::Cell;

use super::*;
use crate::{LightBlockSample, LightProperties};

struct Source(u8);

impl LightBlockAccess for Source {
    fn sample(&self, _: BlockPos) -> LightBlockSample {
        LightBlockSample::Resident(LightProperties::new(self.0, 0).unwrap())
    }
}

struct Neighbours {
    reads: Cell<usize>,
    level: u8,
}

impl LightReadAccess for Neighbours {
    fn read_light(&self, _: i32, _: BlockPos, _: LightChannel) -> u8 {
        self.reads.set(self.reads.get() + 1);
        self.level
    }
}

fn supported(source: u8, neighbour: u8, required: u8) -> (bool, usize) {
    let position = BlockPos::new(0, 0, 0);
    let bounds = LightBounds::new(0, BlockPos::new(-1, -1, -1), BlockPos::new(1, 1, 1)).unwrap();
    let prior = Neighbours {
        reads: Cell::new(0),
        level: neighbour,
    };
    let result = prior_supports_level(
        &Source(source),
        &prior,
        bounds,
        position,
        LightChannel::Block,
        DimensionLightProfile::Nether,
        required,
    )
    .unwrap();
    (result, prior.reads.get())
}

#[test]
fn local_light_support_needs_no_neighbour_reads() {
    assert_eq!(supported(9, 15, 9), (true, 0));
}

#[test]
fn sufficient_light_neighbour_stops_support_search() {
    assert_eq!(supported(0, 10, 9), (true, 1));
    assert_eq!(supported(0, 9, 9), (false, 6));
}

struct DirectSky {
    reads: Cell<usize>,
}

impl LightReadAccess for DirectSky {
    fn read_light(&self, _: i32, _: BlockPos, _: LightChannel) -> u8 {
        self.reads.set(self.reads.get() + 1);
        15
    }

    fn has_direct_sky_provenance(&self, _: i32, _: BlockPos) -> bool {
        true
    }
}

#[test]
fn direct_sky_support_checks_above_before_lateral_neighbours() {
    let bounds = LightBounds::new(0, BlockPos::new(-1, -1, -1), BlockPos::new(1, 1, 1)).unwrap();
    let prior = DirectSky {
        reads: Cell::new(0),
    };
    assert!(
        prior_supports_level(
            &Source(0),
            &prior,
            bounds,
            BlockPos::new(0, 0, 0),
            LightChannel::Sky,
            DimensionLightProfile::Overworld {
                direct_sky_down: true
            },
            15,
        )
        .unwrap()
    );
    assert_eq!(prior.reads.get(), 1);
}
