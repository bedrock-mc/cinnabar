//! Unknown unused startup entries must not replace actual protocol progress with an error.

use super::early_spawn_tests::{Route, fixture_route};
use super::*;
use crate::{LoginSequence, Packet, ProtocolError, SessionLogin};
use bedrock_net::{
    BatchCodec, BatchLimits, BatchPhase, CompressionAlgorithm, CompressionSettings, DecodeOutcome,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID, wire};
use nethernet_tokio::AcceptedSession;
use std::time::Duration;

mod live;

#[derive(Clone, Copy, Debug)]
enum Case {
    Healthy,
    Settings,
    Offer,
    LargeOffer,
    LargeLive,
    Stack,
    MalformedHeader,
    MalformedSettings,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Stage {
    Settings,
    Offer,
    Stack,
    Live,
}

impl Case {
    /// Selects one real server send, without changing the original negotiation order.
    fn stage(self) -> Option<Stage> {
        match self {
            Self::Healthy | Self::LargeLive => None,
            Self::Settings | Self::MalformedHeader | Self::MalformedSettings => {
                Some(Stage::Settings)
            }
            Self::Offer | Self::LargeOffer => Some(Stage::Offer),
            Self::Stack => Some(Stage::Stack),
        }
    }

    /// Encodes an actual unused header, or deliberately malformed input for the rejection controls.
    fn entry(self) -> Bytes {
        if matches!(self, Self::MalformedHeader) {
            return Bytes::from_static(&[0x80]);
        }
        let id = if matches!(self, Self::MalformedSettings) {
            packets::PacketId::NetworkSettings as u32
        } else {
            (0..=wire::PacketHeader::ID_MASK)
                .rev()
                .find(|id| packets::PacketId::from_raw(*id).is_none())
                .expect("the selected protocol must have an unused packet ID")
        };
        let mut bytes = Vec::new();
        wire::Writer::with_limit(&mut bytes, BatchLimits::default().max_decoded_bytes)
            .write_var_u32(wire::PacketHeader::new(id).to_raw())
            .unwrap();
        if matches!(self, Self::LargeOffer) {
            bytes.resize(BatchLimits::default().max_decoded_bytes + 1, b'x');
        } else if !matches!(self, Self::MalformedSettings) {
            bytes.extend_from_slice(b"unused extension body");
        }
        bytes.into()
    }
}

/// Coalesces the chosen entry with original generated siblings in the same real reliable batch.
async fn send<S: BatchPhase>(
    peer: &AcceptedSession,
    codec: &BatchCodec<S>,
    case: Case,
    stage: Stage,
    packets: &[Packet],
) -> io::Result<()> {
    let mut entries = Vec::new();
    if case.stage() == Some(stage) {
        entries.push(case.entry());
    }
    entries.extend(
        packets
            .iter()
            .map(crate::session_transport::tests::packet_bytes),
    );
    let messages = codec.encode(&entries).map_err(io::Error::other)?;
    let large = matches!(
        (case, stage),
        (Case::LargeOffer, Stage::Offer) | (Case::LargeLive, Stage::Live)
    );
    if large
        && (messages.len() != 1
            || messages[0].first().copied()
                != u8::try_from(CompressionAlgorithm::Deflate.network_id()).ok()
            || messages[0].len() > nethernet::session::Channel::Reliable.max_message_size())
    {
        return Err(io::Error::other(
            "large decoded fixture must fit one compressed original reliable message",
        ));
    }
    for message in messages {
        peer.session.send(message).await.map_err(io::Error::other)?;
    }
    Ok(())
}

/// Reads through the real codec but returns peer termination to the fixture's cleanup owner.
async fn receive<S: BatchPhase>(
    peer: &mut AcceptedSession,
    codec: &BatchCodec<S>,
) -> io::Result<Vec<packets::Packet>> {
    let message = peer
        .reliable
        .recv()
        .await
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "native client ended"))?;
    let DecodeOutcome::Packets { packets, .. } = codec.decode(message).map_err(io::Error::other)?
    else {
        return Err(io::Error::other("native client sent a dropped batch"));
    };
    packets
        .map(|bytes| {
            let mut reader = wire::Reader::from_shared(&bytes);
            let header =
                wire::PacketHeader::from_raw(reader.read_var_u32().map_err(io::Error::other)?);
            let id = packets::PacketId::from_raw(header.id)
                .ok_or_else(|| io::Error::other("unexpected client packet ID"))?;
            crate::codec::decode_body(id, &bytes.slice(reader.position()..))
                .map_err(io::Error::other)
        })
        .collect()
}

