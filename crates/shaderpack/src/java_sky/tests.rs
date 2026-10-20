use super::*;

/// Loads the compiled Java mapping fixture.
fn mapping() -> &'static JavaBlockMapping {
    JavaBlockMapping::pinned().expect("the Java mapping carrier is compiled in")
}

/// The colours of the Java biome `name` in `dimension`.
fn java(name: &str, dimension: JavaDimension) -> BiomeColors {
    let entry = mapping()
        .java_biomes()
        .find(|biome| biome.name == name)
        .unwrap_or_else(|| panic!("no Java biome {name}"));
    BiomeColors::of(&entry, mapping(), dimension)
}

/// Returns the Overworld plains colors from the fixture.
fn plains() -> BiomeColors {
    java("minecraft:plains", JavaDimension::Overworld)
}

/// Builds a clear normal-world camera state for tests.
fn inputs(celestial_angle: f32, look: [f32; 3]) -> SkyFogInputs {
    SkyFogInputs {
        biome: plains(),
        dimension: JavaDimension::Overworld,
        celestial_angle,
        rain: 0.0,
        thunder: 0.0,
        render_distance_chunks: 16.0,
        look,
        camera_y: 74.0,
        void_darkness_range: 32.0,
        medium: FogMedium::Air,
        night_vision: 0.0,
    }
}

/// Compares colors within the sky/fog test tolerance.
fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.5e-3)
}

#[test]
fn biome_colours_come_from_the_biome_over_its_dimension_type() {
    let colours = plains();
    assert_eq!(colours.sky, rgb(0x78_A7_FF), "plains' sky is #78A7FF");
    assert_eq!(colours.fog, rgb(0xC0_D8_FF), "the overworld's fog");
    assert_eq!(colours.water_fog, rgb(0x05_05_33), "the attribute default");
    let desert = java("minecraft:desert", JavaDimension::Overworld);
    assert_eq!(desert.sky, rgb(0x6E_B1_FF), "desert's sky is #6EB1FF");
    let pale = java("minecraft:pale_garden", JavaDimension::Overworld);
    assert_eq!(pale.sky, rgb(0xB9_B9_B9));
    assert_eq!(pale.fog, rgb(0x81_77_70));
    assert_eq!(pale.water_fog, rgb(0x55_69_80));
    let end = java("minecraft:the_end", JavaDimension::End);
    assert_eq!(end.fog, rgb(0x18_13_18), "the end's dimension fog");
}

