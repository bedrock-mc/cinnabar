use super::*;
use crate::login::native_network::transport::{
    self,
    early_spawn_tests::{receive, send},
};
use crate::{NativeConnectionTarget, NativeTransport};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID};
use nethernet_tokio::{ConnectionConfig, LanConfig, LanSignaling, NetherServer, ServerData};
use std::time::Duration;

mod retention;

/// Uses a real native Login peer to prove the shared prefix and browser wire boundary; no ready owner is forged.
/// Exact gateway RakNet admission and application account/controller authority are separate qualification gates.
#[tokio::test]
async fn relay_handoff_preserves_raw_startup_siblings_and_consuming_close() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            41,
            address,
            LanConfig {
                discovery_port: address.port(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut data = ServerData::new("Fixture".into(), "World".into());
        data.flag_b = true;
        data.session_id = "synthetic-nonce".into();
        data.connection_type = transport::LAN_CONNECTION_TYPE;
        data.protocol_version = crate::PROTOCOL_VERSION as u32;
        signaling.set_server_data(data);
        let mut listener = NetherServer::bind_with(
            signaling,
            ConnectionConfig {
                allow_unsigned_answers: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut spawn = crate::Packet::new(packets::PlayStatus {
            status: PlayStatusType::Playerspawn,
        });
        spawn.header.from_subclient = 1;
        spawn.header.to_subclient = 2;
        let before = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(1),
        });
        let mut start = crate::Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        });
        start.header.from_subclient = 2;
        start.header.to_subclient = 1;
        let after = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(2),
        });
        let retired = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(3),
        });
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::RequestNetworkSettings(_)
            ));
            send(
                &peer,
                &codec,
                &[crate::Packet::new(packets::NetworkSettings {
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
                Packet::Login(_)
            ));
            send(
                &peer,
                &codec,
                &[
                    crate::Packet::new(packets::PlayStatus {
                        status: PlayStatusType::Loginsuccess,
                    }),
                    crate::Packet::new(packets::ResourcePacksInfo::default()),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ClientCacheStatus(_)
            ));
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ResourcePackClientResponse(_)
            ));
            send(
                &peer,
                &codec,
                &[
                    spawn.clone(),
                    before.clone(),
                    crate::Packet::new(packets::ResourcePackStack::default()),
                    start.clone(),
                    after.clone(),
                    retired.clone(),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ResourcePackClientResponse(_)
            ));
            peer.session.closed().await;
        };
        let client = async {
            let target =
                NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan);
            let connected = transport::Transport::connect(
                &target,
                None,
                None,
                tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
            )
            .await
            .unwrap();
            let mut joining =
                NativeRelayConnecting::from_connection(Connection::new(connected), target.address);
            let random = crate::ClientRandomId::new(1).unwrap();
            let claims =
                crate::login_client_data("Fixture", random, None, &crate::LoginSettings::default());
            joining
                .negotiate(
                    crate::RawClientData::from_value(&claims).unwrap(),
                    false,
                    None,
                )
                .await
                .unwrap();
            let (mut connected, handoff) = joining
                .try_finish()
                .unwrap_or_else(|_| panic!("actual raw StartGame completed the handoff"));
            assert!(handoff.archives.is_empty());
            let frame =
                crate::session_wire::encode_core_message(&CoreMessage::Handoff(handoff.handoff))
                    .unwrap();
            let (handoff, archives, _) = crate::LoginSequence::receive_web_handoff(
                futures::stream::iter([Ok::<_, bridge::BridgeError>(frame)]),
                1,
                async { Ok::<_, crate::ProtocolError>(None::<tokio::io::Empty>) },
                bridge::tasks::Timers::tokio(),
            )
            .await
            .unwrap();
            assert!(archives.is_empty());
            let originals = [spawn.clone(), before.clone(), start.clone()];
            let body = crate::session_transport::batch_body(&originals).unwrap();
            let expected = wire::Frames::new(body, BatchLimits::default().max_packets)
                .map(|entry| {
                    let entry = entry.unwrap();
                    let mut reader = wire::Reader::from_shared(entry.frame());
                    reader.read_var_u32().unwrap();
                    entry.frame().slice(reader.position()..)
                })
                .collect::<Vec<_>>();
            assert_eq!(
                handoff.startup, expected,
                "original headers and packet FIFO must survive packs and browser handoff"
            );
            let CoreMessage::Batch(packets) = connected.recv().await.unwrap() else {
                panic!("first post-handoff sibling")
            };
            assert_eq!(packets.len(), 1);
            assert_eq!(decode(packets.into_iter().next().unwrap()), after);
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
            let CoreMessage::Batch(packets) = closing.recv().await.unwrap() else {
                panic!("accepted sibling survives consuming close")
            };
            assert_eq!(packets.len(), 1);
            assert_eq!(decode(packets.into_iter().next().unwrap()), retired);
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("real native relay handoff fixture stalled");
}

/// Reframes one original packet only for the existing generated decoder assertion.
fn decode(raw: Bytes) -> crate::Packet {
    let mut body = Vec::new();
    wire::Writer::with_limit(&mut body, BatchLimits::default().max_decoded_bytes)
        .write_len_var_u32(raw.len())
        .unwrap();
    body.extend_from_slice(&raw);
    crate::raw_packet::decode_batch_body(
        Bytes::from(body),
        BatchLimits::default().max_decoded_bytes,
    )
    .unwrap()
    .pop()
    .unwrap()
    .decode()
    .unwrap()
}

mod consumed;

mod composite;

mod cursor;

mod cdn;
