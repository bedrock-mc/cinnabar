use super::*;
use std::num::NonZeroU64;

use crate::camera::{CameraSettingsAuthority, perspective_pose};
use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalAvatarVisibilityCarrier,
    LocalPlayerFrameCarrier, LocalPlayerFrameReset, LocalPlayerFrameSample, LocalViewPose,
    reset_local_player_session,
};
use crate::semantic_controls::{
    SemanticInputAuthorityFrame, SemanticInputRuntime, SemanticTouchTargets,
};
use crate::ui_runtime::UiRuntime;
use bevy::math::Mat4;
use render::{ActorCullView, ActorRenderScene, ActorRenderSource, MAX_RENDERED_PLAYERS};
use semantic_input::{
    Action, ControlSettings, ControllerFrame, DeviceFrame, InputContext, KeyboardMouseFrame,
    ReleaseReason, TouchContact,
};
use ui::UserSettings;

fn frozen_collision_identity() -> sim::WorldCollisionIdentity {
    sim::WorldCollisionIdentity::new(
        sim::CollisionRegistryIdentity {
            protocol: 1001,
            id_space: sim::CollisionIdSpace::Sequential,
            preg_sha256: [0x3a; 32],
        },
        [
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, -2, 7),
                revision: 19,
            },
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, -1, 7),
                revision: 23,
            },
        ],
    )
    .unwrap()
}

fn frozen_local_player_sample_for(
    perspective: semantic_input::PerspectiveMode,
) -> LocalPlayerFrameSample {
    let eye = Vec3::new(8.0, 72.62, -4.0);
    let rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, 0.8, -0.25, 0.0);
    LocalPlayerFrameSample {
        session_generation: 7,
        fifo_sequence: 41,
        physics_tick: 900,
        perspective,
        world_collision_identity: frozen_collision_identity(),
        pose: perspective_pose(eye, rotation, perspective),
        eye,
        rotation,
    }
}

fn frozen_local_player_sample() -> LocalPlayerFrameSample {
    frozen_local_player_sample_for(semantic_input::PerspectiveMode::ThirdPersonBack)
}

#[test]
fn frozen_local_player_frame_samples_pose_and_interaction_identity_atomically() {
    let sample = frozen_local_player_sample();
    let expected = sample.clone();
    let mut carrier = LocalPlayerFrameCarrier::default();

    carrier.publish(sample).unwrap();

    let frozen = carrier.snapshot().expect("one frozen local-player frame");
    assert_eq!(frozen.session_generation(), expected.session_generation);
    assert_eq!(frozen.fifo_sequence(), expected.fifo_sequence);
    assert_eq!(frozen.physics_tick(), expected.physics_tick);
    assert_eq!(frozen.pose_generation(), 1);
    assert_eq!(frozen.perspective(), expected.perspective);
    assert_eq!(
        frozen.world_collision_identity(),
        &expected.world_collision_identity
    );
    assert_eq!(frozen.pose(), &expected.pose);
    assert_eq!(frozen.eye(), expected.eye);
    assert_eq!(frozen.rotation(), expected.rotation);
    assert!(
        frozen
            .direction()
            .abs_diff_eq(expected.rotation * Vec3::NEG_Z, 1.0e-6)
    );
}

