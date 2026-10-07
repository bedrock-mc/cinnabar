use super::*;
use protocol::{CameraInstructionEvent, CameraTargetInstruction};

/// Supplies an actor-free context for isolated preset behavior.
fn context() -> ViewContext<'static> {
    ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &|_| None,
    }
}

/// Selects the first registered camera without explicit transform overrides.
fn selection() -> CameraEvent {
    CameraEvent::Instruction(Box::new(CameraInstructionEvent {
        set: Some(CameraSetInstruction {
            preset_id: 0,
            ease: None,
            position: None,
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
fn clear_restores_fov_and_inherited_listener_state() {
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![
                CameraPreset {
                    name: "child".into(),
                    inherit_from: "parent".into(),
                    ..Default::default()
                },
                CameraPreset {
                    name: "parent".into(),
                    inherit_from: "minecraft:free".into(),
                    listener: Some(1),
                    ..Default::default()
                },
            ]
            .into(),
        ),
        &context(),
    );
    view.apply(2, &selection(), &context());
    assert_eq!(view.active_listener(), Some(1));
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            fov: Some(CameraFovInstruction {
                degrees: 40.0,
                ease_time_seconds: 0.0,
                ease_type: "linear".into(),
                clear: false,
            }),
            ..Default::default()
        })),
        &context(),
    );
    assert_eq!(view.fov_override_degrees(90.0), Some(40.0));
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &context(),
    );
    assert_eq!(view.fov_override_degrees(90.0), None);
    assert_eq!(view.active_listener(), None);
}

#[test]
fn remove_target_keeps_last_direction_and_new_set_can_override_it() {
    let actors = |_: i64| {
        Some(ActorView {
            position: Vec3::X,
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        actors: &actors,
        ..context()
    };
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    view.apply(2, &selection(), &context);
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(CameraTargetInstruction {
                actor_unique_id: 1,
                center_offset: None,
            }),
            ..Default::default()
        })),
        &context,
    );
    view.advance_target(0.0, &context);
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            remove_target: true,
            ..Default::default()
        })),
        &context,
    );
    let rotation = view.pose_override(&context).unwrap().rotation;
    assert!((rotation * Vec3::NEG_Z).abs_diff_eq(Vec3::X, 1e-5));
    let CameraEvent::Instruction(mut instruction) = selection() else {
        unreachable!()
    };
    instruction.set.as_mut().unwrap().rotation_degrees = Some([0.0, 180.0]);
    view.apply(5, &CameraEvent::Instruction(instruction), &context);
    assert!(
        (view.pose_override(&context).unwrap().rotation * Vec3::NEG_Z)
            .abs_diff_eq(Vec3::NEG_Z, 1e-5)
    );
}

#[test]
fn follow_orbit_yaw_limits_clamp_native_yaw_and_keep_partial_defaults() {
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "custom".into(),
                inherit_from: "minecraft:follow_orbit".into(),
                starting_rotation: Some([0.0, 0.0]),
                yaw_limit_max: Some(30.0),
                ..Default::default()
            }]
            .into(),
        ),
        &context(),
    );
    view.apply(2, &selection(), &context());
    let rotation = view.apply_look_delta(Vec2::new(-1.0, 0.0)).unwrap();
    assert!((rotation * Vec3::NEG_Z).abs_diff_eq(bedrock_rotation(30.0, 0.0) * Vec3::NEG_Z, 1e-5));
    let rotation = view.apply_look_delta(Vec2::new(1.0, 0.0)).unwrap();
    assert!((rotation * Vec3::NEG_Z).abs_diff_eq(
        bedrock_rotation(30.0 - 1.0_f32.to_degrees(), 0.0) * Vec3::NEG_Z,
        1e-5
    ));
}

