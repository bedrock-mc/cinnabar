//! The ocean floor must use native current-version water light, not a brighter shader override.
use world::{
    BlockPos, DimensionLightProfile, EmptyLight, LightBlockAccess, LightBlockSample, LightBounds,
    LightChannel, LightProperties, LightReadAccess, SolverLimits, solve_light,
};

struct WaterColumn {
    water: LightProperties,
}

impl LightBlockAccess for WaterColumn {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        match position.y {
            0 => LightBlockSample::Resident(LightProperties::new(0, 15).unwrap()),
            1..=8 => LightBlockSample::Resident(self.water),
            _ => LightBlockSample::KnownAir,
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position.y == 9 { 15 } else { 0 }
    }
}

/// The current BaseGameVersion makes water filter one.
/// flowing_water retains filter two. Ordinary Fancy seeds from the
/// water-including heightmap and then subtracts that filter at every depth.
#[test]
fn shipped_water_retains_native_skylight_at_the_ocean_floor() {
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
    let mut counts = [0; 2];
    for record in &records {
        let (index, filter) = match record.name.as_ref() {
            "minecraft:water" => (0, 1),
            "minecraft:flowing_water" => (1, 2),
            _ => continue,
        };
        let properties = lights[record.sequential_id as usize];
        assert_eq!(
            properties.filter(),
            filter,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let access = WaterColumn {
            water: LightProperties::new(properties.emission(), properties.filter()).unwrap(),
        };
        let output = solve_light(
            &access,
            &EmptyLight,
            LightBounds::new(0, [0, 0, 0].into(), [0, 9, 0].into()).unwrap(),
            1,
            DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
            SolverLimits::new(32, 128),
        )
        .unwrap();
        for depth in 1..=8_u8 {
            let position = BlockPos::new(0, 9 - i32::from(depth), 0);
            assert_eq!(
                output.read_light(0, position, LightChannel::Sky),
                15_u8.saturating_sub(depth * filter),
                "{} {} at depth {depth}",
                record.name,
                record.canonical_state,
            );
        }
        counts[index] += 1;
    }
    assert_eq!(
        counts,
        [16, 16],
        "all depths and falling states of each native type"
    );
}