#[test]
fn correction_session_and_dimension_resets_invalidate_the_frozen_frame_generation() {
    for reset in [
        LocalPlayerFrameReset::Correction,
        LocalPlayerFrameReset::Session,
        LocalPlayerFrameReset::Dimension,
    ] {
        let mut carrier = LocalPlayerFrameCarrier::default();
        let sample = frozen_local_player_sample();
        carrier.publish(sample.clone()).unwrap();
        let stale_generation = carrier.snapshot().unwrap().pose_generation();

        carrier.reset(reset);

        assert!(carrier.snapshot().is_none());
        let mut replacement = sample;
        replacement.session_generation += u64::from(reset == LocalPlayerFrameReset::Session);
        replacement.fifo_sequence += 1;
        replacement.physics_tick += 1;
        carrier.publish(replacement).unwrap();
        assert!(carrier.snapshot().unwrap().pose_generation() > stale_generation);
    }
}

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
fn local_avatar_publishes_only_frozen_visibility_without_owning_the_render_arena() {
    let mut presentation = LocalAvatarPresentation::default();
    presentation.begin_session(7, 91);
    let mut frame = LocalPlayerFrameCarrier::default();
    frame.publish(frozen_local_player_sample()).unwrap();
    let mut visibility = LocalAvatarVisibilityCarrier::default();

    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);

    let frozen = visibility
        .snapshot()
        .expect("third-person local avatar visibility");
    assert_eq!(frozen.session_generation(), 7);
    assert_eq!(frozen.runtime_id(), 91);
    assert_eq!(
        frozen.pose_generation(),
        frame.snapshot().unwrap().pose_generation()
    );
    assert!(frozen.visible());
    assert_eq!(frozen.eye(), frame.snapshot().unwrap().eye());
    assert_eq!(frozen.rotation(), frame.snapshot().unwrap().rotation());

    frame
        .publish(frozen_local_player_sample_for(
            semantic_input::PerspectiveMode::FirstPerson,
        ))
        .unwrap();
    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);
    assert!(!visibility.snapshot().unwrap().visible());

    presentation.clear();
    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);
    assert!(visibility.snapshot().is_none());

    let local_player_source = include_str!("../local_player.rs");
    let network_source = include_str!("../runtime/network.rs");
    assert!(!local_player_source.contains("ActorRenderSource"));
    assert!(!local_player_source.contains("MAX_RENDERED_PLAYERS"));
    assert!(!network_source.contains("reconcile_sources"));
}