/// Drives the original empty-pack, real StartGame/radius and Initialized exchange.
async fn serve(peer: &mut AcceptedSession, case: Case) -> io::Result<()> {
    let mut limits = BatchLimits::default();
    if matches!(case, Case::LargeOffer | Case::LargeLive) {
        limits.max_decoded_bytes *= 2;
    }
    let codec = BatchCodec::new(BatchHeader::NetherNet, limits);
    if !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::RequestNetworkSettings(_))
    ) {
        return Err(io::Error::other("expected original settings request"));
    }
    send(
        peer,
        &codec,
        case,
        Stage::Settings,
        &[Packet::new(packets::NetworkSettings {
            compression_threshold: wire::U16LE(u16::MAX),
            ..Default::default()
        })],
    )
    .await?;
    let codec = codec.negotiate(CompressionSettings {
        algorithm: CompressionAlgorithm::Deflate,
        threshold: u16::MAX,
    });
    if !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::Login(_))
    ) {
        return Err(io::Error::other("expected original Login"));
    }
    send(
        peer,
        &codec,
        case,
        Stage::Offer,
        &[
            Packet::new(packets::PlayStatus {
                status: PlayStatusType::Loginsuccess,
            }),
            Packet::new(packets::ResourcePacksInfo::default()),
        ],
    )
    .await?;
    if !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::ClientCacheStatus(_))
    ) || !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::ResourcePackClientResponse(_))
    ) {
        return Err(io::Error::other("expected original pack acknowledgments"));
    }
    send(
        peer,
        &codec,
        case,
        Stage::Stack,
        &[
            Packet::new(packets::PlayStatus {
                status: PlayStatusType::Playerspawn,
            }),
            time(1),
            Packet::new(packets::ResourcePackStack::default()),
            Packet::new(packets::StartGame {
                runtime_id: ActorRuntimeID(42),
                ..Default::default()
            }),
            Packet::new(packets::ItemRegistry::default()),
            radius(),
            time(2),
        ],
    )
    .await?;
    if !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::ResourcePackClientResponse(_))
    ) || !matches!(
        receive(peer, &codec).await?.first(),
        Some(packets::Packet::RequestChunkRadius(_))
    ) {
        return Err(io::Error::other(
            "expected original completion and radius request",
        ));
    }
    let initialized = receive(peer, &codec).await?;
    if initialized
        .iter()
        .filter(|packet| {
            matches!(packet,
                packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42
            )
        })
        .count()
        != 1
    {
        return Err(io::Error::other("missing original Initialized receipt"));
    }
    if matches!(case, Case::LargeLive) {
        send(
            peer,
            &codec,
            case,
            Stage::Live,
            &[
                time(3),
                Packet::new(packets::ScriptMessage {
                    message_id: LIVE_SCRIPT_CHANNEL.into(),
                    message_value: vec![LIVE_SCRIPT_BYTE; live_script_len()].into(),
                }),
                time(4),
            ],
        )
        .await?;
    }
    Ok(())
}

const LIVE_SCRIPT_CHANNEL: &str = "cinnabar:compatibility-fixture";
const LIVE_SCRIPT_BYTE: u8 = b'x';

/// Exceeds the original body ceiling with one valid generated byte-buffer field.
fn live_script_len() -> usize {
    BatchLimits::default().max_decoded_bytes + 1
}

/// Consumes each actual packet view before awaiting the next body on the same play transport.
async fn receive_live(play: &mut crate::PlaySession) -> Result<(), ProtocolError> {
    let mut positions = [(0, 0); 3];
    for (index, position) in positions.iter_mut().enumerate() {
        let crate::session_transport::Inbound::Packet(raw) = play.transport.recv().await? else {
            return Err(super::super::failure(io::Error::other(
                "live fixture ended before its original packet sequence",
            )));
        };
        *position = raw.trace_position;
        let (header, packet) = raw.decode_parts()?;
        let primary = header.from_subclient == 0 && header.to_subclient == 0;
        let valid = match (index, packet) {
            (0, packets::Packet::SetTime(value)) => value.time.0 == 3,
            (1, packets::Packet::ScriptMessage(value)) => {
                value.message_id.to_str().ok() == Some(LIVE_SCRIPT_CHANNEL)
                    && value.message_value.len() == live_script_len()
                    && value
                        .message_value
                        .iter()
                        .all(|byte| *byte == LIVE_SCRIPT_BYTE)
            }
            (2, packets::Packet::SetTime(value)) => value.time.0 == 4,
            _ => false,
        };
        if !primary || !valid {
            return Err(super::super::failure(io::Error::other(
                "original live packet header, order or generated body changed",
            )));
        }
    }
    if positions[1].1 != 0 || positions[0].0 == positions[1].0 || positions[1].0 == positions[2].0 {
        return Err(super::super::failure(io::Error::other(
            "the large live packet did not occupy its own complete body",
        )));
    }
    Ok(())
}

/// Original generated FIFO markers distinguish skipped input from lost or replayed siblings.
fn time(value: i32) -> Packet {
    Packet::new(packets::SetTime {
        time: wire::ZigZag32(value),
    })
}

