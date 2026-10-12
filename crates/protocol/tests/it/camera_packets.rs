use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{
    CameraShakeAction as WireShakeAction, CameraShakeType as WireShakeType,
};
use bedrock_protocol_1_26_51::packets::{
    self, Camera as CameraPacket, CameraInstruction as InstructionPacket,
    CameraShake as ShakePacket, PacketId,
};
use bedrock_protocol_1_26_51::types::{
    ActorUniqueID, CameraEntityOffset, CameraFacing, CameraFadeColor as WireFadeColor,
    CameraFadeTimeData, CameraInstructionData, CameraInstructionFade, CameraInstructionFieldOfView,
    CameraInstructionSet, CameraInstructionTarget, CameraInstructionTargetData, CameraPosition,
    CameraRotation, CameraViewOffset,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{
    CameraEase, CameraEvent, CameraFadeColor, CameraFadeInstruction, CameraFadeTimes,
    CameraFovInstruction, CameraInstructionEvent, CameraSetInstruction, CameraShakeAction,
    CameraShakeEvent, CameraShakeType, CameraSwitchEvent, CameraTargetInstruction, ProtocolError,
    WorldEvent, WorldPacketError, decode_batch, encode, into_world_event,
};
type SetEase = bedrock_protocol_1_26_51::types::CameraEase;

#[test]
fn spline_instruction_reaches_camera_presentation() {
    let event = normalized(InstructionPacket {
        camera_instruction: CameraInstructionData {
            spline: Some(bedrock_protocol_1_26_51::types::CameraSplineInstruction {
                total_time: wire::F32LE(2.0),
                type_: wire::U8(1),
                curve: vec![[0.0, 2.0, 0.0].into(), [4.0, 2.0, 0.0].into()],
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    assert!(matches!(
        event,
        Some(WorldEvent::Camera(CameraEvent::Instruction(_)))
    ));
}

#[test]
fn aim_assist_settings_reach_camera_presentation() {
    let event = normalized(packets::CameraAimAssist {
        preset_id: "test:combat".into(),
        view_angle: [30.0, 30.0].into(),
        distance: wire::F32LE(8.0),
        ..Default::default()
    });
    assert!(matches!(event, Some(WorldEvent::Camera(_))));
}

fn normalized<P>(packet: P) -> Option<WorldEvent>
where
    packets::Packet: From<P>,
{
    into_world_event(protocol::Packet::new(packet), 0)
        .expect("well-formed camera packet normalizes")
}

fn instruction_event(instruction: CameraInstructionData) -> CameraInstructionEvent {
    let event = normalized(InstructionPacket {
        camera_instruction: instruction,
    })
    .expect("camera instruction produces an event");
    let WorldEvent::Camera(CameraEvent::Instruction(event)) = event else {
        panic!("expected a camera instruction event")
    };
    *event
}

#[test]
fn set_instruction_normalizes_every_present_option() {
    let event = instruction_event(CameraInstructionData {
        set: Some(CameraInstructionSet {
            preset: wire::U32LE(7),
            ease: Some(SetEase {
                type_: wire::U8(200),
                time: wire::F32LE(0.5),
            }),
            pos: Some(CameraPosition {
                pos: [1.0, 2.0, 3.0].into(),
            }),
            rot: Some(CameraRotation {
                x: wire::F32LE(-10.0),
                y: wire::F32LE(20.0),
            }),
            facing: Some(CameraFacing {
                pos: [-1.0, 64.0, 8.0].into(),
            }),
            view_offset: Some(CameraViewOffset {
                x: wire::F32LE(0.25),
                y: wire::F32LE(-0.75),
            }),
            entity_offset: Some(CameraEntityOffset {
                entity_offset_x: wire::F32LE(1.0),
                entity_offset_y: wire::F32LE(2.5),
                entity_offset_z: wire::F32LE(-3.0),
            }),
            default: Some(true),
            remove_ignore_starting_values_component: true,
        }),
        ..Default::default()
    });
    assert_eq!(
        event,
        CameraInstructionEvent {
            set: Some(CameraSetInstruction {
                preset_id: 7,
                ease: Some(CameraEase {
                    kind: 200,
                    time_seconds: 0.5,
                }),
                position: Some([1.0, 2.0, 3.0]),
                rotation_degrees: Some([-10.0, 20.0]),
                facing_position: Some([-1.0, 64.0, 8.0]),
                view_offset: Some([0.25, -0.75]),
                entity_offset: Some([1.0, 2.5, -3.0]),
                default_preset: Some(true),
                remove_ignore_starting_values: true,
            }),
            clear: None,
            fade: None,
            target: None,
            remove_target: false,
            fov: None,
            attach_to_entity: None,
            detach_from_entity: false,
            spline: None,
        }
    );
}

#[test]
fn clear_fade_and_fov_instructions_normalize() {
    let clear = instruction_event(CameraInstructionData {
        clear: Some(false),
        ..Default::default()
    });
    assert_eq!(clear.clear, Some(false));
    assert_eq!(clear.set, None);

    let fade = instruction_event(CameraInstructionData {
        fade: Some(CameraInstructionFade {
            time: Some(CameraFadeTimeData {
                fade_in_time: wire::F32LE(1.0),
                hold_time: wire::F32LE(2.0),
                fade_out_time: wire::F32LE(3.0),
            }),
            color: Some(WireFadeColor {
                red: wire::F32LE(0.25),
                green: wire::F32LE(0.5),
                blue: wire::F32LE(0.75),
            }),
        }),
        ..Default::default()
    });
    assert_eq!(
        fade.fade,
        Some(CameraFadeInstruction {
            time: Some(CameraFadeTimes {
                fade_in_seconds: 1.0,
                hold_seconds: 2.0,
                fade_out_seconds: 3.0,
            }),
            color: Some(CameraFadeColor {
                red: 0.25,
                green: 0.5,
                blue: 0.75,
            }),
        })
    );

    let fov = instruction_event(CameraInstructionData {
        field_of_view: Some(CameraInstructionFieldOfView {
            field_of_view: wire::F32LE(70.0),
            fov_ease_time: wire::F32LE(1.5),
            fov_ease_type: "spring".into(),
            field_of_view_clear: true,
        }),
        ..Default::default()
    });
    assert_eq!(
        fov.fov,
        Some(CameraFovInstruction {
            degrees: 70.0,
            ease_time_seconds: 1.5,
            ease_type: Arc::from("spring"),
            clear: true,
        })
    );
}

#[test]
fn focus_attach_and_detach_instructions_normalize() {
    let target = instruction_event(CameraInstructionData {
        target: Some(CameraInstructionTargetData {
            target_center_offset: Some([0.5, 0.0, -0.5].into()),
            target_actor_id: wire::I64LE(-42),
        }),
        remove_target: Some(true),
        attach_to_entity: Some(CameraInstructionTarget(1234)),
        detach_from_entity: Some(true),
        ..Default::default()
    });
    assert_eq!(
        target.target,
        Some(CameraTargetInstruction {
            center_offset: Some([0.5, 0.0, -0.5]),
            actor_unique_id: -42,
        })
    );
    assert!(target.remove_target);
    assert_eq!(target.attach_to_entity, Some(1234));
    assert!(target.detach_from_entity);

    let bare_target = instruction_event(CameraInstructionData {
        target: Some(CameraInstructionTargetData {
            target_center_offset: None,
            target_actor_id: wire::I64LE(7),
        }),
        ..Default::default()
    });
    assert_eq!(
        bare_target.target,
        Some(CameraTargetInstruction {
            center_offset: None,
            actor_unique_id: 7,
        })
    );
    assert!(!bare_target.remove_target);
}

#[test]
fn legacy_switch_and_shake_packets_normalize() {
    let event = normalized(CameraPacket {
        camera_id: ActorUniqueID(-3),
        target_player_id: ActorUniqueID(-9),
    })
    .expect("legacy switch normalizes");
    assert_eq!(
        event,
        WorldEvent::Camera(CameraEvent::Switch(CameraSwitchEvent {
            camera_unique_id: -3,
            target_player_unique_id: -9,
        }))
    );

    let event = normalized(ShakePacket {
        intensity: wire::F32LE(0.4),
        seconds: wire::F32LE(2.5),
        shake_type: WireShakeType::Positional,
        shake_action: WireShakeAction::Add,
    })
    .expect("shake add normalizes");
    assert_eq!(
        event,
        WorldEvent::Camera(CameraEvent::Shake(CameraShakeEvent {
            intensity: 0.4,
            duration_seconds: 2.5,
            shake_type: CameraShakeType::Positional,
            action: CameraShakeAction::Add,
        }))
    );

    let event = normalized(ShakePacket {
        intensity: wire::F32LE(0.0),
        seconds: wire::F32LE(0.0),
        shake_type: WireShakeType::Rotational,
        shake_action: WireShakeAction::Stop,
    })
    .expect("shake stop normalizes");
    assert_eq!(
        event,
        WorldEvent::Camera(CameraEvent::Shake(CameraShakeEvent {
            intensity: 0.0,
            duration_seconds: 0.0,
            shake_type: CameraShakeType::Rotational,
            action: CameraShakeAction::Stop,
        }))
    );
}

#[test]
fn unknown_shake_kinds_are_retained_verbatim() {
    let event = normalized(ShakePacket {
        shake_type: WireShakeType::Unknown(211),
        shake_action: WireShakeAction::Unknown(97),
        ..Default::default()
    })
    .expect("unknown shake kinds stay well-formed");
    let WorldEvent::Camera(CameraEvent::Shake(event)) = event else {
        panic!("expected a camera shake event")
    };
    assert_eq!(event.shake_type, CameraShakeType::Unknown(211));
    assert_eq!(event.action, CameraShakeAction::Unknown(97));
}

#[test]
fn non_finite_camera_values_are_semantic_skips() {
    for packet in [
        protocol::Packet::new(ShakePacket {
            intensity: wire::F32LE(f32::NAN),
            ..Default::default()
        }),
        protocol::Packet::new(ShakePacket {
            seconds: wire::F32LE(f32::INFINITY),
            ..Default::default()
        }),
        protocol::Packet::new(InstructionPacket {
            camera_instruction: CameraInstructionData {
                fade: Some(CameraInstructionFade {
                    color: Some(WireFadeColor {
                        green: wire::F32LE(f32::NAN),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }),
        protocol::Packet::new(InstructionPacket {
            camera_instruction: CameraInstructionData {
                field_of_view: Some(CameraInstructionFieldOfView {
                    field_of_view: wire::F32LE(f32::NEG_INFINITY),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }),
        protocol::Packet::new(InstructionPacket {
            camera_instruction: CameraInstructionData {
                set: Some(CameraInstructionSet {
                    pos: Some(CameraPosition {
                        pos: [f32::NAN, 0.0, 0.0].into(),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }),
        protocol::Packet::new(InstructionPacket {
            camera_instruction: CameraInstructionData {
                target: Some(CameraInstructionTargetData {
                    target_center_offset: Some([f32::INFINITY, 0.0, 0.0].into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }),
    ] {
        let error = into_world_event(packet, 0).expect_err("non-finite camera data must skip");
        assert!(
            matches!(&error, WorldPacketError::NonFiniteCameraField { .. }),
            "unexpected error: {error:?}"
        );
        assert!(!is_fatal_wire(&error));
    }
}

#[test]
fn oversized_camera_strings_are_semantic_skips() {
    let packet: protocol::Packet = protocol::Packet::new(InstructionPacket {
        camera_instruction: CameraInstructionData {
            field_of_view: Some(CameraInstructionFieldOfView {
                fov_ease_type: "x"
                    .repeat(protocol::MAX_CAMERA_EASE_IDENTIFIER_BYTES + 1)
                    .into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let error = into_world_event(packet, 0).expect_err("over-long ease name must skip");
    assert!(matches!(
        error,
        WorldPacketError::CameraIdentifierTooLong { .. }
    ));
    assert!(!is_fatal_wire(&error));
}

#[test]
fn invalid_spline_instructions_are_semantic_skips() {
    let packet: protocol::Packet = protocol::Packet::new(InstructionPacket {
        camera_instruction: CameraInstructionData {
            spline: Some(bedrock_protocol_1_26_51::types::CameraSplineInstruction::default()),
            ..Default::default()
        },
    });
    let error = into_world_event(packet, 0).expect_err("spline instructions must skip");
    assert!(matches!(error, WorldPacketError::InvalidCameraField { .. }));
    assert!(!is_fatal_wire(&error));
}

#[test]
fn camera_instruction_batches_round_trip_through_the_codec() {
    let packet: protocol::Packet = protocol::Packet::new(InstructionPacket {
        camera_instruction: CameraInstructionData {
            clear: Some(true),
            fade: Some(CameraInstructionFade {
                color: Some(WireFadeColor {
                    red: wire::F32LE(0.1),
                    green: wire::F32LE(0.2),
                    blue: wire::F32LE(0.3),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let bytes = encode(&packet).expect("encode camera batch");
    let mut decoded = decode_batch(bytes).expect("decode camera batch");
    assert_eq!(decoded.len(), 1);
    let decoded = decoded.pop().expect("one packet");
    assert_eq!(decoded.header.id, PacketId::CameraInstruction);
    let event = into_world_event(decoded, 0)
        .expect("normalize")
        .expect("event");
    let WorldEvent::Camera(CameraEvent::Instruction(event)) = event else {
        panic!("expected a camera instruction event")
    };
    assert_eq!(event.clear, Some(true));
    assert_eq!(
        event.fade.and_then(|fade| fade.color),
        Some(CameraFadeColor {
            red: 0.1,
            green: 0.2,
            blue: 0.3,
        })
    );
}

#[test]
fn truncated_camera_shake_wire_stays_fatal() {
    let packet: protocol::Packet = protocol::Packet::new(ShakePacket::default());
    let bytes = encode(&packet).expect("encode shake batch");
    let truncated = &bytes[..bytes.len() - 1];
    assert!(matches!(
        decode_batch(bytes::Bytes::copy_from_slice(truncated)),
        Err(ProtocolError::TruncatedPacket { .. } | ProtocolError::Decode(_))
    ));
}

#[test]
fn preset_registry_preserves_indices_and_counts_non_finite_rejection() {
    use bedrock_protocol_1_26_51::packets::CameraPresets as CameraPresetsPacket;
    use bedrock_protocol_1_26_51::types::{CameraPreset, CameraPresetList as CameraPresets};
    let preset = |name: &str, radius: f32| CameraPreset {
        name: name.into(),
        inherit_from: "minecraft:free".into(),
        radius: Some(wire::F32LE(radius)),
        pos_x: Some(wire::F32LE(1.0)),
        rot_y: Some(wire::F32LE(20.0)),
        ..Default::default()
    };
    let event = normalized(CameraPresetsPacket {
        camera_presets: CameraPresets {
            presets: vec![preset("a", 4.0), preset("b", 7.0)],
        },
    })
    .expect("presets produce an event");
    let WorldEvent::Camera(CameraEvent::Presets(presets)) = event else {
        panic!("expected a preset registry")
    };
    assert_eq!(presets.len(), 2);
    assert_eq!(&*presets[0].name, "a");
    assert_eq!(presets[0].radius, Some(4.0));
    assert_eq!(presets[0].position, [Some(1.0), None, None]);
    assert_eq!(presets[0].rotation_degrees[1], Some(20.0));
    assert_eq!(presets[1].radius, Some(7.0));
    assert!(matches!(
        into_world_event(
            protocol::Packet::new(CameraPresetsPacket {
                camera_presets: CameraPresets {
                    presets: vec![preset("bad", f32::INFINITY)]
                },
            }),
            0
        ),
        Err(WorldPacketError::NonFiniteCameraField { .. })
    ));
}

fn is_fatal_wire(error: &WorldPacketError) -> bool {
    matches!(error, WorldPacketError::Wire(_))
}