#[test]
fn custom_boom_starting_values_are_explicit_or_opted_in_and_not_inherited() {
    let subject = Quat::from_euler(EulerRot::YXZ, 0.3, -0.2, 0.0);
    let context = ViewContext {
        subject: Transform::from_rotation(subject),
        ..context()
    };
    for (base, inherit, explicit, expected) in [
        ("minecraft:follow_orbit", false, None, subject),
        (
            "minecraft:follow_orbit",
            true,
            None,
            bedrock_rotation(45.0, 45.0),
        ),
        (
            "minecraft:fixed_boom",
            false,
            None,
            bedrock_rotation(0.0, 0.0),
        ),
        (
            "minecraft:fixed_boom",
            true,
            None,
            bedrock_rotation(45.0, 45.0),
        ),
        (
            "minecraft:follow_orbit",
            false,
            Some([10.0, 20.0]),
            bedrock_rotation(20.0, 10.0),
        ),
    ] {
        let mut view = ServerCameraView::default();
        view.apply(
            1,
            &CameraEvent::Presets(
                vec![
                    CameraPreset {
                        name: "child".into(),
                        inherit_from: "parent".into(),
                        apply_inherited_starting_rotation: inherit,
                        starting_rotation: explicit,
                        ..Default::default()
                    },
                    CameraPreset {
                        name: "parent".into(),
                        inherit_from: base.into(),
                        starting_rotation: Some([50.0, 60.0]),
                        apply_inherited_starting_rotation: true,
                        rotation_degrees: [Some(70.0), Some(80.0)],
                        ..Default::default()
                    },
                ]
                .into(),
            ),
            &context,
        );
        view.apply(2, &selection(), &context);
        assert!(
            view.pose_override(&context)
                .unwrap()
                .rotation
                .dot(expected)
                .abs()
                > 0.99999,
            "{base}: inherited={inherit}, explicit={explicit:?}"
        );
    }
}

#[test]
fn follow_reactivation_resamples_player_but_repeated_set_keeps_orbit_input() {
    let mut view = ServerCameraView::default();
    let context = context();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "orbit".into(),
                inherit_from: "minecraft:follow_orbit".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    view.apply(2, &selection(), &context);
    let changed = view.apply_look_delta(Vec2::new(0.3, 0.2)).unwrap();
    view.apply(3, &selection(), &context);
    assert!(
        view.pose_override(&context)
            .unwrap()
            .rotation
            .dot(changed)
            .abs()
            > 0.99999
    );
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &context,
    );
    view.apply(5, &selection(), &context);
    assert!(
        view.pose_override(&context)
            .unwrap()
            .rotation
            .dot(context.subject.rotation)
            .abs()
            > 0.99999
    );
}

#[test]
fn player_effects_gate_fog_and_lightmap_without_erasing_clocks_or_nausea() {
    let effects = super::super::super::VisionEffects {
        blindness: 0.3,
        darkness: 0.4,
        night_vision: 0.5,
        nausea: 0.6,
    };
    for (base, enabled, expected) in [
        ("minecraft:free", None, false),
        ("minecraft:free", Some(false), false),
        ("minecraft:free", Some(true), true),
        ("minecraft:third_person", Some(false), true),
    ] {
        let mut view = ServerCameraView::default();
        assert_eq!(effects.for_camera(&view), effects);
        view.apply(
            1,
            &CameraEvent::Presets(
                vec![CameraPreset {
                    name: "custom".into(),
                    inherit_from: base.into(),
                    player_effects: enabled,
                    ..Default::default()
                }]
                .into(),
            ),
            &context(),
        );
        view.apply(2, &selection(), &context());
        let rendered = effects.for_camera(&view);
        assert_eq!(rendered.nausea, effects.nausea);
        assert_eq!(
            rendered.blindness,
            if expected { effects.blindness } else { 0.0 }
        );
        assert_eq!(
            rendered.darkness,
            if expected { effects.darkness } else { 0.0 }
        );
        assert_eq!(
            rendered.night_vision,
            if expected { effects.night_vision } else { 0.0 }
        );
        view.clear();
        assert_eq!(effects.for_camera(&view), effects);
    }
}

#[test]
fn experimental_control_scheme_camera_uses_its_declared_follow_parent() {
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:control_scheme_camera".into(),
                inherit_from: "minecraft:follow_orbit".into(),
                control_scheme: Some(1),
                ..Default::default()
            }]
            .into(),
        ),
        &context(),
    );
    view.apply(2, &selection(), &context());
    assert_eq!(
        view.active_base_preset_name(),
        Some("minecraft:follow_orbit")
    );
    assert_eq!(view.active_control_scheme(), Some(1));
    assert!(view.apply_look_delta(Vec2::new(0.2, 0.0)).is_some());
}

