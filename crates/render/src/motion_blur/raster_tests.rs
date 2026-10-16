//! Pixel tests against the production camera exposure shader on a native headless adapter.

use super::{
    CameraMotionBlur,
    history::{CameraHistory, ExposureUniform},
};
use crate::gpu_snapshot::Gpu;
use bevy::math::{Mat4, UVec4, Vec3, Vec4};
use wgpu::util::DeviceExt;

const SIDE: u32 = 128;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const FILTER_SAMPLE_COUNTS: [u32; 3] = [3, 17, 32];

struct Raster {
    gpu: Gpu,
    depth_samples: Vec<u32>,
}

/// Velocity covers only this rectangle; all other samples belong to the world.
struct ActorMotion {
    velocity: [f32; 4],
    bounds: UVec4,
    depth: Option<f32>,
}

#[path = "raster_tests/fixture.rs"]
mod fixture;
#[path = "raster_tests/occlusion.rs"]
mod occlusion;
#[path = "raster_tests/resolve.rs"]
mod resolve;
#[path = "raster_tests/surfaces.rs"]
mod surfaces;
use occlusion::Coverage;

#[test]
fn actor_coverage_outside_msaa_sample_zero_does_not_receive_world_translation_blur() {
    let Some(raster) = Raster::new("partial following actor coverage raster") else {
        return;
    };
    let input = impulse();
    let uniform = exposure(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)));
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in raster.depth_samples.iter().filter(|&&samples| samples > 1) {
        let output = raster.draw_with_motion_mask(
            &input,
            uniform,
            samples,
            z,
            Some(ActorMotion {
                depth: None,
                velocity: [0.0, 0.0, 1.0, 1.0],
                bounds: UVec4::new(0, 0, SIDE, SIDE),
            }),
            1 << (samples - 1),
        );
        assert_eq!(
            output, input,
            "actor covered only by the last sample blurred at {samples}x MSAA"
        );
    }
}

#[test]
fn actor_velocity_keeps_camera_followed_pixels_sharp_without_erasing_object_motion() {
    let Some(raster) = Raster::new("actor velocity exposure raster") else {
        return;
    };
    let input = impulse();
    let uniform = exposure(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)));
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in &raster.depth_samples {
        let world = raster.draw(&input, uniform, samples, z);
        assert!(extent(&world)[0] > extent(&input)[0]);
        let followed = raster.draw_with_motion(
            &input,
            uniform,
            samples,
            z,
            Some(ActorMotion {
                depth: None,
                velocity: [0.0, 0.0, 1.0, 1.0],
                bounds: UVec4::new(0, 0, SIDE, SIDE),
            }),
        );
        assert_eq!(
            followed, input,
            "following actor changed at {samples}x MSAA"
        );
        let mut stationary_camera = exposure(Mat4::IDENTITY);
        stationary_camera.strength.x = uniform.strength.x;
        let moving = raster.draw_with_motion(
            &input,
            stationary_camera,
            samples,
            z,
            Some(ActorMotion {
                depth: None,
                velocity: [0.125, 0.0, 1.0, 1.0],
                bounds: UVec4::new(0, 0, SIDE, SIDE),
            }),
        );
        assert!(
            extent(&moving)[0] > 6,
            "actual object motion must remain visible"
        );
        assert_eq!(extent(&moving)[1], extent(&input)[1]);
        if samples == 1 {
            save("following-actor", &followed);
            save("moving-actor", &moving);
            save("world-translation", &world);
        }
    }
}

