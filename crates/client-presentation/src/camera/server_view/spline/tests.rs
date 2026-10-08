use std::sync::Arc;

use protocol::{CameraSplineProgressKeyFrame, CameraSplineRotationKeyFrame};

use super::*;

/// Constructs a valid path with independent endpoint tracks.
fn path(kind: CameraSplineKind, points: &[[f32; 3]]) -> CameraSpline {
    CameraSpline {
        name: Arc::from("test:path"),
        total_time_seconds: 4.0,
        kind,
        control_points: Arc::from(points),
        progress_key_frames: [(0.0, 0.0), (1.0, 4.0)]
            .map(|(progress, time_seconds)| CameraSplineProgressKeyFrame {
                progress,
                time_seconds,
                ease_type: Arc::from("linear"),
            })
            .into(),
        rotation_key_frames: [(0.0, 0.0), (360.0, 4.0)]
            .map(|(yaw, time_seconds)| CameraSplineRotationKeyFrame {
                rotation_degrees: [0.0, yaw, 0.0],
                time_seconds,
                ease_type: Arc::from("linear"),
            })
            .into(),
    }
}

#[test]
fn linear_knots_follow_distance_instead_of_control_point_count() {
    let spline = path(
        CameraSplineKind::Linear,
        &[[0.0; 3], [1.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
    );
    let mut playback = SplinePlayback::new(&spline).unwrap();
    playback.advance(2.0);
    assert_eq!(playback.sample().translation, Vec3::new(5.0, 0.0, 0.0));
    playback.advance(2.0);
    assert_eq!(playback.sample().translation, Vec3::new(10.0, 0.0, 0.0));
    playback.advance(10.0);
    assert_eq!(playback.sample().translation, Vec3::new(10.0, 0.0, 0.0));
}

#[test]
fn catmull_rom_has_half_edge_endpoint_tangents() {
    let spline = path(
        CameraSplineKind::CatmullRom,
        &[[0.0; 3], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [4.0, 2.0, 0.0]],
    );
    let playback = SplinePlayback::new(&spline).unwrap();
    assert!((playback.position(1.0 / 6.0) - Vec3::new(1.0, -0.125, 0.0)).length() < 1e-5);
    assert!((playback.position(0.5) - Vec3::new(2.0, 1.0, 0.0)).length() < 1e-5);
}

#[test]
fn rotation_preserves_full_turns_and_uses_its_own_clock() {
    let spline = path(
        CameraSplineKind::Linear,
        &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
    );
    let mut playback = SplinePlayback::new(&spline).unwrap();
    playback.advance(1.0);
    let forward = playback.sample().rotation * Vec3::NEG_Z;
    assert!((forward - Vec3::NEG_X).length() < 1e-5);
}

#[test]
fn progress_uses_starting_keyframes_ease_and_holds_last_value() {
    let frames = [
        Keyframe {
            value: Vec3::ZERO,
            time: 0.0,
            ease: 2,
        },
        Keyframe {
            value: Vec3::X,
            time: 2.0,
            ease: 0,
        },
    ];
    assert_eq!(sample_track(&frames, 1.0, 4.0), Vec3::X * 0.25);
    assert_eq!(sample_track(&frames, 3.0, 4.0), Vec3::X);
}

#[test]
fn degenerate_or_missing_tracks_are_rejected() {
    let mut spline = path(CameraSplineKind::Linear, &[[0.0; 3]; 3]);
    assert!(SplinePlayback::new(&spline).is_none());
    spline.control_points = [[0.0; 3], [1.0, 0.0, 0.0]].into();
    assert!(SplinePlayback::new(&spline).is_none());
    spline.control_points = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]].into();
    spline.progress_key_frames = [].into();
    assert!(SplinePlayback::new(&spline).is_none());
}

#[test]
fn repeated_sampling_keeps_prepared_geometry_and_tracks() {
    let spline = path(
        CameraSplineKind::CatmullRom,
        &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [3.0, 0.0, 0.0]],
    );
    let mut playback = SplinePlayback::new(&spline).unwrap();
    let allocations = (
        playback.points.as_ptr(),
        playback.knots.as_ptr(),
        playback.progress.as_ptr(),
        playback.rotation.as_ptr(),
    );
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        playback.advance(0.001);
        assert!(playback.sample().translation.is_finite());
        assert_eq!(
            allocations,
            (
                playback.points.as_ptr(),
                playback.knots.as_ptr(),
                playback.progress.as_ptr(),
                playback.rotation.as_ptr()
            )
        );
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}

#[test]
fn time_past_duration_finishes_without_evaluating_an_endpoint() {
    let spline = path(
        CameraSplineKind::Linear,
        &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
    );
    let mut playback = SplinePlayback::new(&spline).unwrap();
    playback.advance(3.0);
    let previous = playback.sample();
    playback.advance(2.0);
    assert!(playback.is_finished());
    assert_eq!(playback.sample(), previous);
}

#[test]
fn rotation_uses_direct_euler_yxz_angles_instead_of_set_instruction_axes() {
    let mut spline = path(
        CameraSplineKind::Linear,
        &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
    );
    spline.rotation_key_frames = [CameraSplineRotationKeyFrame {
        rotation_degrees: [0.0; 3],
        time_seconds: 0.0,
        ease_type: Arc::from("linear"),
    }]
    .into();
    let mut playback = SplinePlayback::new(&spline).unwrap();
    assert!((playback.sample().rotation * Vec3::NEG_Z - Vec3::NEG_Z).length() < 1e-5);
    playback.rotation[0].value = Vec3::new(90.0, 0.0, 0.0);
    assert!((playback.sample().rotation * Vec3::NEG_Z - Vec3::Y).length() < 1e-5);
    playback.rotation[0].value = Vec3::new(30.0, 45.0, 60.0);
    let expected = Quat::from_rotation_y(45.0_f32.to_radians())
        * Quat::from_rotation_x(30.0_f32.to_radians())
        * Quat::from_rotation_z(60.0_f32.to_radians());
    assert!(playback.sample().rotation.abs_diff_eq(expected, 1e-5));
}
