use super::*;
use valentine::bedrock::codec::ZigZag32;
use valentine::bedrock::version::v1_26_51::{
    ActorUniqueId, CameraInstruction, CameraInstructionOptionsSplineInstruction,
    CameraInstructionPacket, CameraPacket, CameraShakePacket, EnumsCameraShakeAction,
    EnumsCameraShakeType, ModalFormRequestPacket, TransferPacket,
};

const EPILOGUE_ITERATION_LIMIT: usize = 32;

/// Optional play-stage epilogue appended after the standard script traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlayEpilogue {
    Default,
    ServerDisconnect,
    TruncatedDisconnect,
    ServerTransfer,
    OddTransferEmptyHost,
    OddTransferZeroPort,
    TruncatedTransfer,
    CameraInstructions,
    OddCameraInstruction,
    TruncatedCameraShake,
    OddModalForm,
    TruncatedModalFormRequest,
}

/// The transfer target the well-formed transfer epilogues advertise.
pub(super) const TRANSFER_EPILOGUE_HOST: &str = "game.example.net";
pub(super) const TRANSFER_EPILOGUE_PORT: u16 = 19133;
const DISCONNECT_EPILOGUE_MESSAGE: &str = "We've detected movement cheats";

pub(super) const EPILOGUE_SENTINEL_TIME: i32 = 45_678;
pub(super) const CAMERA_INTERLEAVED_SENTINEL_TIME: i32 = 56_789;