/// Uses the same original configured spawn response as the existing real-peer fixture.
fn radius() -> Packet {
    Packet::new(packets::ChunkRadiusUpdated {
        chunk_radius: wire::ZigZag32(8),
    })
}

/// Returns protocol results only after the original play, accepted peer and listener are retired.
async fn exercise(case: Case) -> (Result<(), ProtocolError>, io::Result<()>) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = fixture_route(Route::Lan).await;
        let mut listener = fixture.listener;
        let (rejected, mut rejection) = tokio::sync::oneshot::channel();
        let (delivered, delivery) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = listener.accept().await.map_err(io::Error::other)?;
            let result = tokio::select! { biased;
                result = async {
                    serve(&mut peer, case).await?;
                    if matches!(case, Case::LargeLive) {
                        // Keep the original peer alive until the client finishes its actual read.
                        delivery.await.map_err(|_| io::Error::other("live packet read was dropped"))?;
                    }
                    Ok(())
                } => result,
                Ok(()) = &mut rejection => Err(io::Error::other("client rejected the actual startup input")),
            };
            let retired = peer.close();
            retired.session.closed().await;
            drop(retired);
            result
        };
        let client = async {
            let settings = super::super::super::LoginSettings::default();
            let result = async {
                let (mut play, runtime_id) = LoginSequence::connect_native(
                    &fixture.target,
                    SessionLogin {
                        display_name: "Fixture",
                        client_random_id: crate::ClientRandomId::new(1).unwrap(),
                        cache: None,
                        skin: None,
                        settings: &settings,
                        tap: None,
                        #[cfg(not(target_arch = "wasm32"))]
                        relay: None,
                        target: None,
                    },
                    |_, game| game.world.local_player_runtime_id,
                ).await?;
                let received = async {
                    let mut packets = Vec::new();
                    for _ in 0..3 {
                        packets.push(play.recv().await?);
                    }
                    play.finish_loading().await?;
                    if matches!(case, Case::LargeLive) {
                        let received = receive_live(&mut play).await;
                        let _ = delivered.send(());
                        received?;
                    }
                    Ok::<_, ProtocolError>((runtime_id, packets))
                }.await;
                let closed = play.close().await;
                let (runtime_id, packets) = received?;
                closed?;
                if runtime_id != 42 || packets != [time(1), radius(), time(2)] {
                    return Err(super::super::failure(io::Error::other("original startup FIFO changed")));
                }
                Ok(())
            }.await;
            if result.is_err() {
                let _ = rejected.send(());
            }
            result
        };
        let (server, client) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        (client, server)
    }).await.expect("finite original native startup fixture stalled")
}

#[tokio::test]
async fn native_startup_unknown_before_settings_keeps_original_join() {
    let (client, server) = exercise(Case::Settings).await;
    client.expect("well-formed unknown before NetworkSettings stopped the original join");
    server.unwrap();
}

#[tokio::test]
async fn native_startup_unknown_before_offer_keeps_original_join() {
    let (client, server) = exercise(Case::Offer).await;
    client.expect("well-formed unknown before ResourcePacksInfo stopped the original join");
    server.unwrap();
}

#[tokio::test]
async fn native_startup_unknown_during_packs_keeps_original_join() {
    let (client, server) = exercise(Case::Stack).await;
    client.expect("well-formed unknown before ResourcePackStack stopped the original join");
    server.unwrap();
}

#[tokio::test]
async fn native_startup_healthy_peer_keeps_original_join() {
    let (client, server) = exercise(Case::Healthy).await;
    client.unwrap();
    server.unwrap();
}

#[tokio::test]
async fn native_startup_compressed_upstream_batch_above_default_bytes_keeps_original_join() {
    let (client, server) = exercise(Case::LargeOffer).await;
    client.expect(
        "valid compressed upstream batch above the default decoded limit stopped the original join",
    );
    server.unwrap();
}

#[tokio::test]
async fn native_live_generated_packet_above_default_bytes_preserves_complete_delivery() {
    let (client, server) = exercise(Case::LargeLive).await;
    client.expect("one complete generated live packet above the old body ceiling was rejected");
    server.unwrap();
}

/// Both malformed controls must retain the original generated wire error, not an unknown-ID skip.
fn wire_failure(error: ProtocolError) -> bool {
    matches!(error, ProtocolError::Session(crate::SessionError(
        crate::codec::SessionFailure::NativeTransport(source)
    )) if source.get_ref().is_some_and(|error| error.is::<wire::DecodeError>()))
}

#[tokio::test]
async fn native_startup_malformed_header_still_fails() {
    let (client, _) = exercise(Case::MalformedHeader).await;
    assert!(wire_failure(
        client.expect_err("malformed header was accepted")
    ));
}

#[tokio::test]
async fn native_startup_malformed_known_settings_still_fails() {
    let (client, _) = exercise(Case::MalformedSettings).await;
    assert!(wire_failure(
        client.expect_err("malformed known NetworkSettings was accepted")
    ));
}
