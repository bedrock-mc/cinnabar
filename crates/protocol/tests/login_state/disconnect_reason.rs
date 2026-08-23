use super::*;
use jolyne::stream::transport::Transport;
use jolyne::valentine::{
    ActorUniqueId, CameraInstruction, CameraInstructionOptionsSplineInstruction,
    CameraInstructionPacket, CameraPacket, CameraShakePacket, EnumsCameraShakeAction,
    EnumsCameraShakeType, McpePacketName, ModalFormRequestPacket,
};
use protocol::PlaySession;

const EPILOGUE_ITERATION_LIMIT: usize = 32;

/// Optional play-stage epilogue appended after the standard script traffic.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayEpilogue {
    Default,
    ServerDisconnect,
    TruncatedDisconnect,
    CameraInstructions,
    OddCameraInstruction,
    TruncatedCameraShake,
    OddModalForm,
    TruncatedModalFormRequest,
}

pub(super) const EPILOGUE_SENTINEL_TIME: i32 = 45_678;
pub(super) const CAMERA_INTERLEAVED_SENTINEL_TIME: i32 = 56_789;

pub(super) fn camera_instruction_epilogue_packets(epilogue: PlayEpilogue) -> Vec<McpePacket> {
    match epilogue {
        PlayEpilogue::CameraInstructions => vec![
            McpePacket::from(CameraInstructionPacket {
                camera_instruction: CameraInstruction {
                    clear: Some(true),
                    ..Default::default()
                },
            }),
            McpePacket::from(SetTimePacket {
                time: EPILOGUE_SENTINEL_TIME,
            }),
            McpePacket::from(CameraShakePacket {
                intensity: 0.25,
                seconds: 3.0,
                shake_type: EnumsCameraShakeType::Rotational,
                shake_action: EnumsCameraShakeAction::Add,
            }),
            McpePacket::from(CameraPacket {
                camera_id: ActorUniqueId {
                    actor_unique_id: -11,
                },
                target_player_id: ActorUniqueId {
                    actor_unique_id: -22,
                },
            }),
            McpePacket::from(SetTimePacket {
                time: CAMERA_INTERLEAVED_SENTINEL_TIME,
            }),
        ],
        PlayEpilogue::OddCameraInstruction => vec![
            McpePacket::from(CameraInstructionPacket {
                camera_instruction: CameraInstruction {
                    spline: Some(CameraInstructionOptionsSplineInstruction::default()),
                    ..Default::default()
                },
            }),
            McpePacket::from(SetTimePacket {
                time: EPILOGUE_SENTINEL_TIME,
            }),
        ],
        PlayEpilogue::OddModalForm => vec![
            McpePacket::from(ModalFormRequestPacket {
                form_id: 5,
                form_uijson: r#"{"type":"form","title""#.to_owned(),
            }),
            McpePacket::from(SetTimePacket {
                time: EPILOGUE_SENTINEL_TIME,
            }),
        ],
        _ => Vec::new(),
    }
}

/// The deliberately truncated raw packet each fatality epilogue appends, so
/// every malformed-wire case shares one ingress path.
pub(super) fn truncated_epilogue_wire(
    epilogue: PlayEpilogue,
) -> Option<(McpePacketName, &'static [u8])> {
    match epilogue {
        PlayEpilogue::TruncatedDisconnect => Some((McpePacketName::DisconnectPacket, &[0x00])),
        PlayEpilogue::TruncatedCameraShake => Some((McpePacketName::CameraShakePacket, &[0x00])),
        PlayEpilogue::TruncatedModalFormRequest => {
            Some((McpePacketName::ModalFormRequestPacket, &[0x05]))
        }
        _ => None,
    }
}

impl ScriptTransport {
    pub(super) fn new_with_epilogue(
        mode: CompressionMode,
        order: SpawnOrder,
        epilogue: PlayEpilogue,
    ) -> Self {
        let mut script = ServerScript::new(
            mode,
            order,
            false,
            false,
            false,
            CachePlayScript::ResolveValid,
        );
        script.epilogue = epilogue;
        Self {
            script: Arc::new(Mutex::new(script)),
        }
    }

