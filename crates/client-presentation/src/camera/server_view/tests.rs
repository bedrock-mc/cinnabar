use std::sync::Arc;

use protocol::{CameraEase, CameraFadeColor, CameraFadeTimes};

use super::*;

/// Supplies the server's free-camera registry for instruction-only fixtures.
fn free_view() -> ServerCameraView {
    let mut view = ServerCameraView::default();
    view.apply(
        0,
        &registry(vec![preset("minecraft:free", "")]),
        &ctx(Transform::IDENTITY),
    );
    view
}

fn no_actors(_: i64) -> Option<ActorView> {
    None
}

fn ctx(base: Transform) -> ViewContext<'static> {
    ViewContext {
        base,
        subject: base,
        base_fov: 90.0,
        actors: &no_actors,
    }
}

fn set_event(set: CameraSetInstruction) -> CameraEvent {
    CameraEvent::Instruction(Box::new(CameraInstructionEvent {
        set: Some(set),
        ..Default::default()
    }))
}

fn empty_set() -> CameraSetInstruction {
    CameraSetInstruction {
        preset_id: 0,
        ease: None,
        position: None,
        rotation_degrees: None,
        facing_position: None,
        view_offset: None,
        entity_offset: None,
        default_preset: None,
        remove_ignore_starting_values: false,
    }
}

#[test]
fn set_with_ease_blends_position_from_the_base_pose() {
    let mut view = free_view();
    let base = Transform::from_xyz(0.0, 0.0, 0.0);
    let mut set = empty_set();
    set.position = Some([10.0, 0.0, 0.0]);
    set.ease = Some(CameraEase {
        kind: 0,
        time_seconds: 2.0,
    });
    view.apply(1, &set_event(set), &ctx(base));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::ZERO
    );
    view.advance(1.0);
    assert!(
        (view
            .pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x
            - 5.0)
            .abs()
            < 1e-4
    );
    view.advance(5.0);
    assert!(
        (view
            .pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x
            - 10.0)
            .abs()
            < 1e-4
    );
}

#[test]
fn instant_set_and_clear() {
    let mut view = free_view();
    let mut set = empty_set();
    set.position = Some([1.0, 2.0, 3.0]);
    view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert!(view.pose_override(&ctx(Transform::IDENTITY)).is_none());
    assert_eq!(view.last_sequence(), 2);
}

#[test]
fn bedrock_rotation_zero_yaw_faces_positive_z() {
    let forward = bedrock_rotation(0.0, 0.0) * Vec3::NEG_Z;
    assert!((forward - Vec3::Z).length() < 1e-5);
    let down = bedrock_rotation(0.0, 90.0) * Vec3::NEG_Z;
    assert!(down.y < -0.99);
}

#[test]
fn facing_position_overrides_rotation() {
    let mut view = free_view();
    let mut set = empty_set();
    set.position = Some([0.0, 0.0, 0.0]);
    set.rotation_degrees = Some([0.0, 0.0]);
    set.facing_position = Some([10.0, 0.0, 0.0]);
    view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
    let forward = view
        .pose_override(&ctx(Transform::IDENTITY))
        .unwrap()
        .rotation
        * Vec3::NEG_Z;
    assert!((forward - Vec3::X).length() < 1e-5);
}

#[test]
fn preset_only_set_is_counted_not_applied() {
    let mut view = ServerCameraView::default();
    view.apply(1, &set_event(empty_set()), &ctx(Transform::IDENTITY));
    assert!(!view.has_pose_override());
    assert_eq!(view.skips().unresolved_presets, 1);
}

