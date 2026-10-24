use super::*;

/// No write, explicit clear and set stay distinct across successful events and frame starts.
#[test]
fn retained_environment_lifecycle_preserves_three_state_writes() {
    let mut environment = Environment {
        pending_time: Some(Some(1000)),
        pending_fullbright: Some(true),
        ..Default::default()
    };
    environment.commit();
    environment.begin_frame();
    environment.begin_callback();
    environment.commit();
    assert_eq!(environment.time_override, Some(1000));
    assert!(environment.fullbright);
    environment.pending_time = Some(None);
    environment.commit();
    assert_eq!(environment.time_override, None);
    environment.pending_time = Some(Some(2000));
    environment.pending_weather = Some(Some(
        super::super::cinnabar::extension::environment::Weather::Rain,
    ));
    environment.pending_click_limits = Some([Some(12), None]);
    environment.revoke();
    environment.commit();
    assert_eq!(environment.time_override, None);
    assert!(environment.weather_override.is_none());
    assert_eq!(environment.click_limits, [None; 2]);
    assert!(!environment.fullbright);
}

/// Event staging and a deferred next frame cannot publish a jump from an earlier callback.
#[test]
fn transient_movement_lifecycle_drops_unpublished_output_and_honors_cancellation() {
    let mut movement = Movement {
        pending_jump: true,
        ..Default::default()
    };
    movement.begin_frame();
    movement.commit_frame();
    assert!(!movement.jump_pulse);
    movement.pending_jump = true;
    movement.pending_jump_cancel = true;
    movement.commit_frame();
    assert!(movement.jump_cancel);
    assert!(!movement.jump_pulse);
    movement.begin_frame();
    movement.pending_jump = true;
    movement.commit_frame();
    movement.finish_frame();
    assert!(movement.jump_pulse);
    movement.revoke();
    assert!(!movement.jump_pulse);
    assert!(!movement.jump_cancel);
}

/// Import budgets reset per callback without clearing retained state.
#[test]
fn output_budgets_reset_at_callback_boundaries() {
    let mut environment = Environment::default();
    let mut movement = Movement::default();
    for _ in 0..super::super::MAX_IMPORT_WRITES {
        environment.write().unwrap();
        movement.write().unwrap();
    }
    assert!(environment.write().is_err());
    assert!(movement.write().is_err());
    environment.begin_callback();
    movement.begin_callback();
    environment.write().unwrap();
    movement.write().unwrap();
}