    pub(super) fn new_with_cache_and_epilogue(
        mode: CompressionMode,
        order: SpawnOrder,
        epilogue: PlayEpilogue,
    ) -> Self {
        let mut script = ServerScript::new(
            mode,
            order,
            false,
            false,
            true,
            CachePlayScript::ResolveValid,
        );
        script.epilogue = epilogue;
        Self {
            script: Arc::new(Mutex::new(script)),
        }
    }
}

pub(super) fn disconnect_epilogue_packets(epilogue: PlayEpilogue) -> Vec<McpePacket> {
    match epilogue {
        PlayEpilogue::Default => Vec::new(),
        PlayEpilogue::ServerDisconnect => vec![
            McpePacket::from(DisconnectPacket {
                reason: EnumsConnectionDisconnectFailReason::Kicked,
                messages: DisconnectPacketMessages {
                    message: "We've detected movement cheats".into(),
                    filtered_message: String::new(),
                },
            }),
            McpePacket::from(SetTimePacket {
                time: EPILOGUE_SENTINEL_TIME,
            }),
        ],
        PlayEpilogue::TruncatedDisconnect
        | PlayEpilogue::CameraInstructions
        | PlayEpilogue::OddCameraInstruction
        | PlayEpilogue::TruncatedCameraShake
        | PlayEpilogue::OddModalForm
        | PlayEpilogue::TruncatedModalFormRequest => vec![],
    }
}

async fn drain_until_disconnect_sentinel<T: Transport>(session: &mut PlaySession<T>) {
    for _ in 0..EPILOGUE_ITERATION_LIMIT {
        match session
            .recv_world_event(0)
            .await
            .expect("epilogue traffic stays decodable")
        {
            WorldEvent::SetTime(protocol::SetTimeEvent { time })
                if time == EPILOGUE_SENTINEL_TIME =>
            {
                return;
            }
            _ => continue,
        }
    }
    panic!("disconnect epilogue sentinel never arrived");
}

#[tokio::test]
async fn play_ingress_retains_a_normalized_server_disconnect_reason() {
    let transport = ScriptTransport::new_with_epilogue(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        PlayEpilogue::ServerDisconnect,
    );
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    drain_until_disconnect_sentinel(&mut session).await;

    let disconnect = session
        .take_server_disconnect()
        .expect("server disconnect reason is retained");
    assert_eq!(disconnect.reason, "Kicked");
    assert_eq!(
        disconnect.message.as_deref(),
        Some("We've detected movement cheats")
    );
    assert_eq!(disconnect.filtered_message, None);
    assert_eq!(session.decode_error_count(), 0);
    assert!(
        session.take_server_disconnect().is_none(),
        "the retained reason is one-shot"
    );
}

#[tokio::test]
async fn cached_play_ingress_retains_a_normalized_server_disconnect_reason() {
    let transport = ScriptTransport::new_with_cache_and_epilogue(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        PlayEpilogue::ServerDisconnect,
    );
    let (mut session, _) = LoginSequence::connect_transport_with_blob_cache(
        transport,
        "RustClient",
        ClientBlobCache::default(),
    )
    .await
    .expect("scripted cache login");

    drain_until_disconnect_sentinel(&mut session).await;

    let disconnect = session
        .take_server_disconnect()
        .expect("cached sessions retain the server disconnect reason");
    assert_eq!(
        disconnect.message.as_deref(),
        Some("We've detected movement cheats")
    );
    assert_eq!(session.decode_error_count(), 0);
}

#[tokio::test]
async fn truncated_disconnect_wire_stays_fatal_without_a_resolver() {
    let transport = ScriptTransport::new_with_epilogue(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        PlayEpilogue::TruncatedDisconnect,
    );
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    for _ in 0..5 {
        session.recv_world_event(0).await.expect("login prelude");
    }
    let error = session
        .recv_world_event(0)
        .await
        .expect_err("truncated DisconnectPacket wire must stay fatal");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(session.decode_error_count(), 1);
}