#[test]
fn stationary_camera_actor_exposure_spreads_colour_into_the_swept_background() {
    let Some(raster) = Raster::new("moving actor exterior exposure raster") else {
        return;
    };
    let bounds = UVec4::new(60, 48, 8, 32);
    let input = image(|x, y| {
        if x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w {
            [255, 0, 0, 255]
        } else {
            [0, 128, 0, 255]
        }
    });
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    let mut uniform = exposure(Mat4::IDENTITY);
    uniform.strength.x = 1.0;
    uniform.strength.y = 17.0;
    for &samples in &raster.depth_samples {
        let output = raster.draw_with_motion(
            &input,
            uniform,
            samples,
            z,
            Some(ActorMotion {
                velocity: [0.125, 0.0, 1.0, 1.0],
                bounds,
                depth: None,
            }),
        );
        let row = &output[(SIDE / 2 * SIDE * 4) as usize..((SIDE / 2 + 1) * SIDE * 4) as usize];
        for x in [bounds.x - 3, bounds.x + bounds.z + 2] {
            assert!(
                row[x as usize * 4] > 8,
                "moving actor must expose background beyond both silhouette edges at {samples}x MSAA"
            );
        }
        let energy: u32 = row
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| u32::from(pixel[0]))
            .sum();
        let input_energy = bounds.z * 255;
        assert!(
            energy.abs_diff(input_energy) <= input_energy / 20,
            "exposure must conserve actor colour within quantization tolerance: {energy}/{input_energy}/{samples}x MSAA"
        );
        for pixel in row.as_chunks::<4>().0.iter() {
            assert_eq!(pixel[2], 0, "exposure invented a blue component");
            assert_eq!(pixel[3], 255, "opaque scene exposure changed alpha");
            let complementary_green = 128.0 * (1.0 - f32::from(pixel[0]) / 255.0);
            assert!(
                (f32::from(pixel[1]) - complementary_green).abs() <= 2.0,
                "actor exposure must blend the covered background rather than add light"
            );
        }
    }
}

#[test]
fn moving_actor_silhouettes_blur_against_world_pixels() {
    let Some(raster) = Raster::new("actor silhouette exposure raster") else {
        return;
    };
    let bounds = UVec4::new(SIDE / 2 - 4, SIDE / 2 - 16, 8, 32);
    let input = image(|x, y| {
        let value =
            if x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w
            {
                255
            } else {
                0
            };
        [value, value, value, 255]
    });
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    let pixel = |pixels: &[u8], x: u32| pixels[((SIDE / 2 * SIDE + x) * 4) as usize];
    for &samples in &raster.depth_samples {
        for camera in [
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        ] {
            let mut uniform = exposure(camera);
            uniform.strength.x = 1.0;
            let moving = raster.draw_with_motion(
                &input,
                uniform,
                samples,
                z,
                Some(ActorMotion {
                    depth: None,
                    velocity: [-0.125, 0.0, 1.0, 1.0],
                    bounds,
                }),
            );
            assert!(
                pixel(&moving, bounds.x) < 240,
                "moving silhouette kept its sharp edge at {samples}x MSAA"
            );
            for x in [bounds.x - 3, bounds.x + bounds.z + 2] {
                assert!(
                    pixel(&moving, x) > 8,
                    "moving silhouette must extend beyond both edges at {samples}x MSAA/{camera:?}"
                );
            }
            let followed = raster.draw_with_motion(
                &input,
                uniform,
                samples,
                z,
                Some(ActorMotion {
                    depth: None,
                    velocity: [0.0, 0.0, 1.0, 1.0],
                    bounds,
                }),
            );
            assert_eq!(
                pixel(&followed, bounds.x),
                255,
                "screen-stationary actor edge must remain sharp"
            );
            assert_eq!(
                pixel(&followed, bounds.x - 3),
                0,
                "world blur must reject screen-stationary actors"
            );
        }
    }
}

