use super::*;

#[test]
fn overlapping_events_add_and_hold_until_their_duration() {
    let mut state = ShakeState::default();
    state.add(ShakeKind::Positional, 1.0, 2.0);
    state.add(ShakeKind::Positional, 2.0, 1.0);
    state.advance(0.5);
    assert_eq!(state.positional.intensity, 3.0);
    state.advance(0.5);
    assert_eq!(state.positional.intensity, 3.0);
    state.advance(0.25);
    assert_eq!(state.positional.intensity, 2.75);
    state.advance(1.0);
    assert_eq!(state.offset(), ShakeOffset::NONE);
}

#[test]
fn decaying_expired_kind_remains_while_the_other_queue_is_active() {
    let mut state = ShakeState::default();
    state.add(ShakeKind::Positional, 2.0, 1.0);
    state.add(ShakeKind::Rotational, 1.0, 5.0);
    state.advance(1.0);
    state.advance(0.5);
    assert_eq!(state.positional.intensity, 1.5);
    assert_eq!(state.rotational.intensity, 1.0);
    state.stop_all();
    assert!(!state.is_active());
    assert_eq!(state.offset(), ShakeOffset::NONE);
}

#[test]
fn invalid_events_preserve_running_shakes_and_total_intensity_is_capped() {
    let mut state = ShakeState::default();
    state.add(ShakeKind::Positional, 100.0, 5.0);
    assert!(!state.add(ShakeKind::Positional, -1.0, 1.0));
    assert!(!state.add(ShakeKind::Positional, 0.0, 1.0));
    assert!(!state.add(ShakeKind::Positional, f32::NAN, 1.0));
    assert!(!state.add(ShakeKind::Positional, 1.0, 0.0));
    state.advance(0.1);
    assert_eq!(state.positional.intensity, MAX_INTENSITY);
}

#[test]
fn sampling_and_expiry_allocate_nothing() {
    let mut state = ShakeState::default();
    state.add(ShakeKind::Positional, 1.0, 1.0);
    state.add(ShakeKind::Rotational, 2.0, 2.0);
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        state.advance(0.01);
        std::hint::black_box(state.offset());
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}

#[test]
fn positional_shake_keeps_world_axes() {
    let mut pose = Transform::from_rotation(Quat::from_rotation_y(1.0));
    let offset = ShakeOffset {
        translation: Vec3::X,
        rotation_radians: None,
    };
    offset.apply(&mut pose);
    assert_eq!(pose.translation, Vec3::X);
}

#[test]
fn native_shake_arguments_share_scaled_radian_noise() {
    let mut state = ShakeState::default();
    state.add(ShakeKind::Positional, 1.0, 2.0);
    state.add(ShakeKind::Rotational, 2.0, 2.0);
    state.advance(0.25);
    let expected =
        state.noise.as_ref().unwrap().sample(1.0, 10.0) * (20.0 * std::f32::consts::PI / 180.0);
    let offset = state.offset();
    assert!((offset.translation - expected).length() < 1e-6);
    assert!((offset.rotation_radians.unwrap() - expected.truncate() * 2.0).length() < 1e-6);
}

#[test]
fn rotational_shake_subtracts_raw_radians_and_resets_roll() {
    let mut pose = Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.3, 0.2, 0.1));
    ShakeOffset {
        translation: Vec3::ZERO,
        rotation_radians: Some(Vec2::new(2.0, -0.7)),
    }
    .apply(&mut pose);
    let expected = Quat::from_euler(EulerRot::YXZ, 1.0, -1.8, 0.0);
    assert!(pose.rotation.dot(expected).abs() > 0.99999);
}
