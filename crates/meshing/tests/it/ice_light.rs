//! Use shipped ice properties, not an invented transparent-block fixture.
use world::{
    BlockPos, DimensionLightProfile, EmptyLight, LightBlockAccess, LightBlockSample, LightBounds,
    LightChannel, LightProperties, LightReadAccess, SolverLimits, solve_light,
};

struct Shoreline {
    ice: LightProperties,
}

impl LightBlockAccess for Shoreline {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        match (position.x, position.y) {
            (0 | 1, 1) => LightBlockSample::Resident(self.ice),
            (1, 2) => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
            _ => LightBlockSample::KnownAir,
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position == BlockPos::new(0, 2, 0) {
            15
        } else {
            0
        }
    }
}

/// Vanilla’s final registrations and light getter:
/// transparent ice filters three sky levels, including sheltered sideways
/// light; its background remains lit. Packed ice deliberately blocks both.
#[test]
fn shipped_ice_lights_its_interior_and_background_under_a_sand_ledge() {
    let breg = include_bytes!("../../../assets/data/block-registry-v2193.bin");
    let protocol = assets::registry_header_protocol(breg).unwrap();
    let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        include_bytes!("../../../assets/data/block-light-registry-v2193.bin"),
        breg,
        records.len(),
        protocol,
    )
    .unwrap();
    let mut count = 0;
    for record in records.iter().filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:ice"
                | "minecraft:frosted_ice"
                | "minecraft:packed_ice"
                | "minecraft:blue_ice"
        )
    }) {
        let properties = lights[record.sequential_id as usize];
        let access = Shoreline {
            ice: LightProperties::new(properties.emission(), properties.filter()).unwrap(),
        };
        let output = solve_light(
            &access,
            &EmptyLight,
            LightBounds::new(0, [0, 0, 0].into(), [1, 2, 0].into()).unwrap(),
            1,
            DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
            SolverLimits::new(32, 128),
        )
        .unwrap();
        let transparent = matches!(
            record.name.as_ref(),
            "minecraft:ice" | "minecraft:frosted_ice"
        );
        for (position, expected) in [
            ([0, 1, 0], if transparent { 12 } else { 0 }),
            ([1, 1, 0], if transparent { 9 } else { 0 }),
            ([0, 0, 0], if transparent { 11 } else { 0 }),
            ([1, 0, 0], if transparent { 10 } else { 0 }),
        ] {
            assert_eq!(
                output.read_light(0, position.into(), LightChannel::Sky),
                expected,
                "{} {} at {position:?}",
                record.name,
                record.canonical_state
            );
        }
        count += 1;
    }
    assert_eq!(
        count, 7,
        "ordinary ice, all four ages, packed ice and blue ice"
    );
}