pub(super) fn camera_instruction_epilogue_packets(epilogue: PlayEpilogue) -> Vec<Packet> {
    match epilogue {
        PlayEpilogue::CameraInstructions => vec![
            Packet::new(CameraInstructionPacket {
                camera_instruction: CameraInstruction {
                    clear: Some(true),
                    ..Default::default()
                },
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
            Packet::new(CameraShakePacket {
                intensity: 0.25,
                seconds: 3.0,
                shake_type: EnumsCameraShakeType::Rotational,
                shake_action: EnumsCameraShakeAction::Add,
            }),
            Packet::new(CameraPacket {
                camera_id: ActorUniqueId {
                    actor_unique_id: -11,
                },
                target_player_id: ActorUniqueId {
                    actor_unique_id: -22,
                },
            }),
            set_time(CAMERA_INTERLEAVED_SENTINEL_TIME),
        ],
        PlayEpilogue::OddCameraInstruction => vec![
            Packet::new(CameraInstructionPacket {
                camera_instruction: CameraInstruction {
                    spline: Some(CameraInstructionOptionsSplineInstruction::default()),
                    ..Default::default()
                },
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
        ],
        PlayEpilogue::OddModalForm => vec![
            Packet::new(ModalFormRequestPacket {
                form_id: 5,
                form_uijson: r#"{"type":"form","title""#.to_owned(),
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
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
        PlayEpilogue::TruncatedTransfer => Some((McpePacketName::TransferPacket, &[0x08, 0x00])),
        PlayEpilogue::TruncatedCameraShake => Some((McpePacketName::CameraShakePacket, &[0x00])),
        PlayEpilogue::TruncatedModalFormRequest => {
            Some((McpePacketName::ModalFormRequestPacket, &[0x05]))
        }
        _ => None,
    }
}

/// The core's code for a disconnect reason, as it would forward the server's.
fn reason_code(reason: EnumsConnectionDisconnectFailReason) -> i32 {
    let mut encoded = BytesMut::new();
    reason.encode(&mut encoded).expect("a reason encodes");
    ZigZag32::decode(&mut encoded.freeze(), ())
        .expect("a reason is a signed varint")
        .0
}

/// The packets a boundary epilogue adds to the play batch and, when the script delivers the
/// boundary as the core does, the terminal message standing for its Disconnect or Transfer.
pub(super) fn boundary_epilogue(script: &Script) -> (Vec<Packet>, Option<Bytes>) {
    let sentinel = set_time(EPILOGUE_SENTINEL_TIME);
    match script.epilogue {
        PlayEpilogue::ServerDisconnect if script.terminal_boundary => (
            Vec::new(),
            Some(terminal_frame(
                KIND_DISCONNECT,
                &serde_json::json!({
                    "reason": reason_code(EnumsConnectionDisconnectFailReason::Kicked),
                    "message": DISCONNECT_EPILOGUE_MESSAGE,
                    "filtered_message": "",
                    "hide_screen": false,
                }),
            )),
        ),
        PlayEpilogue::ServerDisconnect => (
            vec![
                Packet::new(DisconnectPacket {
                    reason: EnumsConnectionDisconnectFailReason::Kicked,
                    hide_disconnection_screen: false,
                    messages: DisconnectPacketMessages {
                        message: DISCONNECT_EPILOGUE_MESSAGE.into(),
                        filtered_message: String::new(),
                    },
                }),
                sentinel,
            ],
            None,
        ),
        PlayEpilogue::ServerTransfer if script.terminal_boundary => (
            Vec::new(),
            Some(terminal_frame(
                KIND_TRANSFER,
                &serde_json::json!({
                    "address": TRANSFER_EPILOGUE_HOST,
                    "port": TRANSFER_EPILOGUE_PORT,
                    "reload_world": false,
                }),
            )),
        ),
        // The core forwards an unusable target in a batch rather than ending the session.
        PlayEpilogue::ServerTransfer
        | PlayEpilogue::OddTransferEmptyHost
        | PlayEpilogue::OddTransferZeroPort => {
            let server_address = match script.epilogue {
                PlayEpilogue::OddTransferEmptyHost => String::new(),
                _ => TRANSFER_EPILOGUE_HOST.to_owned(),
            };
            let server_port = match script.epilogue {
                PlayEpilogue::OddTransferZeroPort => 0,
                _ => TRANSFER_EPILOGUE_PORT,
            };
            // The sentinel SetTime proves the session survives (or observes)
            // each transfer boundary in FIFO order before any later traffic.
            (
                vec![
                    Packet::new(TransferPacket {
                        server_address,
                        server_port,
                        reload_world: false,
                        ..Default::default()
                    }),
                    sentinel,
                ],
                None,
            )
        }
        _ => (Vec::new(), None),
    }
}

/// A script ending with `epilogue`, delivered in a batch or as the core's terminal message.
fn epilogue_script(epilogue: PlayEpilogue, cache_enabled: bool, terminal_boundary: bool) -> Script {
    Script {
        epilogue,
        cache_enabled,
        terminal_boundary,
        ..Script::default()
    }
}

/// Drains the ordinary epilogue until the retained terminal event wakes the receiver with the
/// play-boundary sentinel.
async fn expect_session_boundary(session: &mut PlaySession) {
    for _ in 0..EPILOGUE_ITERATION_LIMIT {
        match session.recv_world_event(0).await {
            Err(ProtocolError::SessionBoundary) => return,
            Err(error) => panic!("terminal epilogue failed before its boundary: {error}"),
            Ok(_) => {}
        }
    }
    panic!("terminal epilogue never surfaced its session boundary");
}

/// A server disconnect, in a batch or as the core's terminal message, is retained once with
/// its normalized reason, with and without a resolver.
#[tokio::test]
async fn play_ingress_retains_a_normalized_server_disconnect_reason() {
    for cache_enabled in [false, true] {
        for terminal_boundary in [false, true] {
            let (mut session, core) = joined(epilogue_script(
                PlayEpilogue::ServerDisconnect,
                cache_enabled,
                terminal_boundary,
            ))
            .await;

            expect_session_boundary(&mut session).await;

            let disconnect = session
                .take_server_disconnect()
                .expect("server disconnect reason is retained");
            assert_eq!(disconnect.reason, "Kicked");
            assert_eq!(
                disconnect.message.as_deref(),
                Some(DISCONNECT_EPILOGUE_MESSAGE)
            );
            assert_eq!(disconnect.filtered_message, None);
            assert_eq!(session.decode_error_count(), 0);
            assert!(
                session.take_server_disconnect().is_none(),
                "the retained reason is one-shot"
            );
            core.finish(session).await;
        }
    }
}

#[tokio::test]
async fn truncated_disconnect_wire_stays_fatal_without_a_resolver() {
    let (mut session, core) = joined(epilogue_script(
        PlayEpilogue::TruncatedDisconnect,
        false,
        false,
    ))
    .await;

    for _ in 0..5 {
        session.recv_world_event(0).await.expect("login prelude");
    }
    let error = session
        .recv_world_event(0)
        .await
        .expect_err("truncated DisconnectPacket wire must stay fatal");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(session.decode_error_count(), 1);
    core.finish(session).await;
}

async fn drain_until_transfer_sentinel(session: &mut PlaySession) {
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
    panic!("transfer epilogue sentinel never arrived");
}

/// A server transfer, in a batch or as the core's terminal message, is retained once with its
/// normalized target, with and without a resolver.
#[tokio::test]
async fn play_ingress_retains_a_normalized_server_transfer_target() {
    for cache_enabled in [false, true] {
        for terminal_boundary in [false, true] {
            let (mut session, core) = joined(epilogue_script(
                PlayEpilogue::ServerTransfer,
                cache_enabled,
                terminal_boundary,
            ))
            .await;

            expect_session_boundary(&mut session).await;

            let transfer = session
                .take_server_transfer()
                .expect("the server transfer target is retained");
            assert_eq!(transfer.host, TRANSFER_EPILOGUE_HOST);
            assert_eq!(transfer.port, TRANSFER_EPILOGUE_PORT);
            assert!(!transfer.reload_world);
            assert_eq!(session.decode_error_count(), 0);
            assert_eq!(session.transfer_skip_count(), 0);
            assert!(
                session.take_server_transfer().is_none(),
                "the retained transfer target is one-shot"
            );
            core.finish(session).await;
        }
    }
}

#[tokio::test]
async fn truncated_transfer_wire_stays_fatal() {
    let (mut session, core) = joined(epilogue_script(
        PlayEpilogue::TruncatedTransfer,
        false,
        false,
    ))
    .await;

    for _ in 0..5 {
        session.recv_world_event(0).await.expect("login prelude");
    }
    let error = session
        .recv_world_event(0)
        .await
        .expect_err("truncated TransferPacket wire must stay fatal");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(session.decode_error_count(), 1);
    assert!(session.take_server_transfer().is_none());
    core.finish(session).await;
}

#[tokio::test]
async fn well_formed_but_unusable_transfer_targets_are_counted_semantic_skips() {
    for cache_enabled in [false, true] {
        for epilogue in [
            PlayEpilogue::OddTransferEmptyHost,
            PlayEpilogue::OddTransferZeroPort,
        ] {
            let (mut session, core) = joined(epilogue_script(epilogue, cache_enabled, false)).await;

            // The sentinel SetTime after the odd transfer proves the session
            // survives the semantic rejection and keeps FIFO order.
            drain_until_transfer_sentinel(&mut session).await;

            assert_eq!(
                session.transfer_skip_count(),
                1,
                "{epilogue:?} must count exactly one semantic transfer skip"
            );
            assert!(session.take_server_transfer().is_none());
            assert_eq!(session.decode_error_count(), 0);
            core.finish(session).await;
        }
    }
}