#[test]
fn inactive_targets_advance_and_remove_target_only_affects_the_selected_camera() {
    let mut view = ServerCameraView::default();
    let actors = |_: i64| {
        Some(ActorView {
            position: Vec3::new(10.0, 0.0, 0.0),
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        actors: &actors,
        ..context()
    };
    view.apply(
        1,
        &CameraEvent::Presets(
            ["a", "b"]
                .map(|name| CameraPreset {
                    name: name.into(),
                    inherit_from: "minecraft:free".into(),
                    rotation_degrees: [Some(0.0), Some(180.0)],
                    rotation_speed: Some(30.0),
                    ..Default::default()
                })
                .into(),
        ),
        &context,
    );
    view.apply(2, &selection(), &context);
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(CameraTargetInstruction {
                actor_unique_id: 7,
                center_offset: None,
            }),
            ..Default::default()
        })),
        &context,
    );
    view.advance_target(1.0, &context);
    let turned = view.pose_override(&context).unwrap().rotation;
    assert!((turned.angle_between(Quat::IDENTITY).to_degrees() - 30.0).abs() < 1e-4);
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            remove_target: true,
            ..Default::default()
        })),
        &context,
    );
    view.apply(
        5,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &context,
    );
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        view.advance_target(0.001, &context);
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
    let mut select = selection();
    let CameraEvent::Instruction(instruction) = &mut select else {
        unreachable!()
    };
    instruction.set.as_mut().unwrap().preset_id = 1;
    view.apply(6, &select, &context);
    let inactive = view.pose_override(&context).unwrap().rotation;
    assert!((inactive.angle_between(Quat::IDENTITY).to_degrees() - 60.0).abs() < 0.05);
    view.apply(7, &selection(), &context);
    assert!(
        view.pose_override(&context)
            .unwrap()
            .rotation
            .dot(turned)
            .abs()
            > 0.99999
    );
}

#[test]
fn inherited_target_distance_limits_tracking() {
    let mut view = ServerCameraView::default();
    let actors = |_: i64| {
        Some(ActorView {
            position: Vec3::new(5.0, 0.0, 0.0),
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        actors: &actors,
        ..context()
    };
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![
                CameraPreset {
                    name: "a".into(),
                    inherit_from: "b".into(),
                    ..Default::default()
                },
                CameraPreset {
                    name: "b".into(),
                    inherit_from: "minecraft:free".into(),
                    block_listening_radius: Some(4.0),
                    rotation_degrees: [Some(0.0), Some(180.0)],
                    ..Default::default()
                },
            ]
            .into(),
        ),
        &context,
    );
    view.apply(2, &selection(), &context);
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(CameraTargetInstruction {
                actor_unique_id: 7,
                center_offset: None,
            }),
            ..Default::default()
        })),
        &context,
    );
    view.advance_target(1.0, &context);
    assert!(
        view.pose_override(&context)
            .unwrap()
            .rotation
            .dot(Quat::IDENTITY)
            .abs()
            > 0.99999
    );
}

#[test]
fn native_preset_radius_starting_angles_and_yaw_limits_cannot_be_overridden() {
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:follow_orbit".into(),
                radius: Some(1.0),
                starting_rotation: Some([0.0, 0.0]),
                yaw_limit_min: Some(0.0),
                yaw_limit_max: Some(0.0),
                ..Default::default()
            }]
            .into(),
        ),
        &context(),
    );
    view.apply(2, &selection(), &context());
    let pose = view.pose_override(&context()).unwrap();
    assert!((pose.translation.length() - 10.0).abs() < 1e-5);
    assert!(pose.rotation.dot(bedrock_rotation(45.0, 45.0)).abs() > 0.99999);
    let rotation = view.apply_look_delta(Vec2::new(-0.3, 0.0)).unwrap();
    assert!((rotation.angle_between(pose.rotation) - 0.3).abs() < 1e-5);
    assert_eq!(view.skips().invalid_options, 3);
}

#[test]
fn clear_removes_active_target_and_preserves_its_last_instruction_direction() {
    let position = std::cell::Cell::new(Vec3::X * 5.0);
    let actors = |_: i64| {
        Some(ActorView {
            position: position.get(),
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        actors: &actors,
        ..context()
    };
    let mut view = ServerCameraView::default();
    view.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    view.apply(2, &selection(), &context);
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(CameraTargetInstruction {
                actor_unique_id: 7,
                center_offset: None,
            }),
            ..Default::default()
        })),
        &context,
    );
    view.advance_target(0.0, &context);
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &context,
    );
    position.set(Vec3::Z * 5.0);
    view.advance_target(1.0, &context);
    view.apply(5, &selection(), &context);
    assert!(
        (view.pose_override(&context).unwrap().rotation * Vec3::NEG_Z).abs_diff_eq(Vec3::X, 1e-5)
    );
}