#[test]
fn noon_sky_is_the_biome_sky_and_fog_leans_towards_it_with_render_distance() {
    let noon = sky_fog(&inputs(0.0, [0.0, 0.0, -1.0]));
    assert_eq!(noon.sky, rgb(0x78_A7_FF));
    // 1 - (0.25 + 0.75 * 16 / 32)^0.25 of the way from #C0D8FF to the sky.
    let t = 1.0 - 0.625f32.powf(0.25);
    let expected = mix(rgb(0xC0_D8_FF), rgb(0x78_A7_FF), t);
    assert!(close(noon.fog, expected), "{:?} vs {expected:?}", noon.fog);
    let far = sky_fog(&SkyFogInputs {
        render_distance_chunks: 32.0,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    assert!(close(far.fog, rgb(0xC0_D8_FF)), "{:?}", far.fog);
}

#[test]
fn midnight_darkens_the_sky_to_black_and_the_fog_to_its_floor() {
    let night = sky_fog(&inputs(0.5, [0.0, 0.0, -1.0]));
    assert_eq!(night.sky, [0.0; 3]);
    let fog = rgb(0xC0_D8_FF);
    let floor = [fog[0] * 0.06, fog[1] * 0.06, fog[2] * 0.09];
    let t = 1.0 - 0.625f32.powf(0.25);
    assert!(
        close(night.fog, floor.map(|c| c * (1.0 - t))),
        "{:?}",
        night.fog
    );
}

#[test]
fn sunset_glow_tints_the_fog_only_towards_the_sun() {
    // Sunset: the sun sets in the west (-X); 0.25 puts it on the horizon.
    let glow = sunrise_color(0.25).expect("the sun is on the horizon");
    assert!(close([glow[0], glow[1], glow[2]], [0.85, 0.375, 0.2]));
    assert!((glow[3] - 1.0).abs() < 1e-6);
    assert_eq!(sunrise_color(0.0), None, "no glow at noon");
    assert_eq!(sunrise_color(0.5), None, "no glow at midnight");
    let west = sky_fog(&inputs(0.25, [-1.0, 0.0, 0.0]));
    let east = sky_fog(&inputs(0.25, [1.0, 0.0, 0.0]));
    let north = sky_fog(&inputs(0.25, [0.0, 0.0, -1.0]));
    assert_eq!(east.fog, north.fog, "facing away keeps the plain fog");
    let t = 1.0 - 0.625f32.powf(0.25);
    let expected = mix([0.85, 0.375, 0.2], west.sky, t);
    assert!(close(west.fog, expected), "{:?} vs {expected:?}", west.fog);
    // Sunrise is in the east.
    let sunrise = sky_fog(&inputs(0.75, [1.0, 0.0, 0.0]));
    assert!(close(sunrise.fog, west.fog), "sunrise mirrors sunset");
}

#[test]
fn rain_and_thunder_grey_the_sky_and_darken_the_fog() {
    let clear = sky_fog(&inputs(0.0, [0.0, 0.0, -1.0]));
    let rain = sky_fog(&SkyFogInputs {
        rain: 1.0,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    let [r, g, b] = rgb(0x78_A7_FF);
    let grey = (r * 0.3 + g * 0.59 + b * 0.11) * 0.6;
    assert_eq!(rain.sky, mix([r, g, b], [grey; 3], 0.75));
    assert!(rain.fog[2] < clear.fog[2] && rain.fog[0] < clear.fog[0]);
    let storm = sky_fog(&SkyFogInputs {
        rain: 1.0,
        thunder: 1.0,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    assert!(storm.fog.iter().zip(rain.fog).all(|(s, r)| *s < r));
}

#[test]
fn fluids_and_other_dimensions_replace_the_overworld_fog() {
    let water = sky_fog(&SkyFogInputs {
        medium: FogMedium::Water,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    assert_eq!(water.fog, rgb(0x05_05_33));
    let lava = sky_fog(&SkyFogInputs {
        medium: FogMedium::Lava,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    assert_eq!(lava.fog, LAVA_FOG);
    let nether = sky_fog(&SkyFogInputs {
        dimension: JavaDimension::Nether,
        biome: java("minecraft:nether_wastes", JavaDimension::Nether),
        ..inputs(0.25, [-1.0, 0.0, 0.0])
    });
    assert_eq!(nether.sky, [0.0; 3]);
    let t = 1.0 - 0.625f32.powf(0.25);
    assert!(close(nether.fog, rgb(0x33_08_08).map(|c| c * (1.0 - t))));
    let below = sky_fog(&SkyFogInputs {
        camera_y: -64.5,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    });
    assert_eq!(below.fog, [0.0; 3], "below the floor the fog is black");
}

#[test]
fn biome_colours_blend_by_weight() {
    let a = plains();
    let b = BiomeColors { sky: [0.0; 3], ..a };
    let half = BiomeColors::blend([(a, 1.0), (b, 1.0)]).unwrap();
    assert!(close(half.sky, a.sky.map(|c| c * 0.5)));
    assert_eq!(BiomeColors::blend([(a, 0.0)]), None);
}

#[test]
fn bedrock_biomes_read_their_java_colours() {
    let mapping = mapping();
    let plains_id = (0..u32::from(u16::MAX))
        .find(|id| mapping.java_biome(*id) == Some("minecraft:plains"))
        .expect("a Bedrock biome maps to plains");
    let overworld = JavaDimension::Overworld;
    assert_eq!(
        BiomeColors::at(mapping, plains_id, overworld),
        Some(plains())
    );
    let void = BiomeColors::fallback(mapping, overworld).expect("the registry has the void");
    assert_eq!(
        BiomeColors::at(mapping, u32::from(u16::MAX) + 7, overworld),
        Some(void)
    );
}

#[test]
fn normal_world_void_fading_uses_a_thirty_two_block_onset() {
    for medium in [FogMedium::Air, FogMedium::Water] {
        let mut view = SkyFogInputs {
            medium,
            ..inputs(0.0, [0.0, 0.0, -1.0])
        };
        let full = sky_fog(&view).fog;
        for (camera_y, multiplier) in [
            (-80.0, 0.0),
            (-64.0, 0.0),
            (-48.0, 0.25),
            (-32.0, 1.0),
            (0.0, 1.0),
        ] {
            view.camera_y = camera_y;
            let expected = full.map(|channel| channel * multiplier);
            assert!(
                close(sky_fog(&view).fog, expected),
                "{medium:?} at {camera_y}"
            );
        }
    }
}

#[test]
fn night_vision_preserves_fog_with_any_zero_channel() {
    for colour in [[0.0, 0.4, 0.8], [0.4, 0.0, 0.8], [0.4, 0.8, 0.0]] {
        let mut view = inputs(0.0, [0.0, 0.0, -1.0]);
        view.biome.fog = colour;
        view.render_distance_chunks = 32.0;
        view.night_vision = 1.0;
        assert_eq!(sky_fog(&view).fog, colour);
    }
    let view = SkyFogInputs {
        medium: FogMedium::Lava,
        night_vision: 1.0,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    };
    assert_eq!(sky_fog(&view).fog, LAVA_FOG);
}

#[test]
fn storm_transition_uses_separate_rain_and_thunder_weights() {
    let view = SkyFogInputs {
        rain: 1.0,
        thunder: 0.5,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    };
    let expected = [0.2930425, 0.3540964, 0.46841013];
    let actual = sky_fog(&view).sky;
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-6),
        "{actual:?}"
    );
}

#[test]
fn sky_preserves_fractional_biome_colour_precision() {
    let mut view = inputs(0.0, [0.0, 0.0, -1.0]);
    view.biome.sky = [0.12345, 0.23456, 0.34567];
    assert_eq!(sky_fog(&view).sky, view.biome.sky);
}

#[test]
fn fog_weather_layers_precede_the_sky_mix() {
    let view = SkyFogInputs {
        rain: 1.0,
        thunder: 0.5,
        render_distance_chunks: 32.0,
        ..inputs(0.0, [0.0, 0.0, -1.0])
    };
    let expected = [0.35165045, 0.39560676, 0.5192157];
    let actual = sky_fog(&view).fog;
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-6),
        "{actual:?}"
    );
}

#[test]
fn flat_world_void_fading_keeps_its_one_block_range() {
    let mut view = inputs(0.0, [0.0, 0.0, -1.0]);
    let full = sky_fog(&view).fog;
    view.void_darkness_range = 1.0;
    view.camera_y = -63.5;
    assert!(close(
        sky_fog(&view).fog,
        full.map(|channel| channel * 0.25)
    ));
    view.camera_y = -63.0;
    assert!(close(sky_fog(&view).fog, full));
}
