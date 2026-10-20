//! The relay observes full native packet headers and the actual prefix through first StartGame.

use super::early_spawn_tests::{FixtureRoute, Route, fixture_route, receive, send};
use crate::login::native_network::negotiated;
use crate::packet_relay::{PacketRelay, RelayOutput};
use crate::session_transport::{Inbound, SessionTransport};
use crate::session_wire::SessionIdentity;
use crate::{LoginSequence, Packet, SessionLogin};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings, Negotiated,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID, wire};
use nethernet_tokio::AcceptedSession;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct RecordingRelay {
    starts: Mutex<Vec<Vec<u8>>>,
    inbound: Mutex<Vec<Vec<u8>>>,
}

impl PacketRelay for RecordingRelay {
    fn start(&self, _: &SessionIdentity, _: &str, startup: &[u8]) -> Vec<bedrock_packs::RelayPack> {
        self.starts.lock().unwrap().push(startup.to_vec());
        Vec::new()
    }

    fn inbound(&self, body: &[u8]) -> Option<Vec<u8>> {
        self.inbound.lock().unwrap().push(body.to_vec());
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

/// Supplies one legal nonzero-subclient entry through the actual generated encoder.
fn marker(time: i32) -> Packet {
    let mut packet = Packet::new(packets::SetTime {
        time: wire::ZigZag32(time),
    });
    packet.header.from_subclient = 2;
    packet.header.to_subclient = 3;
    packet
}

/// Completes the real Settings/Login/packs exchange, including the client's stack acknowledgement.
async fn negotiate_packs(
    peer: &mut AcceptedSession,
    deferred: &[Packet],
) -> BatchCodec<Negotiated> {
    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
    assert!(matches!(
        &*receive(peer, &codec).await[0].data,
        packets::Packet::RequestNetworkSettings(_)
    ));
    send(
        peer,
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
        &*receive(peer, &codec).await[0].data,
        packets::Packet::Login(_)
    ));
    send(
        peer,
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
        &*receive(peer, &codec).await[0].data,
        packets::Packet::ClientCacheStatus(_)
    ));
    assert!(matches!(
        &*receive(peer, &codec).await[0].data,
        packets::Packet::ResourcePackClientResponse(_)
    ));
    let mut stack = deferred.to_vec();
    stack.push(Packet::new(packets::ResourcePackStack::default()));
    send(peer, &codec, &stack).await;
    assert!(matches!(
        &*receive(peer, &codec).await[0].data,
        packets::Packet::ResourcePackClientResponse(_)
    ));
    codec
}

/// Uses the original generated packet framing for callback and replay byte comparisons.
fn batch(packets: &[Packet]) -> Vec<u8> {
    crate::session_transport::batch_body(packets)
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn native_relay_adopts_negotiated_deferred_packet_with_original_subclient_header() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let original = marker(1);
        let relay = Arc::new(RecordingRelay::default());
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            negotiate_packs(&mut peer, std::slice::from_ref(&original)).await;
            peer.session.closed().await;
        };
        let client = async {
            let settings = crate::LoginSettings::default();
            let login = SessionLogin {
                display_name: "Fixture",
                client_random_id: crate::ClientRandomId::new(1).unwrap(),
                cache: None,
                skin: None,
                settings: &settings,
                tap: None,
                relay: None,
                target: None,
            };
            let negotiated = negotiated::negotiate(
                &target,
                &login,
                None,
                tokio::time::Instant::now() + Duration::from_secs(20),
            )
            .await
            .unwrap();
            let mut connection = negotiated.connection;
            let deferred = connection.take_deferred();
            let deferred_count = deferred.len();
            let mut transport = SessionTransport::from_native(connection);
            transport.set_relay(relay.clone());
            transport.adopt_startup(deferred);
            let received = match transport.recv().await {
                Ok(Inbound::Packet(raw)) => raw.decode().map(Some),
                Ok(Inbound::Terminal(_)) => Ok(None),
                Err(error) => Err(error),
            };
            let closed = transport.close().await;
            drop(transport);
            (deferred_count, received, closed)
        };
        let ((), (deferred_count, received, closed)) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        closed.unwrap();
        assert_eq!(
            deferred_count, 1,
            "the real pack consumer must defer the original marker"
        );
        assert!(
            matches!(&received, Ok(Some(packet)) if packet == &original),
            "native relay replay lost the original packet header or payload: {received:?}"
        );
        assert_eq!(*relay.inbound.lock().unwrap(), vec![batch(&[original])]);
    })
    .await
    .expect("finite native relay header fixture stalled");
}

#[tokio::test]
async fn native_relay_start_observes_late_first_start_game_before_original_ordered_replay() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let first = marker(1);
        let start = Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        });
        let radius = Packet::new(packets::ChunkRadiusUpdated {
            chunk_radius: wire::ZigZag32(8),
        });
        let last = marker(2);
        let startup = vec![
            first.clone(),
            start,
            Packet::new(packets::ItemRegistry::default()),
            radius.clone(),
            Packet::new(packets::PlayStatus {
                status: PlayStatusType::Playerspawn,
            }),
            last.clone(),
        ];
        let relay = Arc::new(RecordingRelay::default());
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = negotiate_packs(&mut peer, &[]).await;
            send(&peer, &codec, &startup).await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::RequestChunkRadius(_)
            ));
            let initialized = receive(&mut peer, &codec).await;
            let initialized_count = initialized.iter().filter(|packet| matches!(
                &*packet.data,
                packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42
            )).count();
            peer.session.closed().await;
            initialized_count
        };
        let client = async {
            let settings = crate::LoginSettings::default();
            let (mut play, runtime_id) = LoginSequence::connect_native(
                &target,
                SessionLogin {
                    display_name: "Fixture",
                    client_random_id: crate::ClientRandomId::new(1).unwrap(),
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    relay: Some(relay.clone()),
                    target: None,
                },
                |_, game| game.world.local_player_runtime_id,
            )
            .await
            .unwrap();
            let mut received = Vec::new();
            for _ in 0..3 {
                received.push(play.recv().await.unwrap());
            }
            play.finish_loading().await.unwrap();
            play.finish_loading().await.unwrap();
            let closed = play.close().await;
            drop(play);
            (runtime_id, received, closed)
        };
        let (initialized_count, (runtime_id, received, closed)) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        closed.unwrap();
        assert_eq!(runtime_id, 42);
        assert_eq!(initialized_count, 1);
        assert_eq!(received, vec![first.clone(), radius, last]);
        let replayed = relay.inbound.lock().unwrap().concat();
        let packets = crate::raw_packet::decode_batch_body(
            replayed.into(),
            crate::session_transport::MAX_SESSION_BATCH_BYTES,
        )
        .unwrap()
        .into_iter()
        .map(crate::raw_packet::RawPacket::decode)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(
            packets, startup,
            "each actual startup entry must traverse the relay once in original order"
        );
        assert_eq!(
            *relay.starts.lock().unwrap(),
            vec![batch(&startup[..2])],
            "the native relay began before its actual prefix through first StartGame"
        );
    })
    .await
    .expect("finite native relay callback fixture stalled");
}
