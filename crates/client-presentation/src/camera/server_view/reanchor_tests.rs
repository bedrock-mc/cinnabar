use super::*;
use protocol::{CameraEase, CameraTargetInstruction};

fn context() -> ViewContext<'static> {
    ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &|_| None,
    }
}

fn free_camera() -> ServerCameraView {
    let mut view = ServerCameraView::default();
    view.apply(
        0,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: Arc::from("minecraft:free"),
                ..Default::default()
            }]
            .into(),
        ),
        &context(),
    );
    view
}

fn set(ease: Option<CameraEase>) -> CameraEvent {
    CameraEvent::Instruction(Box::new(CameraInstructionEvent {
        set: Some(CameraSetInstruction {
            preset_id: 0,
            ease,
            position: Some([3.0, 4.0, 5.0]),
            rotation_degrees: None,
            facing_position: None,
            view_offset: None,
            entity_offset: None,
            default_preset: None,
            remove_ignore_starting_values: false,
        }),
        ..Default::default()
    }))
}

#[test]
fn instant_server_camera_changes_reset_motion_history_but_eased_changes_do_not() {
    let mut view = free_camera();
    let initial = view.camera_reanchor_epoch();
    view.apply(
        1,
        &set(Some(CameraEase {
            kind: 0,
            time_seconds: 1.0,
        })),
        &context(),
    );
    assert_eq!(view.camera_reanchor_epoch(), initial);
    view.advance(0.5);
    assert_eq!(view.camera_reanchor_epoch(), initial);
    view.apply(2, &set(None), &context());
    assert_ne!(view.camera_reanchor_epoch(), initial);
    let snapped = view.camera_reanchor_epoch();
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &context(),
    );
    assert_ne!(view.camera_reanchor_epoch(), snapped);
}

#[test]
fn instant_projection_change_resets_history_and_eased_fov_keeps_it() {
    for duration in [0.0, 1.0] {
        let mut view = free_camera();
        let initial = view.camera_reanchor_epoch();
        view.apply(
            1,
            &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                fov: Some(CameraFovInstruction {
                    degrees: 50.0,
                    ease_time_seconds: duration,
                    ease_type: Arc::from("linear"),
                    clear: false,
                }),
                ..Default::default()
            })),
            &context(),
        );
        assert_eq!(view.camera_reanchor_epoch() != initial, duration == 0.0);
    }
}

#[test]
fn delayed_target_acquisition_resets_only_the_snap_frame() {
    let mut view = ServerCameraView::default();
    view.apply(
        0,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: Arc::from("custom:snap"),
                inherit_from: Arc::from("minecraft:free"),
                snap_to_target: Some(true),
                rotation_speed: Some(1.0),
                ..Default::default()
            }]
            .into(),
        ),
        &context(),
    );
    view.apply(1, &set(None), &context());
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(CameraTargetInstruction {
                actor_unique_id: 1,
                center_offset: None,
            }),
            ..Default::default()
        })),
        &context(),
    );
    let initial = view.camera_reanchor_epoch();
    view.advance_target(1.0, &context());
    assert_eq!(view.camera_reanchor_epoch(), initial);
    let context = ViewContext {
        actors: &|_| {
            Some(ActorView {
                position: Vec3::new(8.0, 4.0, 5.0),
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
            })
        },
        ..context()
    };
    view.advance_target(1.0, &context);
    assert_ne!(view.camera_reanchor_epoch(), initial);
    let snapped = view.camera_reanchor_epoch();
    view.advance_target(1.0, &context);
    assert_eq!(view.camera_reanchor_epoch(), snapped);
}
