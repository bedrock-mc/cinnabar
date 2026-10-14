//! Integration tests for app-owned input, ordering and inventory adapters.

use super::*;

mod forms_fixture;
mod forms_interaction_tests;
pub(crate) mod menu_input_tests;

/// A tap that lifts within one frame still routes its control, exactly once.
#[test]
fn lifted_touch_samples_route_once() {
    use crate::semantic_controls::SemanticTouchTargets;
    use semantic_input::{TouchContact, touch};

    let mut targets = SemanticTouchTargets::default();
    let sample = TouchContact {
        contact_id: 4,
        activity_sequence: 0,
        position: [0.6, 0.4],
        delta: [0.1, 0.0],
        hit_id: Some(touch::LOOK_SURFACE),
    };
    targets.set_sample(&sample);
    assert_eq!(targets.target(4), Some(touch::LOOK_SURFACE));
    assert_eq!(targets.delta(4), Some([0.1, 0.0]));
    targets.pulse(sample.clone());
    assert_eq!(targets.take_pulses(), vec![sample]);
    assert!(targets.take_pulses().is_empty());
    targets.release_all();
    assert_eq!(targets.target(4), None);
}

#[test]
fn chat_focus_clears_stale_gameplay_touch_targets() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    use crate::semantic_controls::SemanticTouchTargets;
    use crate::ui_runtime::gameplay_touch::drive_gameplay_touch_targets;
    use bevy::{
        input::touch::Touches,
        prelude::{App, Update},
    };

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    let mut app = App::new();
    let mut targets = SemanticTouchTargets::default();
    targets.set(7, semantic_input::touch::JUMP);
    app.insert_resource(UiRuntimeResource(runtime))
        .insert_resource(player_runtime.clone())
        .init_resource::<Touches>()
        .insert_resource(targets)
        .add_systems(Update, drive_gameplay_touch_targets);

    app.update();

    assert_eq!(
        app.world().resource::<SemanticTouchTargets>().target(7),
        None
    );
}
