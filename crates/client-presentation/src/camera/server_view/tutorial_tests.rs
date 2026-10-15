use super::*;
use protocol::{
    CameraEase, CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction,
};

/// Provides a player and a separate actor for server-directed tutorial views.
fn context() -> ViewContext<'static> {
    ViewContext {
        base: bevy::prelude::Transform::from_xyz(2.0, 3.0, 4.0),
        subject: bevy::prelude::Transform::from_xyz(2.0, 3.0, 4.0),
        base_fov: 90.0,
        actors: &|id| {
            (id == 7).then_some(ActorView {
                position: bevy::prelude::Vec3::new(30.0, 10.0, 20.0),
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
            })
        },
    }
}

/// Defines the tutorial rigs and the first-person camera used when it ends.
fn cameras() -> ServerCameraView {
    let mut view = ServerCameraView::default();
    let presets = [
        ("tutorial:orbit", "minecraft:follow_orbit"),
        ("tutorial:boom", "minecraft:fixed_boom"),
        ("minecraft:first_person", ""),
        ("minecraft:free", ""),
    ]
    .map(|(name, inherit_from)| CameraPreset {
        name: name.into(),
        inherit_from: inherit_from.into(),
        ..Default::default()
    });
    view.apply(1, &CameraEvent::Presets(presets.into()), &context());
    view
}

/// Selects a registered rig with an optional transition.
fn select(preset_id: u32, ease: Option<CameraEase>) -> CameraInstructionEvent {
    CameraInstructionEvent {
        set: Some(CameraSetInstruction {
            preset_id,
            ease,
            position: None,
            rotation_degrees: None,
            facing_position: None,
            view_offset: None,
            entity_offset: None,
            default_preset: None,
            remove_ignore_starting_values: false,
        }),
        ..Default::default()
    }
}

/// Sends one instruction through the same presentation ingress as committed packets.
fn instruct(view: &mut ServerCameraView, sequence: u64, instruction: CameraInstructionEvent) {
    view.apply(
        sequence,
        &CameraEvent::Instruction(Box::new(instruction)),
        &context(),
    );
}

#[test]
fn tutorial_attachment_does_not_capture_the_return_to_first_person() {
    let mut view = cameras();
    let mut start = select(
        0,
        Some(CameraEase {
            kind: 0,
            time_seconds: 1.0,
        }),
    );
    start.attach_to_entity = Some(7);
    instruct(&mut view, 2, start);
    view.advance(1.0);
    assert!(view.pose_override(&context()).unwrap().translation.x > 20.0);

    instruct(&mut view, 3, select(2, None));
    assert_eq!(
        view.pose_override(&context()).unwrap().translation,
        context().subject.translation
    );
    assert!(view.renders_first_person(false));
    assert_eq!(view.last_sequence(), 3);
}

#[test]
fn attachment_does_not_activate_a_camera_or_move_a_free_camera() {
    let mut view = cameras();
    instruct(
        &mut view,
        2,
        CameraInstructionEvent {
            attach_to_entity: Some(7),
            ..Default::default()
        },
    );
    assert!(!view.has_pose_override());
    assert!(view.pose_override(&context()).is_none());
    instruct(&mut view, 3, select(3, None));
    let before = view.pose_override(&context()).unwrap();
    instruct(
        &mut view,
        4,
        CameraInstructionEvent {
            attach_to_entity: Some(7),
            ..Default::default()
        },
    );
    assert_eq!(view.pose_override(&context()), Some(before));
}

#[test]
fn clearing_one_rig_preserves_other_rigs_attachments() {
    let mut view = cameras();
    instruct(
        &mut view,
        2,
        CameraInstructionEvent {
            attach_to_entity: Some(7),
            ..Default::default()
        },
    );
    instruct(&mut view, 3, select(0, None));
    instruct(
        &mut view,
        4,
        CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        },
    );
    assert!(view.pose_override(&context()).is_none());
    instruct(&mut view, 5, select(1, None));
    assert!(view.pose_override(&context()).unwrap().translation.x > 20.0);
    instruct(&mut view, 6, select(0, None));
    assert_eq!(view.pose_override(&context()).unwrap().translation.x, 2.0);
}