#[test]
fn fade_runs_in_hold_out_then_ends() {
    let mut view = free_view();
    view.apply(
        1,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            fade: Some(CameraFadeInstruction {
                time: Some(CameraFadeTimes {
                    fade_in_seconds: 1.0,
                    hold_seconds: 1.0,
                    fade_out_seconds: 1.0,
                }),
                color: Some(CameraFadeColor {
                    red: 1.0,
                    green: 0.0,
                    blue: 2.0,
                }),
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert_eq!(view.fade_overlay(), Some(([1.0, 0.0, 1.0], 0.0)));
    view.advance(0.5);
    assert!((view.fade_overlay().unwrap().1 - 0.5).abs() < 1e-5);
    view.advance(1.0);
    assert_eq!(view.fade_overlay().unwrap().1, 1.0);
    view.advance(1.0);
    assert!((view.fade_overlay().unwrap().1 - 0.5).abs() < 1e-5);
    view.advance(1.0);
    assert!(view.fade_overlay().is_none());
}

#[test]
fn fov_override_blends_and_releases_to_the_setting() {
    let mut view = free_view();
    let fov = |degrees: f32, clear: bool| {
        CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            fov: Some(CameraFovInstruction {
                degrees,
                ease_time_seconds: 1.0,
                ease_type: Arc::from("linear"),
                clear,
            }),
            ..Default::default()
        }))
    };
    view.apply(1, &fov(50.0, false), &ctx(Transform::IDENTITY));
    assert_eq!(view.fov_override_degrees(90.0), Some(90.0));
    view.advance(0.5);
    assert!((view.fov_override_degrees(90.0).unwrap() - 70.0).abs() < 1e-4);
    view.advance(1.0);
    assert!((view.fov_override_degrees(90.0).unwrap() - 50.0).abs() < 1e-4);
    view.apply(2, &fov(0.0, true), &ctx(Transform::IDENTITY));
    view.advance(2.0);
    assert_eq!(view.fov_override_degrees(90.0), None);
}

#[test]
fn shakes_route_by_kind_and_stop() {
    let mut view = free_view();
    let shake = |shake_type, action| {
        CameraEvent::Shake(CameraShakeEvent {
            intensity: 1.0,
            duration_seconds: 2.0,
            shake_type,
            action,
        })
    };
    view.apply(
        1,
        &shake(CameraShakeType::Positional, CameraShakeAction::Add),
        &ctx(Transform::IDENTITY),
    );
    assert!(view.is_active());
    view.apply(
        2,
        &shake(CameraShakeType::Unknown(9), CameraShakeAction::Add),
        &ctx(Transform::IDENTITY),
    );
    assert_eq!(view.skips().unknown_shake, 1);
    view.apply(
        3,
        &shake(CameraShakeType::Positional, CameraShakeAction::Stop),
        &ctx(Transform::IDENTITY),
    );
    assert!(!view.is_active());
}

#[test]
fn upstream_reset_drops_state_but_keeps_counters() {
    let mut view = ServerCameraView::default();
    let mut set = empty_set();
    set.position = Some([1.0, 0.0, 0.0]);
    view.apply(1, &set_event(empty_set()), &ctx(Transform::IDENTITY));
    view.apply(
        1,
        &registry(vec![preset("minecraft:free", "")]),
        &ctx(Transform::IDENTITY),
    );
    view.apply(2, &set_event(set), &ctx(Transform::IDENTITY));
    view.observe_resets(1);
    assert!(!view.has_pose_override());
    assert_eq!(view.skips().unresolved_presets, 1);
}

fn preset(name: &str, inherit: &str) -> CameraPreset {
    CameraPreset {
        name: Arc::from(name),
        inherit_from: Arc::from(inherit),
        ..Default::default()
    }
}

fn registry(presets: Vec<CameraPreset>) -> CameraEvent {
    CameraEvent::Presets(presets.into())
}

fn set_preset(id: u32) -> CameraEvent {
    let mut set = empty_set();
    set.preset_id = id;
    set_event(set)
}

#[test]
fn free_preset_inheritance_supplies_pose_and_explicit_fields_win() {
    let mut view = free_view();
    let mut parent = preset("custom:base", "minecraft:free");
    parent.position = [Some(5.0), Some(6.0), Some(7.0)];
    let mut child = preset("custom:child", "custom:base");
    child.position = [Some(9.0), None, None];
    view.apply(1, &registry(vec![child, parent]), &ctx(Transform::IDENTITY));
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    let translation = view
        .pose_override(&ctx(Transform::IDENTITY))
        .unwrap()
        .translation;
    assert_eq!(translation, Vec3::new(9.0, 6.0, 7.0));

    let mut set = empty_set();
    set.position = Some([1.0, 2.0, 3.0]);
    view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
}

#[test]
fn first_person_preset_restores_the_subject_and_registry_survives_reset() {
    let mut view = free_view();
    let mut free = preset("minecraft:free", "");
    free.position = [Some(1.0); 3];
    view.apply(
        1,
        &registry(vec![free, preset("minecraft:first_person", "")]),
        &ctx(Transform::IDENTITY),
    );
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    assert!(view.has_pose_override());
    let context = ViewContext {
        base: Transform::from_xyz(10.0, 0.0, 0.0),
        subject: Transform::from_xyz(2.0, 3.0, 4.0),
        ..ctx(Transform::IDENTITY)
    };
    view.apply(3, &set_preset(1), &context);
    assert_eq!(
        view.pose_override(&context).unwrap().translation,
        context.subject.translation
    );
    view.apply(4, &set_preset(0), &ctx(Transform::IDENTITY));
    view.observe_resets(1);
    assert!(!view.has_pose_override());
    view.apply(5, &set_preset(0), &ctx(Transform::IDENTITY));
    assert!(view.has_pose_override());
}

