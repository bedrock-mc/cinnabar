use super::*;

use crate::camera::CameraSettingsAuthority;
use crate::local_player::{LocalAvatarPresentation, LocalViewPose, reset_local_player_session};
use crate::runtime::world::apply_committed_control;
use crate::semantic_controls::SemanticInputRuntime;
use semantic_input::{Action, DeviceFrame, KeyboardMouseFrame};
use ui::UserSettings;

#[test]
fn held_cycle_perspective_is_one_shot_across_finalize_barriers() {
    let mut runtime = SemanticInputRuntime::default();
    let held_f5 = DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: vec![0x3e],
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };

    let first = runtime.route_and_finalize(held_f5.clone()).unwrap();
    let repeated = runtime.route_and_finalize(held_f5).unwrap();

    assert!(first.phases[Action::CyclePerspective as usize].pressed);
    assert!(!repeated.phases[Action::CyclePerspective as usize].pressed);
    assert!(!repeated.phases[Action::CyclePerspective as usize].held);
}

#[test]
fn start_game_and_dimension_change_reset_perspective_to_first_person() {
    let mut settings = CameraSettingsAuthority::default();
    let mut configured = UserSettings::default();
    configured.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonFront;
    settings.replace(1, &configured).unwrap();
    let mut view = LocalViewPose::default();
    let mut avatar = LocalAvatarPresentation::default();

    reset_local_player_session(
        4,
        91,
        [8.0, 70.62, -2.0],
        &mut settings,
        &mut view,
        &mut avatar,
    );
    assert_eq!(
        settings.perspective(),
        semantic_input::PerspectiveMode::FirstPerson
    );

    configured.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
    settings.replace(2, &configured).unwrap();
    let mut pending_surface_spawn = None;
    apply_committed_control(
        CommittedControlEvent::ChangeDimension {
            change: protocol::ChangeDimensionEvent {
                dimension: 1,
                position: [16.0, 80.0, 24.0],
            },
            resolved: client_world::ResolvedServerPosition {
                position: [16.0, 80.0, 24.0],
                surface_anchor: None,
            },
        },
        &mut view,
        &mut settings,
        &mut pending_surface_spawn,
    );
    assert_eq!(
        settings.perspective(),
        semantic_input::PerspectiveMode::FirstPerson
    );
}