#[test]
fn actor_culling_precedes_the_remote_cap_and_preserves_visible_high_id_and_local_avatar() {
    let mut presentation = LocalAvatarPresentation::default();
    presentation.begin_session(7, 91);
    let mut local_frame = LocalPlayerFrameCarrier::default();
    let mut sample = frozen_local_player_sample();
    sample.eye = Vec3::new(0.0, 65.62, 0.0);
    sample.rotation = Quat::IDENTITY;
    sample.pose = perspective_pose(sample.eye, sample.rotation, sample.perspective);
    local_frame.publish(sample).unwrap();
    let mut local_visibility = LocalAvatarVisibilityCarrier::default();
    presentation.publish_visibility(local_frame.snapshot().unwrap(), &mut local_visibility);

    let source = |runtime_id: u64, position: [f32; 3]| ActorRenderSource {
        runtime_id,
        unique_id: i64::try_from(runtime_id).unwrap(),
        spawn_revision: 1,
        movement_revision: 1,
        previous_position: position,
        previous_pitch_degrees: 0.0,
        previous_yaw_degrees: 0.0,
        previous_head_yaw_degrees: 0.0,
        position,
        pitch_degrees: 0.0,
        yaw_degrees: 0.0,
        head_yaw_degrees: 0.0,
        teleported: false,
        skin: None,
    };
    let mut remote_sources = (1..=u64::try_from(MAX_RENDERED_PLAYERS + 1).unwrap())
        .map(|runtime_id| source(runtime_id, [500.0, 64.0, 0.0]))
        .collect::<Vec<_>>();
    remote_sources.push(source(999, [1.0, 64.0, 0.0]));
    let cull_view = ActorCullView {
        clip_from_world: Mat4::from_translation(Vec3::new(0.0, -64.0, 0.0)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    let mut scene = ActorRenderScene::default();

    let frame = update_actor_render_scene(
        &mut scene,
        1.0,
        Some(cull_view),
        remote_sources,
        local_visibility.snapshot(),
    );

    assert_eq!(
        frame
            .instances
            .iter()
            .map(|actor| actor.runtime_id)
            .collect::<Vec<_>>(),
        vec![999, 91],
        "Phase 4 must cull and cap remote actors before consuming the frozen local carrier",
    );
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
fn semantic_runtime_wires_context_bindings_authority_and_release_at_finalize() {
    let mut runtime = SemanticInputRuntime::default();
    let held_jump = DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: vec![0x2c],
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    assert!(runtime.route_and_finalize(held_jump).unwrap().phases[Action::Jump as usize].held);

    let generation = NonZeroU64::new(9).unwrap();
    runtime.set_context(InputContext::UiFocused);
    runtime
        .replace_bindings(ControlSettings::default())
        .unwrap();
    runtime.replace_authority(generation);
    runtime.release_all(ReleaseReason::SessionReplaced);
    let released = runtime.route_and_finalize(DeviceFrame::default()).unwrap();

    assert_eq!(released.authority_generation, generation);
    assert!(released.phases[Action::Jump as usize].released);
    assert_eq!(
        released.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::SessionReplaced)
    );
}

#[test]
fn semantic_authority_tracks_ui_settings_session_and_dimension_transitions_in_production_order() {
    let mut runtime = SemanticInputRuntime::default();
    let mut ui = UiRuntime::new(1);
    let controls = ControlSettings::default();
    let held_jump = || DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: vec![0x2c],
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    let authority =
        |context, controls_generation, session_generation, dimension| SemanticInputAuthorityFrame {
            context,
            controls_generation,
            controls: controls.clone(),
            session_generation: NonZeroU64::new(session_generation).unwrap(),
            dimension,
        };

    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 1, 0))
        .unwrap();
    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);

    let ui_transition = ui.open_chat();
    runtime
        .synchronize_authority(authority(ui_transition.requested_input_context(), 1, 1, 0))
        .unwrap();
    let ui_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        ui_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::UiFocusTaken),
    );

    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 1, 0))
        .unwrap();
    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 2, 0))
        .unwrap();
    let session_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        session_release.authority_generation,
        NonZeroU64::new(2).unwrap()
    );
    assert_eq!(
        session_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::SessionReplaced),
    );

    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 2, 1))
        .unwrap();
    let dimension_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        dimension_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::DimensionReplaced),
    );

    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 2, 2, 1))
        .unwrap();
    let binding_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        binding_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::BindingChanged),
    );
}

#[test]
fn semantic_runtime_synthesizes_controller_disconnect_and_releases_stale_touch_targets() {
    let mut runtime = SemanticInputRuntime::default();
    let held_controller_jump = DeviceFrame {
        controllers: vec![ControllerFrame {
            device_id: 7,
            buttons: vec![0],
            ..ControllerFrame::default()
        }],
        ..DeviceFrame::default()
    };
    assert!(
        runtime
            .route_and_finalize(held_controller_jump)
            .unwrap()
            .phases[Action::Jump as usize]
            .held
    );

    let disconnected = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert!(disconnected.phases[Action::Jump as usize].released);
    assert_eq!(
        disconnected.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::ControllerDisconnected)
    );

    let mut targets = SemanticTouchTargets::default();
    targets.set(1, semantic_input::touch::JUMP);
    targets.set(2, semantic_input::touch::USE);
    targets.retain_active_contacts([2]);
    assert_eq!(targets.target(1), None);
    assert_eq!(targets.target(2), Some(semantic_input::touch::USE));
    targets.release_all();
    assert_eq!(targets.target(2), None);

    let physical_source = include_str!("../semantic_controls/physical.rs");
    assert!(physical_source.contains("ResMut<'w, SemanticTouchTargets>"));
    assert!(physical_source.contains("retain_active_contacts"));
    let ui_source = include_str!("../ui_runtime/chat_input.rs");
    assert!(ui_source.contains("touch_targets.set"));
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