#[test]
fn actor_streaks_respect_foreground_depth_and_diagonal_reach() {
    let Some(raster) = Raster::new("actor footprint depth raster") else {
        return;
    };
    // The rectangle crosses a tile boundary so both neighbour lookup and source reduction matter.
    let edge = super::shader::TILE_SIZE;
    let bounds = UVec4::new(edge - 4, edge - 4, 8, 8);
    let input = image(|x, y| {
        if x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w {
            [255, 0, 0, 255]
        } else {
            [0, 128, 0, 255]
        }
    });
    let pixel = |pixels: &[u8], x: u32, y: u32| pixels[((y * SIDE + x) * 4) as usize];
    for &samples in &raster.depth_samples {
        for sample_mask in [!0, 1 << (samples - 1)] {
            for (background_depth, spreads) in [
                (0.0, true),
                (0.0125, true),
                (0.025001, false),
                (0.05, false),
            ] {
                let mut uniform = exposure(Mat4::IDENTITY);
                uniform.strength.x = 1.0;
                let output = raster.draw_with_motion_mask(
                    &input,
                    uniform,
                    samples,
                    background_depth,
                    Some(ActorMotion {
                        velocity: [0.125, 0.125, 1.0, 1.0],
                        bounds,
                        depth: Some(0.025),
                    }),
                    sample_mask,
                );
                for (x, y) in [
                    (bounds.x - 3, bounds.y - 3),
                    (bounds.x + bounds.z + 2, bounds.y + bounds.w + 2),
                ] {
                    if spreads {
                        assert!(
                            pixel(&output, x, y) > 8,
                            "actor streak failed to reach background at {samples}x MSAA/mask {sample_mask}"
                        );
                    } else {
                        assert_eq!(
                            pixel(&output, x, y),
                            0,
                            "actor streak leaked onto closer foreground at {samples}x MSAA/mask {sample_mask}"
                        );
                    }
                }
                assert_eq!(
                    pixel(&output, bounds.x - 3, bounds.y + bounds.w + 2),
                    0,
                    "tile propagation must not spread perpendicular to actual actor motion"
                );
                assert_eq!(
                    pixel(&output, bounds.x - 12, bounds.y - 12),
                    0,
                    "tile propagation must not extend beyond the actor's exposure"
                );
            }
        }
    }
}

#[test]
fn bilinear_world_gathers_do_not_leak_stationary_actor_colour() {
    let Some(raster) = Raster::new("stationary actor bilinear footprint raster") else {
        return;
    };
    let bounds = UVec4::new(60, 40, 8, 48);
    let input = image(|x, y| {
        if x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w {
            [255, 0, 0, 255]
        } else {
            [0, 128, 0, 255]
        }
    });
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in &raster.depth_samples {
        for translation in [
            Vec3::new(0.27, 0.0, 0.0),
            Vec3::new(0.0, 0.27, 0.0),
            Vec3::new(0.27, 0.19, 0.0),
        ] {
            let output = raster.draw_with_motion(
                &input,
                exposure(Mat4::from_translation(translation)),
                samples,
                z,
                Some(ActorMotion {
                    depth: None,
                    velocity: [0.0, 0.0, 1.0, 1.0],
                    bounds,
                }),
            );
            for y in bounds.y - 3..bounds.y + bounds.w + 3 {
                for x in bounds.x - 3..bounds.x + bounds.z + 3 {
                    if x >= bounds.x
                        && x < bounds.x + bounds.z
                        && y >= bounds.y
                        && y < bounds.y + bounds.w
                    {
                        continue;
                    }
                    let offset = ((y * SIDE + x) * 4) as usize;
                    assert_eq!(
                        output[offset], 0,
                        "actor colour leaked into world at ({x},{y})/{samples}x MSAA/{translation:?}"
                    );
                }
            }
        }
    }
}

fn projection() -> Mat4 {
    glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    )
}

fn exposure(world_from_view: Mat4) -> ExposureUniform {
    let viewport = UVec4::new(0, 0, SIDE, SIDE);
    let mut previous = CameraHistory::new(projection(), Mat4::IDENTITY, viewport, 0);
    previous.advance(
        CameraHistory::new(projection(), world_from_view, viewport, 0),
        CameraMotionBlur {
            exposure_seconds: 1.0 / 120.0,
            delta_seconds: 1.0 / 120.0,
            samples: FILTER_SAMPLE_COUNTS[1],
            reset_epoch: 0,
        },
    )
}

fn image(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            bytes.extend(pixel(x, y));
        }
    }
    bytes
}

fn impulse() -> Vec<u8> {
    image(|x, y| {
        let value = if x.abs_diff(SIDE / 2) <= 1 && y.abs_diff(SIDE / 2) <= 1 {
            255
        } else {
            0
        };
        [value, value, value, 255]
    })
}

