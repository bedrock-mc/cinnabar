#[path = "../../src/lightmap.rs"]
mod lightmap_src;
use lightmap_src::{LightmapInputs, darkness_pulse};

/// Compares independently calculated RGB fixtures without display encoding.
fn close(actual: [f32; 3], expected: [f32; 3]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 0.000_002, "{actual:?} != {expected:?}");
    }
}

#[test]
fn rm01_block_sky_addition_and_sunrise() {
    let base = LightmapInputs::default();
    close(base.colour(0.25, 0.0), [0.4, 0.3136, 0.1984]);
    close(base.colour(0.25, 0.25), [0.65, 0.5636, 0.4484]);
    let night = LightmapInputs {
        sky_darken: 0.0,
        ..base
    };
    close(night.colour(0.0, 1.0), [0.0175, 0.0175, 0.05]);
    close(
        LightmapInputs {
            lightning: true,
            ..night
        }
        .colour(0.0, 1.0),
        [0.35, 0.35, 1.0],
    );
    close(
        LightmapInputs {
            sunrise: [1.0, 0.5, 0.0, 1.0],
            ..night
        }
        .colour(0.0, 1.0),
        [0.11725, 0.06475, 0.035],
    );
}

#[test]
fn rm02_gamma_endpoints_and_both_ambient_stages() {
    let base = LightmapInputs::default();
    close(base.colour(0.0, 0.0), [0.0; 3]);
    close(
        LightmapInputs {
            ambient_adjustment: true,
            ..base
        }
        .colour(0.0, 0.0),
        [0.0588; 3],
    );
    for gamma in [0.0, 0.5, 1.0] {
        let expected = 0.25 + (0.68359375 - 0.25) * gamma;
        close(
            LightmapInputs {
                brightness: gamma,
                ..base
            }
            .colour(0.0, 0.25),
            [expected; 3],
        );
    }
}

#[test]
fn rm03_night_vision_normalizes_block_light_and_darkness_subtracts_all_channels() {
    let base = LightmapInputs::default();
    close(
        LightmapInputs {
            night_vision: 1.0,
            ..base
        }
        .colour(0.25, 0.0),
        [1.0, 0.784, 0.496],
    );
    close(
        LightmapInputs {
            brightness: 0.5,
            darkness: 0.5,
            darkness_pulse: 0.1,
            ..base
        }
        .colour(0.25, 0.0),
        [0.3, 0.2136, 0.0984],
    );
    assert_eq!(darkness_pulse(0.0, 0.0, 1.0, 1.0, 0.45), 0.45);
    assert_eq!(darkness_pulse(40.0, 0.0, 1.0, 1.0, 0.45), 0.0);
    assert!((darkness_pulse(0.5, 0.5, 0.0, 1.0, 0.45) - 0.225).abs() < 1e-6);
}

#[test]
fn all_256_pairs_remain_finite_and_dimension_builder_has_ambient_color() {
    for sky_darken in [0.0, 0.5, 1.0] {
        let inputs = LightmapInputs {
            sky_darken,
            ..Default::default()
        };
        for (index, color) in inputs.build().into_iter().enumerate() {
            assert!(
                color
                    .into_iter()
                    .all(|c| c.is_finite() && (0.0..=1.0).contains(&c))
            );
            close(
                [color[0], color[1], color[2]],
                inputs.colour(inputs.ramp[index & 15], inputs.ramp[index >> 4]),
            );
        }
    }
    close(
        LightmapInputs {
            dimension_tint: Some([1.0; 3]),
            ..Default::default()
        }
        .colour(0.0, 0.0),
        [0.22, 0.28, 0.25],
    );
}
