use super::*;

fn region(hit_id: u16) -> TouchControlRegion {
    TouchControlRegion {
        hit_id,
        bounds: TouchBounds {
            min: [0.1, 0.6],
            max: [0.3, 1.0],
        },
    }
}

fn look() -> TouchControlRegion {
    TouchControlRegion {
        hit_id: touch::LOOK_SURFACE,
        bounds: TouchBounds {
            min: [0.0; 2],
            max: [1.0; 2],
        },
    }
}

fn approximately(actual: [f32; 2], expected: [f32; 2]) {
    for axis in 0..2 {
        assert!((actual[axis] - expected[axis]).abs() < 0.00001);
    }
}

#[test]
fn movement_look_and_action_fingers_keep_independent_owners() {
    let mut state = TouchControlState::default();
    assert!(state.begin(1, [0.2, 0.8], 1, Some(region(touch::JOYSTICK))));
    assert!(state.begin(2, [0.7, 0.3], 2, Some(look())));
    assert!(state.begin(3, [0.2, 0.8], 3, Some(region(touch::JUMP))));
    state.move_to(1, [0.3, 0.6], 4);
    state.move_to(2, [0.75, 0.4], 5);
    let samples = state.sample();
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0].hit_id, Some(touch::JOYSTICK));
    approximately(samples[0].delta, [std::f32::consts::FRAC_1_SQRT_2; 2]);
    assert!(samples[0].delta[1] > 0.0);
    assert_eq!(samples[1].hit_id, Some(touch::LOOK_SURFACE));
    approximately(samples[1].delta, [0.05, 0.1]);
    assert_eq!(samples[2].hit_id, Some(touch::JUMP));
    state.release(3);
    let samples = state.sample();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].hit_id, Some(touch::JOYSTICK));
    assert_eq!(samples[1].hit_id, Some(touch::LOOK_SURFACE));
}

#[test]
fn look_begins_without_a_jump_and_accumulates_until_one_sample() {
    let mut state = TouchControlState::default();
    state.begin(11, [0.6, 0.3], 1, Some(look()));
    assert_eq!(state.sample()[0].delta, [0.0; 2]);
    state.move_to(11, [0.61, 0.35], 2);
    state.move_to(11, [0.64, 0.33], 3);
    let sample = state.sample();
    approximately(sample[0].delta, [0.04, 0.03]);
    assert_eq!(sample[0].activity_sequence, 3);
    assert_eq!(state.sample()[0].delta, [0.0; 2]);
}

#[test]
fn joystick_deflection_stays_held_and_clamps_radially_outside_activation_rectangle() {
    let mut state = TouchControlState::default();
    state.begin(11, [0.2, 0.8], 1, Some(region(touch::JOYSTICK)));
    state.move_to(11, [0.4, 0.4], 2);
    let sample = state.sample()[0].clone();
    assert_eq!(sample.hit_id, Some(touch::JOYSTICK));
    approximately(sample.delta, [std::f32::consts::FRAC_1_SQRT_2; 2]);
    assert_eq!(state.sample()[0].delta, sample.delta);
    state.release(11);
    assert!(state.sample().is_empty());
}

#[test]
fn ui_excluded_begin_cannot_acquire_gameplay_controls_later() {
    let mut state = TouchControlState::default();
    assert!(!state.begin(11, [0.9, 0.1], 1, None));
    assert!(!state.move_to(11, [0.2, 0.8], 2));
    assert!(!state.begin(11, [0.2, 0.8], 3, Some(region(touch::JOYSTICK))));
    assert!(state.sample().is_empty());
    state.release(11);
    assert!(state.begin(11, [0.2, 0.8], 4, Some(region(touch::JOYSTICK))));
}

#[test]
fn second_surface_finger_does_not_steal_or_inherit_the_active_contact() {
    for hit_id in [touch::JOYSTICK, touch::LOOK_SURFACE] {
        let mut state = TouchControlState::default();
        let target = region(hit_id);
        assert!(state.begin(1, [0.2, 0.8], 1, Some(target)));
        assert!(!state.begin(2, [0.2, 0.8], 2, Some(target)));
        state.release(1);
        assert!(!state.move_to(2, [0.2, 0.7], 3));
        assert!(state.sample().is_empty());
        assert!(state.begin(3, [0.2, 0.7], 4, Some(target)));
        assert_eq!(state.sample()[0].contact_id, 3);
    }
}

#[test]
fn one_action_finger_releasing_does_not_drop_a_second_held_finger() {
    let mut state = TouchControlState::default();
    for id in [1, 2] {
        state.begin(id, [0.2, 0.8], id, Some(region(touch::JUMP)));
    }
    state.release(1);
    let samples = state.sample();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].contact_id, 2);
    assert_eq!(samples[0].hit_id, Some(touch::JUMP));
}

#[test]
fn focus_or_context_clear_removes_held_controls_and_queued_look_motion() {
    let mut state = TouchControlState::default();
    state.begin(1, [0.2, 0.8], 1, Some(region(touch::JOYSTICK)));
    state.begin(2, [0.2, 0.8], 2, Some(region(touch::JUMP)));
    state.begin(3, [0.6, 0.3], 3, Some(look()));
    state.move_to(3, [0.7, 0.4], 4);
    state.clear();
    assert!(state.sample().is_empty());
    assert!(!state.move_to(3, [0.8, 0.5], 5));
    state.begin(3, [0.8, 0.5], 6, Some(look()));
    assert_eq!(state.sample()[0].delta, [0.0; 2]);
}

#[test]
fn over_capacity_ignored_contacts_do_not_create_unbounded_tracking() {
    let mut state = TouchControlState::default();
    for id in 0..MAX_TOUCH_CONTACTS as u64 {
        assert!(!state.begin(id, [0.2, 0.8], id, None));
    }
    assert!(!state.begin(100, [0.2, 0.8], 100, Some(region(touch::JUMP))));
    state.release(0);
    assert!(state.begin(101, [0.2, 0.8], 101, Some(region(touch::JUMP))));
    assert_eq!(state.sample().len(), 1);
}

#[test]
fn invalid_geometry_or_coordinates_cannot_produce_invalid_semantic_samples() {
    let mut state = TouchControlState::default();
    let mut invalid = region(touch::JOYSTICK);
    invalid.bounds.max = invalid.bounds.min;
    assert!(!state.begin(1, [0.2, 0.8], 1, Some(invalid)));
    assert!(!state.begin(2, [f32::NAN, 0.8], 2, Some(region(touch::JUMP))));
    assert!(!state.begin(3, [0.9, 0.1], 3, Some(region(touch::JUMP))));
    assert!(state.sample().is_empty());
    assert!(state.begin(4, [0.2, 0.8], 4, Some(region(touch::JUMP))));
    assert!(!state.move_to(4, [f32::INFINITY, 0.8], 5));
    assert!(state.sample().is_empty());
}

#[test]
fn an_ended_finger_frees_its_id_for_a_new_contact_in_the_same_frame() {
    let mut state = TouchControlState::default();
    state.begin(7, [0.2, 0.8], 1, Some(region(touch::JUMP)));
    let ended = state.end(7).unwrap();
    assert_eq!(ended.hit_id, Some(touch::JUMP));
    assert!(state.begin(7, [0.2, 0.7], 2, Some(region(touch::JOYSTICK))));
    let held = state.sample();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].hit_id, Some(touch::JOYSTICK));
    assert_eq!(state.end(8), None);
}