#[test]
fn third_person_preset_orbits_the_subject_at_its_radius() {
    let mut view = free_view();
    let mut orbit = preset("test:third_person", "minecraft:third_person");
    orbit.radius = Some(6.0);
    view.apply(1, &registry(vec![orbit]), &ctx(Transform::IDENTITY));
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    let mut context = ctx(Transform::IDENTITY);
    context.subject = Transform::from_xyz(10.0, 70.0, 10.0);
    let camera = view.pose_override(&context).unwrap();
    assert!((camera.translation - Vec3::new(10.0, 70.0, 16.0)).length() < 1e-4);
}

#[test]
fn unknown_preset_ids_are_counted() {
    let mut view = free_view();
    view.apply(1, &set_preset(7), &ctx(Transform::IDENTITY));
    assert_eq!(view.skips().unresolved_presets, 1);
}

#[test]
fn attach_and_target_follow_actor_positions() {
    let actors = |id: i64| {
        (id == 7).then_some(ActorView {
            position: Vec3::new(0.0, 0.0, -10.0),
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &actors,
    };
    let mut view = free_view();
    view.apply(
        1,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            attach_to_entity: Some(7),
            ..Default::default()
        })),
        &context,
    );
    let attached = view.pose_override(&context).unwrap();
    assert_eq!(attached.translation, Vec3::new(0.0, 0.0, -10.0));
    // Bedrock yaw 0 faces +Z.
    assert!(((attached.rotation * Vec3::NEG_Z) - Vec3::Z).length() < 1e-5);

    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(protocol::CameraTargetInstruction {
                center_offset: None,
                actor_unique_id: 7,
            }),
            detach_from_entity: true,
            ..Default::default()
        })),
        &context,
    );
    assert!(view.pose_override(&context).is_none());
    assert_eq!(view.skips().actor_bound, 0);
}

#[test]
fn missing_actors_are_counted_not_fatal() {
    let mut view = free_view();
    view.apply(
        1,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            attach_to_entity: Some(99),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert_eq!(view.skips().actor_bound, 1);
    // The attachment is kept; until the actor exists the player camera stays in charge.
    assert!(view.has_pose_override());
    assert!(view.pose_override(&ctx(Transform::IDENTITY)).is_none());
}

#[test]
fn inherited_native_parent_fields_and_long_chains_are_resolved() {
    let mut view = free_view();
    let mut free = preset("minecraft:free", "");
    free.position = [Some(3.0), Some(4.0), Some(5.0)];
    let mut presets = vec![free];
    for i in 0..12 {
        let parent = if i == 0 {
            "minecraft:free".to_owned()
        } else {
            format!("test:{}", i - 1)
        };
        presets.push(preset(&format!("test:{i}"), &parent));
    }
    view.apply(1, &registry(presets), &ctx(Transform::IDENTITY));
    view.apply(2, &set_preset(12), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(3.0, 4.0, 5.0)
    );
}

#[test]
fn circular_inheritance_is_counted_and_does_not_move_the_camera() {
    let mut view = free_view();
    let mut one = preset("test:one", "test:two");
    one.position = [Some(100.0); 3];
    view.apply(
        1,
        &registry(vec![one, preset("test:two", "test:one")]),
        &ctx(Transform::IDENTITY),
    );
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    assert_eq!(view.skips().unresolved_presets, 1);
    assert!(!view.has_pose_override());
}

#[test]
fn orbit_offsets_are_inherited_and_explicit_values_override_them() {
    let mut view = free_view();
    let mut orbit = preset("minecraft:follow_orbit", "");
    orbit.entity_offset = Some([1.0, 2.0, 0.0]);
    orbit.view_offset = Some([2.0, 3.0]);
    view.apply(
        1,
        &registry(vec![preset("test:orbit", "minecraft:follow_orbit"), orbit]),
        &ctx(Transform::IDENTITY),
    );
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    let camera = view.pose_override(&ctx(Transform::IDENTITY)).unwrap();
    assert!((camera.translation - Vec3::new(3.0, 5.0, 10.0)).length() < 1e-5);
    let mut set = empty_set();
    set.entity_offset = Some([0.0; 3]);
    set.view_offset = Some([0.0; 2]);
    view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(0.0, 0.0, 10.0)
    );
}

