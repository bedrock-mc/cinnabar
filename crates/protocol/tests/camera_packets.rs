use std::sync::Arc;

use protocol::{
    BedrockSession, CameraEase, CameraEvent, CameraFadeColor, CameraFadeInstruction,
    CameraFadeTimes, CameraFovInstruction, CameraInstructionEvent, CameraSetInstruction,
    CameraShakeAction, CameraShakeEvent, CameraShakeType, CameraSwitchEvent,
    CameraTargetInstruction, ProtocolError, WorldEvent, WorldPacketError, decode_batch, encode,
    into_world_event,
};
use valentine::bedrock::version::v1_26_44::{
    ActorUniqueId, CameraInstruction, CameraInstructionOptionsAttachToEntityInstruction,
    CameraInstructionOptionsFadeInstruction, CameraInstructionOptionsFadeInstructionColorOption,
    CameraInstructionOptionsFadeInstructionTimeOption, CameraInstructionOptionsFovInstruction,
    CameraInstructionOptionsSetInstruction, CameraInstructionOptionsSetInstructionEaseOption,
    CameraInstructionOptionsSetInstructionEntityOffsetOption,
    CameraInstructionOptionsSetInstructionFacingOption,
    CameraInstructionOptionsSetInstructionPosOption,
    CameraInstructionOptionsSetInstructionRotOption,
    CameraInstructionOptionsSetInstructionViewOffsetOption,
    CameraInstructionOptionsTargetInstruction, CameraPacket, CameraShakePacket,
    CameraShakePacket as ShakePacket, EnumsCameraShakeAction, EnumsCameraShakeType, McpePacketName,
};
type InstructionPacket = valentine::bedrock::version::v1_26_44::CameraInstructionPacket;
type SetEase = CameraInstructionOptionsSetInstructionEaseOption;

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn normalized(packet: impl Into<protocol::Packet>) -> Option<WorldEvent> {
    into_world_event(packet.into(), 0).expect("well-formed camera packet normalizes")
}

fn instruction_event(instruction: CameraInstruction) -> CameraInstructionEvent {
    let event = normalized(InstructionPacket {
        camera_instruction: instruction,
    })
    .expect("camera instruction produces an event");
    let WorldEvent::Camera(CameraEvent::Instruction(event)) = event else {
        panic!("expected a camera instruction event")
    };
    event
}

