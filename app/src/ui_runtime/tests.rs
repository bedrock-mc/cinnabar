//! Integration tests for app-owned input, ordering and inventory adapters.

use super::*;

mod forms_fixture;
mod forms_interaction_tests;
pub(crate) mod menu_input_tests;

#[test]
fn gameplay_touch_targets_remain_unreachable_without_native_layout_authority() {
    use crate::semantic_controls::SemanticTouchTargets;
    use crate::ui_runtime::gameplay_touch::{
        GameplayTouchSample, reconcile_gameplay_touch_targets,
    };

    let mut targets = SemanticTouchTargets::default();
    reconcile_gameplay_touch_targets(
        &mut targets,
        &[
            GameplayTouchSample::new(1, [0.25, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(2, [0.75, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(3, [0.90, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(4, [0.70, 0.40], [0.08, 0.01]),
            GameplayTouchSample::new(5, [0.25, 0.25], [0.0, 0.0]),
        ],
    );

    assert_eq!(targets.target(1), None);
    assert_eq!(targets.target(2), None);
    assert_eq!(targets.target(3), None);
    assert_eq!(targets.target(4), None);
    assert_eq!(targets.target(5), None);

    reconcile_gameplay_touch_targets(
        &mut targets,
        &[GameplayTouchSample::new(4, [0.62, 0.40], [-0.08, 0.01])],
    );
    assert_eq!(targets.target(2), None);
    assert_eq!(targets.target(3), None);
    assert_eq!(targets.target(4), None);

    reconcile_gameplay_touch_targets(&mut targets, &[]);
    assert_eq!(targets.target(4), None);
}

#[test]
fn chat_focus_clears_stale_gameplay_touch_targets() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
    app.insert_resource(runtime)
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
