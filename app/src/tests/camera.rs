use super::*;
use crate::camera::{CameraSettingsAuthority, perspective_pose};
use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalViewPose,
    reset_local_player_session,
};
use crate::semantic_controls::SemanticInputRuntime;
use semantic_input::{Action, ControllerFrame, DeviceFrame, KeyboardMouseFrame, TouchContact};
use ui::UserSettings;

#[test]
fn committed_movement_correction_updates_position_without_overwriting_local_view_rotation() {
    let correction = PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.25, -0.5, 0.75],
        pitch: -15.0,
        yaw: 90.0,
        on_ground: true,
        tick: 55,
    };
    let local_rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, 0.35, -0.2, 0.0);
    let mut view = LocalViewPose::new(Vec3::ZERO, local_rotation);
    let mut settings = CameraSettingsAuthority::default();
    let mut pending_surface_spawn = Some([3, 4]);

    apply_committed_control(
        CommittedControlEvent::PlayerMovementCorrection {
            sequence: 7,
            correction,
            resolved: client_world::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        },
        &mut view,
        &mut settings,
        &mut pending_surface_spawn,
    );

    assert_eq!(view.eye_translation(), Vec3::new(27.5, 111.0, 91.5));
    assert!(view.rotation().abs_diff_eq(local_rotation, 0.0001));
    assert_eq!(pending_surface_spawn, None);
}

#[test]
fn committed_correction_offsets_only_the_third_person_view() {
    let correction = PlayerMovementCorrectionEvent {
        position: [12.0, 72.62, -8.0],
        delta: [0.0; 3],
        pitch: 10.0,
        yaw: 135.0,
        on_ground: true,
        tick: 91,
    };
    let mut view = LocalViewPose::default();
    let mut settings = CameraSettingsAuthority::default();
    let mut pending_surface_spawn = None;

    apply_committed_control(
        CommittedControlEvent::PlayerMovementCorrection {
            sequence: 12,
            correction,
            resolved: client_world::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        },
        &mut view,
        &mut settings,
        &mut pending_surface_spawn,
    );

    let subject = Vec3::from_array(correction.position);
    let camera = perspective_pose(
        view.eye_translation(),
        view.rotation(),
        semantic_input::PerspectiveMode::ThirdPersonBack,
    );
    assert_eq!(view.eye_translation(), subject);
    assert!((camera.translation.distance(subject) - 4.0).abs() < 1.0e-5);
    assert_eq!(pending_surface_spawn, None);
}

#[test]
fn interaction_origin_is_the_local_eye_pose_not_the_offset_camera_pose() {
    let view = LocalViewPose::new(
        Vec3::new(8.0, 72.62, -4.0),
        Quat::from_euler(bevy::math::EulerRot::YXZ, 0.8, -0.25, 0.0),
    );
    let camera = CameraPose::new(perspective_pose(
        view.eye_translation(),
        view.rotation(),
        semantic_input::PerspectiveMode::ThirdPersonBack,
    ));

    let interaction = InteractionOriginSnapshot::from_local_view(7, view);

    assert_eq!(interaction.frame_sequence(), 7);
    assert_eq!(interaction.origin(), view.eye_translation());
    assert!(
        interaction
            .direction()
            .abs_diff_eq(view.rotation() * Vec3::NEG_Z, 1.0e-6)
    );
    assert_ne!(interaction.origin(), camera.transform().translation);

    for perspective in [
        semantic_input::PerspectiveMode::FirstPerson,
        semantic_input::PerspectiveMode::ThirdPersonBack,
        semantic_input::PerspectiveMode::ThirdPersonFront,
    ] {
        let camera = CameraPose::new(perspective_pose(
            view.eye_translation(),
            view.rotation(),
            perspective,
        ));
        let interaction = InteractionOriginSnapshot::from_local_view(7, view);
        assert_eq!(interaction.origin(), view.eye_translation());
        assert!(
            interaction
                .direction()
                .abs_diff_eq(view.rotation() * Vec3::NEG_Z, 1.0e-6)
        );
        if perspective != semantic_input::PerspectiveMode::FirstPerson {
            assert_ne!(interaction.origin(), camera.transform().translation);
        }
    }
}

#[test]
fn local_avatar_is_hidden_first_person_and_rendered_exactly_once_third_person() {
    let view = LocalViewPose::new(Vec3::new(2.0, 66.62, 3.0), Quat::IDENTITY);
    let mut presentation = LocalAvatarPresentation::default();
    presentation.begin_session(4, 91);
    let mut sources = Vec::new();

    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::FirstPerson,
        view,
        &mut sources,
    );
    assert!(sources.iter().all(|source| source.runtime_id != 91));

    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonBack,
        view,
        &mut sources,
    );
    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonBack,
        view,
        &mut sources,
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| source.runtime_id == 91)
            .count(),
        1
    );

    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonFront,
        view,
        &mut sources,
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| source.runtime_id == 91)
            .count(),
        1
    );

    presentation.begin_session(5, 92);
    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonBack,
        view,
        &mut sources,
    );
    assert!(sources.iter().all(|source| source.runtime_id != 91));
    assert_eq!(
        sources
            .iter()
            .filter(|source| source.runtime_id == 92)
            .count(),
        1
    );

    let template = sources
        .iter()
        .find(|source| source.runtime_id == 92)
        .unwrap()
        .clone();
    let mut saturated = (1_000_u64..1_128)
        .map(|runtime_id| {
            let mut source = template.clone();
            source.runtime_id = runtime_id;
            source.unique_id = i64::try_from(runtime_id).unwrap();
            source
        })
        .collect::<Vec<_>>();
    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonFront,
        view,
        &mut saturated,
    );
    assert_eq!(saturated.len(), render::MAX_RENDERED_PLAYERS);
    assert_eq!(
        saturated
            .iter()
            .filter(|source| source.runtime_id == 92)
            .count(),
        1
    );

    presentation.clear();
    saturated.clear();
    presentation.reconcile_sources(
        semantic_input::PerspectiveMode::ThirdPersonBack,
        view,
        &mut saturated,
    );
    assert!(saturated.is_empty());
}

#[test]
fn app_semantic_runtime_preserves_keyboard_controller_touch_equivalence() {
    let frames = [
        DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                keys: vec![0x1a, 0x2c],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        },
        DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                axes: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        },
        DeviceFrame {
            touches: vec![
                TouchContact {
                    contact_id: 1,
                    activity_sequence: 0,
                    position: [0.25, 0.5],
                    delta: [0.0, 0.0],
                    hit_id: None,
                },
                TouchContact {
                    contact_id: 2,
                    activity_sequence: 0,
                    position: [0.75, 0.75],
                    delta: [0.0, 0.0],
                    hit_id: Some(semantic_input::touch::JUMP),
                },
            ],
            ..DeviceFrame::default()
        },
    ];
    let projections = frames.map(|frame| {
        let mut runtime = SemanticInputRuntime::default();
        let snapshot = runtime.route_and_finalize(frame).unwrap();
        (
            snapshot.movement,
            snapshot.phases[Action::Jump as usize].pressed,
            snapshot.phases[Action::Jump as usize].held,
        )
    });
    assert_eq!(projections[0], projections[1]);
    assert_eq!(projections[1], projections[2]);
}

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