#[test]
fn set_instruction_normalizes_every_present_option() {
    let event = instruction_event(CameraInstruction {
        set: Some(CameraInstructionOptionsSetInstruction {
            preset: 7,
            ease: Some(SetEase {
                type_: 200,
                time: 0.5,
            }),
            pos: Some(CameraInstructionOptionsSetInstructionPosOption {
                pos: valentine_vec3(1.0, 2.0, 3.0),
            }),
            rot: Some(CameraInstructionOptionsSetInstructionRotOption { x: -10.0, y: 20.0 }),
            facing: Some(CameraInstructionOptionsSetInstructionFacingOption {
                pos: valentine_vec3(-1.0, 64.0, 8.0),
            }),
            view_offset: Some(CameraInstructionOptionsSetInstructionViewOffsetOption {
                x: 0.25,
                y: -0.75,
            }),
            entity_offset: Some(CameraInstructionOptionsSetInstructionEntityOffsetOption {
                entity_offset_x: 1.0,
                entity_offset_y: 2.5,
                entity_offset_z: -3.0,
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
        }
    );
}

#[test]
fn clear_fade_and_fov_instructions_normalize() {
    let clear = instruction_event(CameraInstruction {
        clear: Some(false),
        ..Default::default()
    });
    assert_eq!(clear.clear, Some(false));
    assert_eq!(clear.set, None);

    let fade = instruction_event(CameraInstruction {
        fade: Some(CameraInstructionOptionsFadeInstruction {
            time: Some(CameraInstructionOptionsFadeInstructionTimeOption {
                fade_in_time: 1.0,
                hold_time: 2.0,
                fade_out_time: 3.0,
            }),
            color: Some(CameraInstructionOptionsFadeInstructionColorOption {
                red: 0.25,
                green: 0.5,
                blue: 0.75,
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

    let fov = instruction_event(CameraInstruction {
        field_of_view: Some(CameraInstructionOptionsFovInstruction {
            fieldof_view: 70.0,
            fov_ease_time: 1.5,
            fov_ease_type: "spring".into(),
            fieldof_view_clear: true,
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
    let target = instruction_event(CameraInstruction {
        target: Some(CameraInstructionOptionsTargetInstruction {
            target_center_offset: Some(valentine_vec3(0.5, 0.0, -0.5)),
            target_actor_id: -42,
        }),
        remove_target: Some(true),
        attach_to_entity: Some(CameraInstructionOptionsAttachToEntityInstruction {
            entity_actor_id: 1234,
        }),
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

    let bare_target = instruction_event(CameraInstruction {
        target: Some(CameraInstructionOptionsTargetInstruction {
            target_center_offset: None,
            target_actor_id: 7,
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
        camera_id: ActorUniqueId {
            actor_unique_id: -3,
        },
        target_player_id: ActorUniqueId {
            actor_unique_id: -9,
        },
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
        intensity: 0.4,
        seconds: 2.5,
        shake_type: EnumsCameraShakeType::Positional,
        shake_action: EnumsCameraShakeAction::Add,
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
        intensity: 0.0,
        seconds: 0.0,
        shake_type: EnumsCameraShakeType::Rotational,
        shake_action: EnumsCameraShakeAction::Stop,
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
        shake_type: EnumsCameraShakeType::Unknown(211),
        shake_action: EnumsCameraShakeAction::Unknown(97),
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
        CameraShakePacket {
            intensity: f32::NAN,
            ..Default::default()
        }
        .into(),
        CameraShakePacket {
            seconds: f32::INFINITY,
            ..Default::default()
        }
        .into(),
        InstructionPacket {
            camera_instruction: CameraInstruction {
                fade: Some(CameraInstructionOptionsFadeInstruction {
                    color: Some(CameraInstructionOptionsFadeInstructionColorOption {
                        green: f32::NAN,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }
        .into(),
        InstructionPacket {
            camera_instruction: CameraInstruction {
                field_of_view: Some(CameraInstructionOptionsFovInstruction {
                    fieldof_view: f32::NEG_INFINITY,
                    ..Default::default()
                }),
                ..Default::default()
            },
        }
        .into(),
        InstructionPacket {
            camera_instruction: CameraInstruction {
                set: Some(CameraInstructionOptionsSetInstruction {
                    pos: Some(CameraInstructionOptionsSetInstructionPosOption {
                        pos: valentine_vec3(f32::NAN, 0.0, 0.0),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }
        .into(),
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
    let packet: protocol::Packet = InstructionPacket {
        camera_instruction: CameraInstruction {
            field_of_view: Some(CameraInstructionOptionsFovInstruction {
                fov_ease_type: "x".repeat(protocol::MAX_CAMERA_EASE_IDENTIFIER_BYTES + 1),
                ..Default::default()
            }),
            ..Default::default()
        },
    }
    .into();
    let error = into_world_event(packet, 0).expect_err("over-long ease name must skip");
    assert!(matches!(
        error,
        WorldPacketError::CameraIdentifierTooLong { .. }
    ));
    assert!(!is_fatal_wire(&error));
}

#[test]
fn unsupported_spline_instructions_are_semantic_skips() {
    let packet: protocol::Packet = InstructionPacket {
        camera_instruction: CameraInstruction {
            spline: Some(
                valentine::bedrock::version::v1_26_44::CameraInstructionOptionsSplineInstruction::default(),
            ),
            ..Default::default()
        },
    }
    .into();
    let error = into_world_event(packet, 0).expect_err("spline instructions must skip");
    assert!(matches!(error, WorldPacketError::UnsupportedCameraSpline));
    assert!(!is_fatal_wire(&error));
}

#[test]
fn camera_instruction_batches_round_trip_through_the_codec() {
    let packet: protocol::Packet = InstructionPacket {
        camera_instruction: CameraInstruction {
            clear: Some(true),
            fade: Some(CameraInstructionOptionsFadeInstruction {
                color: Some(CameraInstructionOptionsFadeInstructionColorOption {
                    red: 0.1,
                    green: 0.2,
                    blue: 0.3,
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    }
    .into();
    let bytes = encode(&packet, &session()).expect("encode camera batch");
    let mut decoded = decode_batch(bytes, &session()).expect("decode camera batch");
    assert_eq!(decoded.len(), 1);
    let decoded = decoded.pop().expect("one packet");
    assert_eq!(decoded.header.id, McpePacketName::CameraInstructionPacket);
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
    let packet: protocol::Packet = ShakePacket::default().into();
    let bytes = encode(&packet, &session()).expect("encode shake batch");
    let truncated = &bytes[..bytes.len() - 1];
    assert!(matches!(
        decode_batch(bytes::Bytes::copy_from_slice(truncated), &session()),
        Err(ProtocolError::TruncatedPacket { .. } | ProtocolError::Decode(_))
    ));
}

fn is_fatal_wire(error: &WorldPacketError) -> bool {
    matches!(error, WorldPacketError::Wire(_))
}

fn valentine_vec3(x: f32, y: f32, z: f32) -> valentine::bedrock::version::v1_26_44::Vec3 {
    valentine::bedrock::version::v1_26_44::Vec3 { x, y, z }
}
