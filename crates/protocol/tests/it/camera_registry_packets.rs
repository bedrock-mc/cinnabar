use ::protocol;
use bedrock_protocol_1_26_51::enums::{
    CameraAimAssistAction, CameraAimAssistPresetOperation, CameraPresetAudioListener,
    ClientCameraAimAssistAction, TargetMode,
};
use bedrock_protocol_1_26_51::packets::{
    self, CameraAimAssist, CameraAimAssistActorPriority, CameraAimAssistPresets, CameraInstruction,
    CameraPresets, CameraSpline,
};
use bedrock_protocol_1_26_51::types::{
    CameraAimAssistActorPriorityData, CameraAimAssistCategoryDefinition,
    CameraAimAssistCategoryPriorities, CameraAimAssistPresetDefinition,
    CameraAimAssistPresetExclusionDefinition, CameraInstructionData, CameraPreset,
    CameraPresetList, CameraSplineControlPoint, CameraSplineDefinition, CameraSplineInstruction,
    CameraSplineProgressKeyFrame, CameraSplineRotationKeyFrame,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{CameraEvent, CameraSplineKind, WorldEvent, WorldPacketError, into_world_event};

/// Exercises the owned wire codec before checking normalized camera data.
fn camera<P>(packet: P) -> CameraEvent
where
    packets::Packet: From<P>,
{
    let batch = protocol::encode(&protocol::Packet::new(packet)).unwrap();
    let packet = protocol::decode_batch(batch).unwrap().remove(0);
    match into_world_event(packet, 0).unwrap().unwrap() {
        WorldEvent::Camera(event) => event,
        other => panic!("expected camera event, got {other:?}"),
    }
}

#[test]
fn camera_activation_acknowledges_selected_camera_and_support_separately() {
    let packets::Packet::ClientCameraAimAssist(set) =
        *protocol::camera_aim_assist_activation_packet(&"a".repeat(70), true, false).data
    else {
        panic!("activation packet")
    };
    assert_eq!(set.camera_preset_id.to_str().unwrap(), "a".repeat(64));
    assert_eq!(set.action, ClientCameraAimAssistAction::Setfromcamerapreset);
    assert!(set.allow_aim_assist);
    let packets::Packet::ClientCameraAimAssist(clear) =
        *protocol::camera_aim_assist_activation_packet("", false, true).data
    else {
        panic!("activation packet")
    };
    assert_eq!(clear.action, ClientCameraAimAssistAction::Clear);
    assert!(!clear.allow_aim_assist);
    assert!(clear.camera_preset_id.is_empty());
}

#[test]
fn camera_presets_preserve_targeting_limits_starting_rotation_and_listener() {
    let event = camera(CameraPresets {
        camera_presets: CameraPresetList {
            presets: vec![CameraPreset {
                name: "test:orbit".into(),
                inherit_from: "minecraft:follow_orbit".into(),
                rotation_speed: Some(wire::F32LE(3.0)),
                snap_to_target: Some(false),
                horizontal_rotation_limit: Some([-25.0, 35.0].into()),
                vertical_rotation_limit: Some([-50.0, 60.0].into()),
                continue_targeting: Some(true),
                block_listening_radius: Some(wire::F32LE(4.0)),
                yaw_limit_min: Some(wire::F32LE(-90.0)),
                yaw_limit_max: Some(wire::F32LE(100.0)),
                listener: Some(CameraPresetAudioListener::Player),
                player_effects: Some(false),
                apply_inherited_starting_rotation: true,
                starting_rotation: Some([20.0, 30.0].into()),
                ..Default::default()
            }],
        },
    });
    let CameraEvent::Presets(presets) = event else {
        panic!("presets")
    };
    let preset = &presets[0];
    assert_eq!(preset.rotation_speed, Some(3.0));
    assert_eq!(preset.snap_to_target, Some(false));
    assert_eq!(preset.horizontal_rotation_limit, Some([-25.0, 35.0]));
    assert_eq!(preset.vertical_rotation_limit, Some([-50.0, 60.0]));
    assert_eq!(preset.continue_targeting, Some(true));
    assert_eq!(preset.block_listening_radius, Some(4.0));
    assert_eq!(
        (preset.yaw_limit_min, preset.yaw_limit_max),
        (Some(-90.0), Some(100.0))
    );
    assert_eq!(preset.listener, Some(1));
    assert_eq!(preset.player_effects, Some(false));
    assert!(preset.apply_inherited_starting_rotation);
    assert_eq!(preset.starting_rotation, Some([20.0, 30.0]));
}

/// One linear spline definition named `test:flight`.
fn linear_spline() -> CameraSpline {
    CameraSpline {
        camera_data_splines: vec![CameraSplineDefinition {
            name: "test:flight".into(),
            total_time: wire::F32LE(4.0),
            spline_type: "linear".into(),
            control_points: [0.0, 2.0, 6.0]
                .map(|x| CameraSplineControlPoint {
                    position: [x, 3.0, 5.0].into(),
                })
                .into(),
            ..Default::default()
        }],
    }
}

/// A mixed-case spline type breaks the codec's exact-name pattern, so the packet is a skippable
/// rejection rather than an admitted spline.
#[test]
fn camera_spline_type_is_case_sensitive() {
    let encoded = protocol::encode(&protocol::Packet::new(linear_spline())).unwrap();
    assert!(protocol::decode_batch(encoded.clone()).is_ok());
    let position = encoded
        .windows(6)
        .position(|window| window == b"linear")
        .expect("spline type bytes");
    let mut mixed = encoded.to_vec();
    mixed[position..position + 6].copy_from_slice(b"LiNeAr");
    assert!(matches!(
        protocol::decode_batch(mixed.into()),
        Err(protocol::ProtocolError::Ui(
            protocol::UiPacketError::SchemaViolation { .. }
        ))
    ));
}

#[test]
fn camera_spline_registry_keeps_independent_tracks_and_lowercase_type() {
    let event = camera(CameraSpline {
        camera_data_splines: vec![CameraSplineDefinition {
            name: "test:flight".into(),
            total_time: wire::F32LE(4.0),
            spline_type: "linear".into(),
            control_points: [0.0, 2.0, 6.0]
                .map(|x| CameraSplineControlPoint {
                    position: [x, 3.0, 5.0].into(),
                })
                .into(),
            progress_key_frames: vec![CameraSplineProgressKeyFrame {
                progress: wire::F32LE(0.25),
                time: wire::F32LE(1.0),
                easing: Some("in_quad".into()),
            }],
            rotation_key_frames: vec![CameraSplineRotationKeyFrame {
                rotation: [10.0, 270.0, 30.0].into(),
                time: wire::F32LE(2.0),
                easing: None,
            }],
        }],
    });
    let CameraEvent::Splines(paths) = event else {
        panic!("spline registry")
    };
    let path = &paths[0];
    assert_eq!(&*path.name, "test:flight");
    assert_eq!(path.kind, CameraSplineKind::Linear);
    assert_eq!(path.total_time_seconds, 4.0);
    assert_eq!(path.control_points[2], [6.0, 3.0, 5.0]);
    assert_eq!(path.progress_key_frames[0].progress, 0.25);
    assert_eq!(&*path.progress_key_frames[0].ease_type, "in_quad");
    assert_eq!(
        path.rotation_key_frames[0].rotation_degrees,
        [10.0, 270.0, 30.0]
    );
    assert_eq!(path.rotation_key_frames[0].time_seconds, 2.0);
}

#[test]
fn camera_spline_named_reference_does_not_need_inline_points() {
    let event = camera(CameraInstruction {
        camera_instruction: CameraInstructionData {
            spline: Some(CameraSplineInstruction {
                spline_identifier: "test:flight".into(),
                load_from_json: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let CameraEvent::Instruction(instruction) = event else {
        panic!("instruction")
    };
    let spline = instruction.spline.unwrap();
    assert!(spline.load_from_json);
    assert_eq!(&*spline.spline.name, "test:flight");
}

#[test]
fn camera_aim_registry_preserves_categories_exclusions_and_item_settings() {
    let event = camera(CameraAimAssistPresets {
        camera_aim_assist_presets: vec![CameraAimAssistCategoryDefinition {
            name: "combat".into(),
            priorities: CameraAimAssistCategoryPriorities {
                entities: vec![("minecraft:zombie".into(), wire::I32LE(90))],
                entity_default: Some(wire::I32LE(10)),
                block_default: Some(wire::I32LE(0)),
                ..Default::default()
            },
        }],
        camera_aim_assist_categories: vec![CameraAimAssistPresetDefinition {
            identifier: "test:combat".into(),
            exclusion_settings: CameraAimAssistPresetExclusionDefinition {
                entities: vec!["minecraft:player".into()],
                block_tags: vec!["logs".into()],
                ..Default::default()
            },
            item_settings: vec![("minecraft:bow".into(), "combat".into())],
            hand_settings: Some("combat".into()),
            default_item_settings: Some("combat".into()),
            liquid_targeting_list: vec!["minecraft:bucket".into()],
        }],
        operation: CameraAimAssistPresetOperation::Addtoexisting,
    });
    let CameraEvent::AimAssistPresets(registry) = event else {
        panic!("aim registry")
    };
    assert!(!registry.replace);
    assert_eq!(&*registry.categories[0].name, "combat");
    assert_eq!(registry.categories[0].priorities.entities[0].priority, 90);
    assert_eq!(registry.categories[0].priorities.entity_default, Some(10));
    assert_eq!(
        &*registry.presets[0].exclusions.entities[0],
        "minecraft:player"
    );
    assert_eq!(&*registry.presets[0].item_settings[0].category, "combat");
    assert_eq!(
        &*registry.presets[0].liquid_targeting_list[0],
        "minecraft:bucket"
    );
}

#[test]
fn camera_actor_priorities_preserve_signed_indices_for_runtime_validation() {
    let event = camera(CameraAimAssistActorPriority {
        camera_aim_assist_actor_priority_list: vec![CameraAimAssistActorPriorityData {
            preset_index: wire::I32LE(2),
            category_index: wire::I32LE(3),
            actor_index: wire::I32LE(-1),
            priority_value: wire::I32LE(77),
        }],
    });
    let CameraEvent::AimAssistActorPriority(updates) = event else {
        panic!("priorities")
    };
    assert_eq!(
        updates[0],
        protocol::CameraAimAssistActorPriority {
            preset_index: 2,
            category_index: 3,
            actor_index: -1,
            priority: 77,
        }
    );
}

#[test]
fn camera_odd_aim_values_are_semantic_errors() {
    for packet in [
        CameraAimAssist {
            distance: wire::F32LE(f32::NAN),
            ..Default::default()
        },
        CameraAimAssist {
            action: CameraAimAssistAction::Unknown(44),
            ..Default::default()
        },
        CameraAimAssist {
            target_mode: TargetMode::Unknown(44),
            ..Default::default()
        },
    ] {
        let error = into_world_event(protocol::Packet::new(packet), 0).unwrap_err();
        assert!(matches!(
            error,
            WorldPacketError::NonFiniteCameraField { .. }
                | WorldPacketError::InvalidCameraField { .. }
        ));
    }
}

#[test]
fn camera_oversized_spline_registry_is_a_semantic_error() {
    let packet = CameraSpline {
        camera_data_splines: vec![
            CameraSplineDefinition::default();
            protocol::MAX_CAMERA_PRESETS + 1
        ],
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(packet), 0),
        Err(WorldPacketError::CameraCollectionTooLarge { .. })
    ));
}
