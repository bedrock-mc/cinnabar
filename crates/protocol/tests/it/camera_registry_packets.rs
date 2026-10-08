use ::protocol;
use protocol::{CameraEvent, CameraSplineKind, WorldEvent, WorldPacketError, into_world_event};
use valentine::bedrock::version::v1_26_51::*;

/// Exercises the owned wire codec before checking normalized camera data.
fn camera(packet: impl Into<protocol::Packet>) -> CameraEvent {
    let session = protocol::BedrockSession { shield_item_id: 0 };
    let batch = protocol::encode(&packet.into(), &session).unwrap();
    let packet = protocol::decode_batch(batch, &session).unwrap().remove(0);
    match into_world_event(packet, 0).unwrap().unwrap() {
        WorldEvent::Camera(event) => event,
        other => panic!("expected camera event, got {other:?}"),
    }
}

#[test]
fn camera_activation_acknowledges_selected_camera_and_support_separately() {
    let McpePacketData::ClientCameraAimAssistPacket(set) =
        protocol::camera_aim_assist_activation_packet(&"a".repeat(70), true, false).data
    else {
        panic!("activation packet")
    };
    assert_eq!(set.camera_preset_id, "a".repeat(64));
    assert_eq!(
        set.action,
        EnumsClientCameraAimAssistPacketAction::Setfromcamerapreset
    );
    assert!(set.allowaimassist);
    let McpePacketData::ClientCameraAimAssistPacket(clear) =
        protocol::camera_aim_assist_activation_packet("", false, true).data
    else {
        panic!("activation packet")
    };
    assert_eq!(clear.action, EnumsClientCameraAimAssistPacketAction::Clear);
    assert!(!clear.allowaimassist);
    assert!(clear.camera_preset_id.is_empty());
}

#[test]
fn camera_presets_preserve_targeting_limits_starting_rotation_and_listener() {
    let event = camera(CameraPresetsPacket {
        camera_presets: CameraPresets {
            presets: vec![SharedTypesv12650CameraPreset {
                name: "test:orbit".into(),
                inherit_from: "minecraft:follow_orbit".into(),
                rotation_speed: Some(3.0),
                snapto_target: Some(false),
                horizontal_rotation_limit: Some(Vec2 { x: -25.0, y: 35.0 }),
                vertical_rotation_limit: Some(Vec2 { x: -50.0, y: 60.0 }),
                continue_targeting: Some(true),
                block_listening_radius: Some(4.0),
                yaw_limit_min: Some(-90.0),
                yaw_limit_max: Some(100.0),
                listener: Some(EnumsSharedTypesv12190CameraPresetAudioListener::Player),
                player_effects: Some(false),
                apply_inherited_starting_rotation: true,
                starting_rotation: Some(Vec2 { x: 20.0, y: 30.0 }),
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

#[test]
fn camera_spline_registry_keeps_independent_tracks_and_case_insensitive_type() {
    let event = camera(CameraSplinePacket {
        camera_data_splines: vec![SharedTypesv1260CameraSplineDefinition {
            name: "test:flight".into(),
            total_time: 4.0,
            spline_type: "LiNeAr".into(),
            control_points: [0.0, 2.0, 6.0]
                .map(|x| SharedTypesv1260CameraSplineControlPoint {
                    position: Vec3 { x, y: 3.0, z: 5.0 },
                })
                .into(),
            progress_key_frames: vec![SharedTypesv1260CameraSplineProgressKeyFrame {
                progress: 0.25,
                time: 1.0,
                easing: Some("in_quad".into()),
            }],
            rotation_key_frames: vec![SharedTypesv1260CameraSplineRotationKeyFrame {
                rotation: Vec3 {
                    x: 10.0,
                    y: 270.0,
                    z: 30.0,
                },
                time: 2.0,
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
    let event = camera(CameraInstructionPacket {
        camera_instruction: CameraInstruction {
            spline: Some(CameraInstructionOptionsSplineInstruction {
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
    let event = camera(CameraAimAssistPresetsPacket {
        camera_aim_assist_presets: vec![SharedTypesv12150CameraAimAssistCategoryDefinition {
            name: "combat".into(),
            priorities: SharedTypesv12150CameraAimAssistCategoryPriorities {
                entities: vec![
                    SharedTypesv12150CameraAimAssistCategoryPrioritiesEntitiesItem {
                        key: "minecraft:zombie".into(),
                        value: 90,
                    },
                ],
                entity_default: Some(10),
                block_default: Some(0),
                ..Default::default()
            },
        }],
        camera_aim_assist_categories: vec![SharedTypesv121120CameraAimAssistPresetDefinition {
            identifier: "test:combat".into(),
            exclusion_settings: SharedTypesv121120CameraAimAssistPresetExclusionDefinition {
                entities: vec!["minecraft:player".into()],
                block_tags: vec!["logs".into()],
                ..Default::default()
            },
            item_settings: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                key: "minecraft:bow".into(),
                value: "combat".into(),
            }],
            hand_settings: Some("combat".into()),
            default_item_settings: Some("combat".into()),
            liquid_targeting_list: vec!["minecraft:bucket".into()],
        }],
        operation: EnumsCameraAimAssistPresetsPacketOperation::Addtoexisting,
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
    let event = camera(CameraAimAssistActorPriorityPacket {
        camera_aim_assist_actor_priority_list: vec![CameraAimAssistActorPriorityPriorityData {
            preset_index: 2,
            category_index: 3,
            actor_index: -1,
            priority_value: 77,
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
        CameraAimAssistPacket {
            distance: f32::NAN,
            ..Default::default()
        },
        CameraAimAssistPacket {
            action: EnumsCameraAimAssistPacketPayloadAction::Unknown(44),
            ..Default::default()
        },
        CameraAimAssistPacket {
            target_mode: EnumsCameraAimAssistPacketPayloadTargetMode::Unknown(44),
            ..Default::default()
        },
    ] {
        let error = into_world_event(packet.into(), 0).unwrap_err();
        assert!(matches!(
            error,
            WorldPacketError::NonFiniteCameraField { .. }
                | WorldPacketError::InvalidCameraField { .. }
        ));
    }
}

#[test]
fn camera_oversized_spline_registry_is_a_semantic_error() {
    let packet = CameraSplinePacket {
        camera_data_splines: vec![
            SharedTypesv1260CameraSplineDefinition::default();
            protocol::MAX_CAMERA_PRESETS + 1
        ],
    };
    assert!(matches!(
        into_world_event(packet.into(), 0),
        Err(WorldPacketError::CameraCollectionTooLarge { .. })
    ));
}