#[test]
fn omitted_fields_keep_per_preset_overrides_and_default_resets_pose() {
    let mut view = free_view();
    let mut free = preset("minecraft:free", "");
    free.position = [Some(1.0), Some(2.0), Some(3.0)];
    view.apply(1, &registry(vec![free]), &ctx(Transform::IDENTITY));
    let mut set = empty_set();
    set.position = Some([9.0, 8.0, 7.0]);
    view.apply(2, &set_event(set), &ctx(Transform::IDENTITY));
    view.apply(3, &set_event(empty_set()), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(9.0, 8.0, 7.0)
    );
    set.default_preset = Some(true);
    view.apply(4, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
}

#[test]
fn default_keeps_offsets_until_remove_ignore_is_also_set() {
    let mut view = free_view();
    let mut orbit = preset("test:orbit", "minecraft:follow_orbit");
    orbit.starting_rotation = Some([0.0, 180.0]);
    orbit.view_offset = Some([1.0, 0.0]);
    view.apply(1, &registry(vec![orbit]), &ctx(Transform::IDENTITY));
    let mut set = empty_set();
    set.view_offset = Some([3.0, 0.0]);
    view.apply(2, &set_event(set), &ctx(Transform::IDENTITY));
    set.view_offset = None;
    set.remove_ignore_starting_values = true;
    view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        3.0
    );
    set.default_preset = Some(true);
    set.remove_ignore_starting_values = false;
    view.apply(4, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        3.0
    );
    set.remove_ignore_starting_values = true;
    view.apply(5, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        1.0
    );
}

#[test]
fn unknown_preset_with_explicit_pose_and_offsets_on_free_are_counted() {
    let mut view = free_view();
    let mut set = empty_set();
    set.preset_id = 99;
    set.position = Some([1.0; 3]);
    view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(view.skips().unresolved_presets, 1);
    assert!(!view.has_pose_override());
    set.preset_id = 0;
    set.view_offset = Some([1.0; 2]);
    set.entity_offset = Some([1.0; 3]);
    view.apply(2, &set_event(set), &ctx(Transform::IDENTITY));
    assert_eq!(view.skips().invalid_options, 2);
}

#[test]
fn facing_vertical_and_coincident_targets_preserves_undefined_axes() {
    let previous = bedrock_rotation(45.0, 10.0);
    assert_eq!(facing_rotation(Vec3::ZERO, Vec3::ZERO, previous), previous);
    let upward = facing_rotation(Vec3::ZERO, Vec3::Y, previous);
    assert!(((upward * Vec3::NEG_Z) - Vec3::Y).length() < 1e-5);
    let right = facing_rotation(Vec3::ZERO, Vec3::X, previous);
    assert!(((right * Vec3::NEG_Z) - Vec3::X).length() < 1e-5);
}

/// Defines a linear path whose half-time point lies five blocks along X.
fn spline_definition() -> CameraSpline {
    CameraSpline {
        name: Arc::from("test:path"),
        total_time_seconds: 2.0,
        kind: protocol::CameraSplineKind::Linear,
        control_points: Arc::from([[0.0; 3], [2.0, 0.0, 0.0], [10.0, 0.0, 0.0]]),
        progress_key_frames: [(0.0, 0.0), (1.0, 2.0)]
            .map(
                |(progress, time_seconds)| protocol::CameraSplineProgressKeyFrame {
                    progress,
                    time_seconds,
                    ease_type: Arc::from("linear"),
                },
            )
            .into(),
        rotation_key_frames: [protocol::CameraSplineRotationKeyFrame {
            rotation_degrees: [0.0; 3],
            time_seconds: 0.0,
            ease_type: Arc::from("linear"),
        }]
        .into(),
    }
}

#[test]
fn named_spline_continues_under_set_easing_then_returns_to_stationary_values() {
    let mut view = free_view();
    view.apply(1, &set_preset(0), &ctx(Transform::IDENTITY));
    let path = spline_definition();
    view.apply(
        1,
        &CameraEvent::Splines([path.clone()].into()),
        &ctx(Transform::IDENTITY),
    );
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            spline: Some(protocol::CameraSplineInstruction {
                spline: path,
                load_from_json: true,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    view.advance(1.0);
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        5.0
    );
    let mut set = empty_set();
    set.position = Some([15.0, 0.0, 0.0]);
    set.ease = Some(CameraEase {
        kind: 0,
        time_seconds: 2.0,
    });
    view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
    view.advance(1.0);
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        7.5
    );
    view.advance(1.0);
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        15.0
    );
}

