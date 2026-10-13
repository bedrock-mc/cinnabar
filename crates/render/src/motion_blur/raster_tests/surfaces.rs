//! Coverage witnesses and continuous textured surfaces exercise the depth guards.

use super::*;

#[test]
fn mixed_source_fixture_retains_separate_actor_and_nearer_depth_samples() {
    let Some(raster) = Raster::new("mixed source depth attachment witness") else {
        return;
    };
    let bounds = UVec4::new(60, 48, 8, 32);
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in raster.depth_samples.iter().filter(|&&samples| samples > 1) {
        let mask = 1 << (samples - 1);
        let output = raster.probe_coverage(
            &image(|_, _| [0, 128, 0, 255]),
            exposure(Mat4::IDENTITY),
            samples,
            0.0,
            Some(ActorMotion {
                velocity: [0.125, 0.0, 1.0, 1.0],
                bounds,
                depth: Some(z),
            }),
            Coverage {
                sample_mask: mask,
                occluder: Some(bounds),
                occluder_mask: !mask,
                depth_slope: 0.0,
            },
        );
        let offset = ((SIDE / 2 * SIDE + bounds.x + 2) * 4) as usize;
        let actual = &output[offset..offset + 3];
        let expected = [
            128,
            (z * 10.0 * 255.0).round() as u8,
            (255.0 / samples as f32).round() as u8,
        ];
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(&actual, expected)| actual.abs_diff(expected) <= 1),
            "fixture must retain nearest depth0.5, actor depth{z}, and one actor sample/{samples}; actual RGB={actual:?}, expected={expected:?}"
        );
    }
}

#[test]
fn unobstructed_textured_actor_exposure_is_independent_of_surface_depth_slope() {
    let Some(raster) = Raster::new("continuous sloped actor exposure raster") else {
        return;
    };
    let bounds = UVec4::new(16, 16, 96, 96);
    let input = image(|x, y| {
        if x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w {
            if x % 16 < 8 {
                [255, 0, 0, 255]
            } else {
                [0, 128, 0, 255]
            }
        } else {
            [0, 0, 0, 255]
        }
    });
    let mut uniform = exposure(Mat4::IDENTITY);
    uniform.strength.x = 1.0;
    uniform.strength.y = 17.0;
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in &raster.depth_samples {
        let draw = |slope| {
            raster.draw_with_coverage(
                &input,
                uniform,
                samples,
                0.0,
                Some(ActorMotion {
                    velocity: [0.125, 0.0, 1.0, 1.0],
                    bounds,
                    depth: Some(z),
                }),
                Coverage {
                    sample_mask: !0,
                    occluder: None,
                    occluder_mask: !0,
                    depth_slope: slope,
                },
            )
        };
        let flat = draw(0.0);
        assert_ne!(flat, input, "moving textured actor must receive exposure");
        for slope in [-0.0001, 0.0001] {
            let sloped = draw(slope);
            for y in 32..96 {
                for x in 32..96 {
                    let offset = ((y * SIDE + x) * 4) as usize;
                    assert_eq!(
                        &sloped[offset..offset + 4],
                        &flat[offset..offset + 4],
                        "same unobstructed actor texture changed exposure with slope{slope} at ({x},{y})/{samples}x MSAA"
                    );
                }
            }
        }
    }
}