fn extent(bytes: &[u8]) -> [u32; 2] {
    let mut lower = [SIDE; 2];
    let mut upper = [0; 2];
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        if pixel[0] > 8 {
            let position = [index as u32 % SIDE, index as u32 / SIDE];
            for axis in 0..2 {
                lower[axis] = lower[axis].min(position[axis]);
                upper[axis] = upper[axis].max(position[axis]);
            }
        }
    }
    assert!(
        lower[0] <= upper[0],
        "blur must preserve the visible impulse"
    );
    [upper[0] - lower[0] + 1, upper[1] - lower[1] + 1]
}

fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        image::save_buffer(
            std::path::PathBuf::from(directory).join(format!("motion-blur-{name}.png")),
            pixels,
            SIDE,
            SIDE,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
fn motion_blur_static_camera_matches_disabled_raster_for_supported_msaa() {
    let Some(raster) = Raster::new("motion blur static camera raster") else {
        return;
    };
    let input = image(|x, y| [(x * 7) as u8, (y * 11) as u8, (x ^ y) as u8, 255]);
    for samples in &raster.depth_samples {
        let mut uniform = exposure(Mat4::IDENTITY);
        let disabled = raster.draw(&input, uniform, *samples, 0.5);
        assert_eq!(
            disabled, input,
            "disabled pass must preserve every source pixel"
        );
        for count in FILTER_SAMPLE_COUNTS {
            uniform.strength = Vec4::new(1.0, count as f32, 0.0, 0.0);
            let enabled = raster.draw(&input, uniform, *samples, 0.5);
            assert_eq!(
                enabled, disabled,
                "stationary reprojection changed pixels at {samples}x MSAA/{count} samples"
            );
        }
    }
}

#[test]
fn motion_blur_rotating_camera_smears_pixels_along_its_rotation_axis() {
    let Some(raster) = Raster::new("motion blur rotating camera raster") else {
        return;
    };
    let input = impulse();
    for samples in &raster.depth_samples {
        for (name, pose, axis) in [
            ("yaw", Mat4::from_rotation_y(0.2), 0),
            ("pitch", Mat4::from_rotation_x(0.2), 1),
        ] {
            for count in FILTER_SAMPLE_COUNTS {
                let mut uniform = exposure(pose);
                uniform.strength.y = count as f32;
                let blurred = raster.draw(&input, uniform, *samples, 0.05);
                assert_ne!(blurred, input, "{name} must change pixels");
                let span = extent(&blurred);
                assert!(
                    span[axis] > 6,
                    "{name} must spread the impulse along its axis: {span:?}"
                );
                assert!(
                    span[1 - axis] <= 4,
                    "{name} must not smear the perpendicular axis: {span:?}"
                );
                if *samples == 1 && count == FILTER_SAMPLE_COUNTS[1] {
                    save(name, &blurred);
                }
            }
        }
    }
    save("off", &input);
}

#[test]
fn motion_blur_translation_uses_depth_and_keeps_sky_stationary() {
    let Some(raster) = Raster::new("motion blur translation raster") else {
        return;
    };
    let input = impulse();
    let uniform = exposure(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)));
    for samples in &raster.depth_samples {
        let near = raster.draw(
            &input,
            uniform,
            *samples,
            projection().project_point3(Vec3::new(0.0, 0.0, -2.0)).z,
        );
        let far = raster.draw(
            &input,
            uniform,
            *samples,
            projection().project_point3(Vec3::new(0.0, 0.0, -8.0)).z,
        );
        let sky = raster.draw(&input, uniform, *samples, 0.0);
        assert_eq!(sky, input, "translation must not move the sky at infinity");
        let near_span = extent(&near);
        let far_span = extent(&far);
        assert!(
            near_span[0] > far_span[0],
            "nearby surfaces must smear farther: {near_span:?} vs {far_span:?}"
        );
        assert!(
            far_span[0] > extent(&input)[0],
            "distant geometry still receives camera translation"
        );
        assert_eq!(near_span[1], extent(&input)[1]);
        assert_eq!(far_span[1], extent(&input)[1]);
    }
}
