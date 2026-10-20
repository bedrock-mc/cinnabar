//! Measures the actual consumed socket-batch backing through the native relay handoff.

use super::*;
use std::sync::{Arc, Mutex};

/// Handoff ownership releases the last consumed projection while preserving unread input.
#[tokio::test]
async fn completed_handoff_releases_the_consumed_start_game_backing() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            45,
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
        let start = crate::Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        });
        let after = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(2),
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
                &[crate::Packet::new(packets::ResourcePackStack::default())],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ResourcePackClientResponse(_)
            ));
            // Distinct application batches give the consumed projection and unread sibling
            // independent actual received backing allocations.
            send(&peer, &codec, std::slice::from_ref(&start)).await;
            send(&peer, &codec, std::slice::from_ref(&after)).await;
            peer.session.closed().await;
        };
        let client = async {
            let target =
                NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan);
            let transport = transport::Transport::connect(
                &target,
                None,
                None,
                tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
            )
            .await
            .unwrap();
            let observations = Arc::new(Mutex::new(Vec::new()));
            let mut connection = Connection::new(transport);
            connection.observe_batches(Arc::clone(&observations));
            let mut joining = NativeRelayConnecting::from_connection(connection, target.address);
            let claims = crate::login_client_data(
                "Fixture",
                crate::ClientRandomId::new(1).unwrap(),
                None,
                &crate::LoginSettings::default(),
            );
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
                .unwrap_or_else(|_| panic!("actual StartGame handoff"));
            assert_eq!(handoff.handoff.startup.len(), 1);
            assert_eq!(decode(handoff.handoff.startup[0].clone()), start);
            let consumed = observations.lock().unwrap().last().unwrap().1.clone();
            drop(handoff);
            assert!(
                consumed.upgrade().is_none(),
                "completed handoff retained its already consumed StartGame projection"
            );
            let CoreMessage::Batch(packets) = connected.recv().await.unwrap() else {
                panic!("unread post-handoff sibling must remain on its original FIFO")
            };
            assert_eq!(packets.len(), 1);
            assert_eq!(decode(packets.into_iter().next().unwrap()), after);
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
            drop(closing);
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("actual consumed StartGame backing fixture stalled");
}
