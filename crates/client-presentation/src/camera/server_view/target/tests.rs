use super::*;

/// Places a target at a stable test position.
fn actor(position: Vec3) -> ActorView {
    ActorView {
        position,
        yaw_degrees: 0.0,
        pitch_degrees: 0.0,
    }
}

/// Compares forward vectors without depending on equivalent quaternion signs.
fn assert_direction(rotation: Quat, expected: Vec3) {
    assert!((rotation * Vec3::NEG_Z).abs_diff_eq(expected.normalize(), 1e-5));
}

#[test]
fn speed_is_angular_and_snap_only_applies_to_acquisition() {
    let settings = TargetSettings {
        rotation_speed: 90.0,
        ..Default::default()
    };
    let mut focus = TargetFocus::new(1, Vec3::ZERO, Quat::IDENTITY, settings);
    focus.advance(0.5, Vec3::ZERO, Some(actor(Vec3::X)));
    assert_direction(
        focus.sample(Vec3::ZERO, Some(actor(Vec3::X))),
        Vec3::new(1.0, 0.0, -1.0),
    );
    focus.advance(0.5, Vec3::ZERO, Some(actor(Vec3::X)));
    assert_direction(focus.last_rotation(), Vec3::X);

    let mut focus = TargetFocus::new(
        1,
        Vec3::ZERO,
        Quat::IDENTITY,
        TargetSettings {
            snap_to_target: true,
            ..settings
        },
    );
    focus.advance(0.0, Vec3::ZERO, Some(actor(Vec3::X)));
    assert_direction(focus.last_rotation(), Vec3::X);
    focus.advance(0.5, Vec3::ZERO, Some(actor(Vec3::NEG_Z)));
    assert_direction(focus.last_rotation(), Vec3::new(1.0, 0.0, -1.0));
}

#[test]
fn distance_and_asymmetric_limits_control_return_or_continuation() {
    let settings = TargetSettings {
        horizontal_limit: [0.0, 90.0],
        vertical_limit: [60.0, 120.0],
        continue_targeting: true,
        ..Default::default()
    };
    let mut focus = TargetFocus::new(1, Vec3::ZERO, Quat::IDENTITY, settings);
    focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::X)));
    assert_direction(focus.last_rotation(), Vec3::X);
    focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::NEG_X)));
    assert_direction(focus.last_rotation(), Vec3::NEG_Z);
    focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::new(0.0, 10.0, -1.0))));
    assert_direction(
        focus.last_rotation(),
        Vec3::new(0.0, 0.5, -(3.0_f32).sqrt() / 2.0),
    );

    let mut focus = TargetFocus::new(1, Vec3::ZERO, Quat::IDENTITY, TargetSettings::default());
    focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::X * 50.0)));
    assert_direction(focus.last_rotation(), Vec3::X);
    focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::X * 50.01)));
    assert_direction(focus.last_rotation(), Vec3::NEG_Z);
}

#[test]
fn missing_targets_wait_until_acquired_and_release_after_removal_or_far_distance() {
    let mut focus = TargetFocus::new(1, Vec3::ZERO, Quat::IDENTITY, TargetSettings::default());
    assert!(focus.advance(1.0, Vec3::ZERO, None));
    assert!(focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::X))));
    assert!(!focus.advance(1.0, Vec3::ZERO, None));
    assert_direction(focus.last_rotation(), Vec3::X);
    assert!(!focus.advance(1.0, Vec3::ZERO, Some(actor(Vec3::X * 1025.0))));
}

#[test]
fn target_updates_allocate_nothing_after_initialization() {
    let mut focus = TargetFocus::new(1, Vec3::ZERO, Quat::IDENTITY, TargetSettings::default());
    focus.advance(0.01, Vec3::ZERO, Some(actor(Vec3::X)));
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        focus.advance(0.01, Vec3::ZERO, Some(actor(Vec3::X)));
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}
