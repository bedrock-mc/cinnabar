use super::*;
use bedrock_protocol_1_26_51::enums::{CameraShakeAction, CameraShakeType};
use bedrock_protocol_1_26_51::packets::{
    Camera, CameraInstruction, CameraShake, ModalFormRequest, Transfer,
};
use bedrock_protocol_1_26_51::types::{
    ActorUniqueID, CameraInstructionData, CameraSplineInstruction,
};

const EPILOGUE_ITERATION_LIMIT: usize = 32;

/// Optional play-stage epilogue appended after the standard script traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlayEpilogue {
    Default,
    ServerDisconnect,
    TruncatedDisconnect,
    UnknownSelectorDisconnect,
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
            Packet::new(CameraInstruction {
                camera_instruction: CameraInstructionData {
                    clear: Some(true),
                    ..Default::default()
                },
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
            Packet::new(CameraShake {
                intensity: wire::F32LE(0.25),
                seconds: wire::F32LE(3.0),
                shake_type: CameraShakeType::Rotational,
                shake_action: CameraShakeAction::Add,
            }),
            Packet::new(Camera {
                camera_id: ActorUniqueID(-11),
                target_player_id: ActorUniqueID(-22),
            }),
            set_time(CAMERA_INTERLEAVED_SENTINEL_TIME),
        ],
        PlayEpilogue::OddCameraInstruction => vec![
            Packet::new(CameraInstruction {
                camera_instruction: CameraInstructionData {
                    spline: Some(CameraSplineInstruction::default()),
                    ..Default::default()
                },
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
        ],
        PlayEpilogue::OddModalForm => vec![
            Packet::new(ModalFormRequest {
                form_id: wire::VarUInt(5),
                form_ui_json: r#"{"type":"form","title""#.into(),
            }),
            set_time(EPILOGUE_SENTINEL_TIME),
        ],
        _ => Vec::new(),
    }
}

/// The deliberately truncated raw packet each fatality epilogue appends, so
/// every malformed-wire case shares one ingress path.
pub(super) fn truncated_epilogue_wire(epilogue: PlayEpilogue) -> Option<(PacketId, &'static [u8])> {
    match epilogue {
        PlayEpilogue::TruncatedDisconnect => Some((PacketId::Disconnect, &[0x00])),
        PlayEpilogue::TruncatedTransfer => Some((PacketId::Transfer, &[0x08, 0x00])),
        PlayEpilogue::TruncatedCameraShake => Some((PacketId::CameraShake, &[0x00])),
        PlayEpilogue::TruncatedModalFormRequest => Some((PacketId::ModalFormRequest, &[0x05])),
        _ => None,
    }
}

/// A well-framed raw packet the codec rejects, which play must skip before the epilogue
/// sentinel.
pub(super) fn skipped_epilogue_wire(epilogue: PlayEpilogue) -> Option<(PacketId, &'static [u8])> {
    match epilogue {
        // Reason 0, then a messages selector of 2, which names no variant.
        PlayEpilogue::UnknownSelectorDisconnect => Some((PacketId::Disconnect, &[0x00, 0x02])),
        _ => None,
    }
}

/// The core's code for a disconnect reason, as it would forward the server's.
fn reason_code(reason: ConnectionDisconnectFailReason) -> i32 {
    reason.to_raw()
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
                    "reason": reason_code(ConnectionDisconnectFailReason::Kicked),
                    "message": DISCONNECT_EPILOGUE_MESSAGE,
                    "filtered_message": "",
                    "hide_screen": false,
                }),
            )),
        ),
        PlayEpilogue::ServerDisconnect => (
            vec![
                Packet::new(Disconnect {
                    reason: ConnectionDisconnectFailReason::Kicked,
                    messages: DisconnectMessages::DisconnectPacketMessages {
                        message: DISCONNECT_EPILOGUE_MESSAGE.into(),
                        filtered_message: Default::default(),
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
                PlayEpilogue::OddTransferEmptyHost => "",
                _ => TRANSFER_EPILOGUE_HOST,
            };
            let server_port = match script.epilogue {
                PlayEpilogue::OddTransferZeroPort => 0,
                _ => TRANSFER_EPILOGUE_PORT,
            };
            // The sentinel SetTime proves the session survives (or observes)
            // each transfer boundary in FIFO order before any later traffic.
            (
                vec![
                    Packet::new(Transfer {
                        server_address: server_address.into(),
                        server_port: wire::U16LE(server_port),
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

    recv_prelude(&mut session, 5).await;
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

    recv_prelude(&mut session, 5).await;
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

/// A Disconnect with an unknown messages selector is dropped as a counted skip, as vanilla fails
/// its read, rather than ending the session or being retained.
#[tokio::test]
async fn unknown_disconnect_selector_is_a_counted_skip() {
    let (mut session, core) = joined(epilogue_script(
        PlayEpilogue::UnknownSelectorDisconnect,
        false,
        false,
    ))
    .await;

    drain_until_transfer_sentinel(&mut session).await;

    assert!(session.take_server_disconnect().is_none());
    assert_eq!(session.decode_error_count(), 0);
    // The batch's out-of-bounds LevelChunk and the Disconnect.
    assert_eq!(session.world_skip_count(), 2);
    core.finish(session).await;
}
