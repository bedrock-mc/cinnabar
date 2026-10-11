use super::{CameraMotionBlur, graph::*, history::*};
use bevy::prelude::*;

fn settings() -> CameraMotionBlur {
    CameraMotionBlur {
        exposure_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
        delta_seconds: 0.02,
    }
}
fn history(position: Vec3, yaw: f32, epoch: u64) -> CameraHistory {
    let pose = Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), position);
    CameraHistory::new(
        Mat4::perspective_infinite_reverse_rh(1.2, 2.0, 0.1),
        pose,
        UVec4::new(0, 0, 640, 320),
        epoch,
    )
}

#[test]
fn camera_reanchors_and_invalid_frames_have_zero_strength() {
    let mut previous = history(Vec3::ZERO, 0.0, 0);
    assert_eq!(previous.advance(previous, settings()).strength.x, 0.0);
    assert!(
        previous
            .advance(history(Vec3::X, 0.1, 0), settings())
            .strength
            .x
            > 0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::X, 0.2, 1), settings())
            .strength
            .x,
        0.0
    );
    assert!(
        previous
            .advance(history(Vec3::X, 0.3, 1), settings())
            .strength
            .x
            > 0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::X * 100.0, 0.3, 1), settings())
            .strength
            .x,
        0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::ZERO, 2.0, 1), settings())
            .strength
            .x,
        0.0
    );
    for dt in [0.0, -1.0, f32::NAN, f32::INFINITY, 0.3] {
        let mut previous = history(Vec3::ZERO, 0.0, 0);
        assert_eq!(
            previous
                .advance(
                    history(Vec3::ZERO, 0.1, 0),
                    CameraMotionBlur {
                        delta_seconds: dt,
                        ..settings()
                    }
                )
                .strength
                .x,
            0.0
        );
    }
}

#[test]
fn exposure_uses_real_elapsed_time_and_history_allocates_nothing() {
    let projected_velocity = |dt| {
        let mut previous = history(Vec3::ZERO, 0.0, 0);
        let uniform = previous.advance(
            history(Vec3::X * dt, 0.0, 0),
            CameraMotionBlur {
                delta_seconds: dt,
                ..settings()
            },
        );
        let previous_pixel = uniform
            .previous_clip_from_clip
            .project_point3(Vec3::new(0.0, 0.0, 0.05));
        previous_pixel.x * uniform.strength.x
    };
    assert!((projected_velocity(1.0 / 60.0) - projected_velocity(1.0 / 240.0)).abs() < 0.00001);
    let mut previous = history(Vec3::ZERO, 0.0, 0);
    let current = history(Vec3::X, 0.1, 0);
    let start = crate::alloc_count::thread_allocations();
    std::hint::black_box(previous.advance(current, settings()));
    assert_eq!(crate::alloc_count::thread_allocations(), start);
}

#[test]
fn motion_blur_reprojection_is_independent_of_the_world_origin() {
    let reproject = |origin: Vec3, translation: Vec3| {
        let mut previous = history(origin, 0.1, 0);
        previous
            .advance(history(origin + translation, 0.101, 0), settings())
            .previous_clip_from_clip
            .project_point3(Vec3::new(0.0, 0.0, 0.1))
    };
    for translation in [Vec3::ZERO, Vec3::new(0.125, 0.25, -0.125)] {
        let expected = reproject(Vec3::ZERO, translation);
        for offset in [1_000.0, 10_000.0, 100_000.0, 1_000_000.0, -1_000_000.0] {
            let actual = reproject(Vec3::new(offset, 64.0, offset), translation);
            assert!(
                actual.abs_diff_eq(expected, 0.000001),
                "{offset}: {actual:?} != {expected:?}"
            );
        }
    }
}

#[test]
fn disabled_or_unprepared_exposure_encodes_nothing_across_toggles() {
    let mut world = crate::render_test_support::empty_render_world();
    crate::ui_render::install_overlay_graph(&mut world);
    for enabled in [false, true, true, false, false] {
        configure_graph(&mut world, enabled);
        let before = crate::alloc_count::thread_allocations();
        configure_graph(&mut world, enabled);
        assert_eq!(before, crate::alloc_count::thread_allocations());
        crate::render_test_support::assert_empty_render(&mut world);
    }
}