#[test]
fn missing_named_spline_preserves_pose_and_clear_releases_the_selected_camera() {
    let mut view = free_view();
    let mut set = empty_set();
    set.position = Some([3.0, 0.0, 0.0]);
    view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            spline: Some(protocol::CameraSplineInstruction {
                spline: spline_definition(),
                load_from_json: true,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert_eq!(view.skips().invalid_splines, 1);
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        3.0
    );
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            spline: Some(protocol::CameraSplineInstruction {
                spline: spline_definition(),
                load_from_json: false,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    view.apply(
        4,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert!(!view.has_pose_override());
}

#[test]
fn combined_options_use_set_target_remove_clear_then_independent_effects() {
    let mut view = free_view();
    let mut set = empty_set();
    set.position = Some([8.0; 3]);
    view.apply(
        1,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            set: Some(set),
            clear: Some(true),
            fade: Some(CameraFadeInstruction {
                time: None,
                color: None,
            }),
            fov: Some(CameraFovInstruction {
                degrees: 500.0,
                ease_time_seconds: 0.0,
                ease_type: Arc::from("linear"),
                clear: false,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert!(!view.has_pose_override());
    assert!(view.fade_overlay().is_some());
    assert_eq!(view.fov_override_degrees(90.0), Some(110.0));
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            fov: Some(CameraFovInstruction {
                degrees: 1.0,
                ease_time_seconds: 0.0,
                ease_type: Arc::from("linear"),
                clear: false,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert_eq!(view.fov_override_degrees(90.0), Some(30.0));
}

#[test]
fn inactive_stationary_cameras_advance_splines_until_selected() {
    let mut view = free_view();
    view.apply(
        1,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            spline: Some(protocol::CameraSplineInstruction {
                spline: spline_definition(),
                load_from_json: false,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert!(!view.has_pose_override());
    assert_eq!(view.skips().invalid_splines, 0);
    view.advance(1.0);
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        5.0
    );
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    view.advance(0.5);
    view.apply(4, &set_preset(0), &ctx(Transform::IDENTITY));
    assert_eq!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation
            .x,
        7.5
    );
}

#[test]
fn full_camera_frame_evaluation_allocates_nothing() {
    let mut view = free_view();
    view.apply(1, &set_preset(0), &ctx(Transform::IDENTITY));
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            spline: Some(protocol::CameraSplineInstruction {
                spline: spline_definition(),
                load_from_json: false,
            }),
            fade: Some(CameraFadeInstruction {
                time: None,
                color: None,
            }),
            fov: Some(CameraFovInstruction {
                degrees: 45.0,
                ease_time_seconds: 1.0,
                ease_type: Arc::from("out_sine"),
                clear: false,
            }),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    view.apply(
        3,
        &CameraEvent::Shake(CameraShakeEvent {
            intensity: 1.0,
            duration_seconds: 1.0,
            shake_type: CameraShakeType::Rotational,
            action: CameraShakeAction::Add,
        }),
        &ctx(Transform::IDENTITY),
    );
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        view.advance(0.01);
        std::hint::black_box(view.pose_override(&ctx(Transform::IDENTITY)));
        std::hint::black_box(view.fade_overlay());
        std::hint::black_box(view.fov_override_degrees(90.0));
        std::hint::black_box(view.shake_offset());
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}

#[test]
fn target_center_offset_rotates_with_the_target_yaw() {
    let mut view = free_view();
    let actors = |_: i64| {
        Some(ActorView {
            position: Vec3::ZERO,
            yaw_degrees: 90.0,
            pitch_degrees: 0.0,
        })
    };
    let context = ViewContext {
        actors: &actors,
        ..ctx(Transform::IDENTITY)
    };
    view.apply(1, &set_preset(0), &context);
    view.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            target: Some(protocol::CameraTargetInstruction {
                actor_unique_id: 7,
                center_offset: Some([1.0, 0.0, 0.0]),
            }),
            ..Default::default()
        })),
        &context,
    );
    let forward = view.pose_override(&context).unwrap().rotation * Vec3::NEG_Z;
    assert!((forward - Vec3::Z).length() < 1e-5);
}

#[test]
fn server_camera_capabilities_control_first_person_rendering_until_clear() {
    let mut view = free_view();
    let bases = [
        "free",
        "first_person",
        "third_person",
        "third_person_front",
        "follow_orbit",
        "fixed_boom",
    ];
    let presets = bases.map(|base| preset(&format!("minecraft:{base}"), ""));
    view.apply(1, &registry(presets.into()), &ctx(Transform::IDENTITY));
    assert!(view.renders_first_person(true));
    assert!(!view.renders_first_person(false));
    for index in 0..bases.len() {
        view.apply(2, &set_preset(index as u32), &ctx(Transform::IDENTITY));
        assert_eq!(view.renders_first_person(true), index == 1);
        assert_eq!(view.renders_first_person(false), index == 1);
    }
    view.apply(
        3,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        &ctx(Transform::IDENTITY),
    );
    assert!(view.renders_first_person(true));
    assert!(!view.renders_first_person(false));
}

#[test]
fn free_camera_publishes_the_local_body_and_clear_restores_first_person() {
    use crate::local_player::{LocalAvatarPresentation, LocalAvatarVisibilityCarrier};
    let mut view = free_view();
    let mut avatar = LocalAvatarPresentation::default();
    avatar.begin_session(1, 7);
    let mut visibility = LocalAvatarVisibilityCarrier::default();
    let context = ctx(Transform::IDENTITY);
    for (sequence, event, expected) in [
        (2, set_preset(0), true),
        (
            3,
            CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                clear: Some(true),
                ..Default::default()
            })),
            false,
        ),
    ] {
        view.apply(sequence, &event, &context);
        crate::actor_clock::publish_local_actor_visibility(
            &avatar,
            semantic_input::PerspectiveMode::FirstPerson,
            Some(&view),
            Some(Vec3::Y),
            Some(Vec3::ZERO),
            Quat::IDENTITY,
            &mut visibility,
        );
        let snapshot = visibility.snapshot().unwrap();
        assert_eq!(snapshot.visible(), expected);
        assert_eq!(snapshot.feet(), Vec3::ZERO);
        assert_eq!(snapshot.eye(), Vec3::Y);
    }
}

#[test]
fn boom_defaults_apply_starting_angles_and_follow_accepts_only_its_look_input() {
    let mut view = free_view();
    view.apply(
        1,
        &registry(vec![
            preset("minecraft:follow_orbit", ""),
            preset("minecraft:fixed_boom", ""),
        ]),
        &ctx(Transform::IDENTITY),
    );
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    let expected = bedrock_rotation(45.0, 45.0);
    let pose = view.pose_override(&ctx(Transform::IDENTITY)).unwrap();
    assert!(pose.rotation.dot(expected).abs() > 0.99999);
    assert!((pose.translation + expected * Vec3::NEG_Z * 10.0).length() < 1e-5);
    let turned = view.apply_look_delta(Vec2::new(-0.1, 0.2)).unwrap();
    let (yaw, pitch, _) = expected.to_euler(EulerRot::YXZ);
    let expected_turn = Quat::from_euler(EulerRot::YXZ, yaw - 0.1, pitch + 0.2, 0.0);
    assert!(turned.dot(expected_turn).abs() > 0.99999);
    view.apply(3, &set_preset(1), &ctx(Transform::IDENTITY));
    assert!(view.apply_look_delta(Vec2::ONE).is_none());
    assert!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            .dot(expected)
            .abs()
            > 0.99999
    );
    view.apply(4, &set_preset(0), &ctx(Transform::IDENTITY));
    assert!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            .dot(Quat::IDENTITY)
            .abs()
            > 0.99999
    );
    let mut set = empty_set();
    set.default_preset = Some(true);
    set.remove_ignore_starting_values = true;
    view.apply(5, &set_event(set), &ctx(Transform::IDENTITY));
    assert!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            .dot(expected)
            .abs()
            > 0.99999
    );
}

#[test]
fn boom_explicit_rotation_overrides_starting_angles() {
    let mut view = free_view();
    let mut boom = preset("test:boom", "minecraft:fixed_boom");
    boom.starting_rotation = Some([10.0, 80.0]);
    view.apply(1, &registry(vec![boom]), &ctx(Transform::IDENTITY));
    view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
    assert!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            .dot(bedrock_rotation(80.0, 10.0))
            .abs()
            > 0.99999
    );
    let mut set = empty_set();
    set.rotation_degrees = Some([20.0, 120.0]);
    view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
    assert!(
        view.pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            .dot(bedrock_rotation(120.0, 20.0))
            .abs()
            > 0.99999
    );
}
