//! Unrelated startup bodies retain their raw owner until a consumer actually uses them.

use super::early_spawn_tests::{FixtureRoute, Route, fixture_route, receive, send};
use crate::packet::GameHeader;
use crate::packet_relay::{PacketRelay, RelayOutput};
use crate::session_transport::{Inbound, tests::packet_bytes};
use crate::session_wire::SessionIdentity;
use crate::{LoginSequence, Packet, ProtocolError, SessionLogin};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID, wire};
use bytes::Bytes;
use std::{sync::Arc, time::Duration};

struct Passthrough;

impl PacketRelay for Passthrough {
    fn start(&self, _: &SessionIdentity, _: &str, _: &[u8]) -> Vec<bedrock_packs::RelayPack> {
        Vec::new()
    }

    fn inbound(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn outbound(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn tick(&self) -> RelayOutput {
        RelayOutput::default()
    }

    fn stop(&self) {}
}

/// Frames a defined packet ID without pretending its unused body was decoded.
fn unused_entry() -> Bytes {
    let mut header = wire::PacketHeader::new(packets::PacketId::ShowStoreOffer as u32);
    header.sender_subclient = 2;
    header.target_subclient = 3;
    let mut entry = Vec::new();
    wire::Writer::with_limit(&mut entry, BatchLimits::default().max_decoded_bytes)
        .write_var_u32(header.to_raw())
        .unwrap();
    entry.into()
}

/// Encodes an ordinary generated sibling through the original codec.
fn time(value: i32) -> Packet {
    Packet::new(packets::SetTime {
        time: wire::ZigZag32(value),
    })
}

#[tokio::test]
async fn native_relay_preserves_unrelated_known_raw_body_before_late_start_game() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let unused = unused_entry();
        let originals = [
            packet_bytes(&time(1)),
            unused.clone(),
            packet_bytes(&time(2)),
            packet_bytes(&time(3)),
        ];
        // This only observes whether the PUBLIC join returned, so either result can clean up.
        let (joined, completed) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::RequestNetworkSettings(_)
            ));
            send(
                &peer,
                &codec,
                &[Packet::new(packets::NetworkSettings {
                    compression_threshold: wire::U16LE(u16::MAX),
                    ..Default::default()
                })],
            )
            .await;
            let codec = codec.negotiate(CompressionSettings {
                algorithm: CompressionAlgorithm::Deflate,
                threshold: u16::MAX,
            });
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::Login(_)
            ));
            send(
                &peer,
                &codec,
                &[
                    Packet::new(packets::PlayStatus {
                        status: PlayStatusType::Loginsuccess,
                    }),
                    Packet::new(packets::ResourcePacksInfo::default()),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ClientCacheStatus(_)
            ));
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            send(
                &peer,
                &codec,
                &[Packet::new(packets::ResourcePackStack::default())],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            let entries = [
                originals[0].clone(),
                unused.clone(),
                originals[2].clone(),
                packet_bytes(&Packet::new(packets::StartGame {
                    runtime_id: ActorRuntimeID(42),
                    ..Default::default()
                })),
                packet_bytes(&Packet::new(packets::ItemRegistry::default())),
                packet_bytes(&Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Playerspawn,
                })),
                originals[3].clone(),
            ];
            for message in codec.encode(&entries).unwrap() {
                peer.session.send(message).await.unwrap();
            }
            let initialized = if completed.await.is_ok() {
                assert!(matches!(
                    &*receive(&mut peer, &codec).await[0].data,
                    packets::Packet::RequestChunkRadius(_)
                ));
                receive(&mut peer, &codec).await.into_iter().filter(|packet| matches!(
                    &*packet.data,
                    packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42
                )).count()
            } else {
                0
            };
            peer.session.closed().await;
            initialized
        };
        let client = async {
            let settings = crate::LoginSettings::default();
            let connected = LoginSequence::connect_native(
                &target,
                SessionLogin {
                    display_name: "Fixture",
                    client_random_id: crate::ClientRandomId::new(1).unwrap(),
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    relay: Some(Arc::new(Passthrough)),
                    target: None,
                },
                |_, game| game.world.local_player_runtime_id,
            )
            .await;
            let (mut play, runtime) = match connected {
                Ok(connected) => connected,
                Err(error) => {
                    drop(joined);
                    return Err(error);
                }
            };
            joined.send(()).unwrap();
            let observed: Result<_, ProtocolError> = async {
                let mut raw = Vec::new();
                for _ in 0..originals.len() {
                    let Inbound::Packet(packet) = play.transport.recv().await? else {
                        return Err(crate::login::native_network::failure(
                            std::io::Error::other("raw startup prefix ended"),
                        ));
                    };
                    raw.push((
                        packet.header,
                        packet.inner_frame().clone(),
                        packet.body().clone(),
                    ));
                }
                play.finish_loading().await?;
                Ok((runtime, raw))
            }
            .await;
            let closed = play.close().await;
            drop(play);
            closed?;
            observed
        };
        let (initialized, observed) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        let (runtime, raw) = observed
            .expect("relay startup eagerly decoded a known body unused by the startup owner");
        assert_eq!(runtime, 42);
        assert_eq!(initialized, 1);
        assert_eq!(
            raw.iter()
                .map(|(_, frame, _)| frame.clone())
                .collect::<Vec<_>>(),
            originals
                .iter()
                .map(|entry| crate::packet_relay::batch_body(std::slice::from_ref(entry)))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            raw[1].0,
            GameHeader {
                id: packets::PacketId::ShowStoreOffer,
                from_subclient: 2,
                to_subclient: 3
            }
        );
        assert!(
            raw[1].2.is_empty(),
            "the unused original body must remain opaque"
        );
    })
    .await
    .expect("finite native relay raw-prefix fixture stalled");
}
