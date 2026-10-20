//! Measures actual coalesced socket-batch ownership through the existing native relay handoff.

use super::*;
use std::sync::{Arc, Mutex};

/// A tiny retained startup frame must not retain a discarded sibling's whole decoded batch.
#[tokio::test]
async fn tiny_relay_startup_detaches_from_discarded_batch_backing() {
    tokio::time::timeout(Duration::from_secs(20), async {
        const PADDING_BYTES: usize = 16 * 1024;
        let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            43,
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
        let tiny = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(1),
        });
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
            // A complete but unusable Transfer is skipped, leaving only tiny to retain its batch.
            // This small fixture stays below the compression threshold and all ordinary bounds.
            send(
                &peer,
                &codec,
                &[
                    tiny.clone(),
                    crate::Packet::new(packets::Transfer {
                        server_address: "x".repeat(PADDING_BYTES).into(),
                        server_port: wire::U16LE(0),
                        ..Default::default()
                    }),
                ],
            )
            .await;
            send(&peer, &codec, &[start.clone(), after.clone()]).await;
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
            assert!(
                joining.startup_bytes < PADDING_BYTES,
                "the owning byte charge contains only actual retained startup frames"
            );
            let (mut connected, mut handoff) = joining
                .try_finish()
                .unwrap_or_else(|_| panic!("actual StartGame handoff"));
            assert_eq!(handoff.handoff.startup.len(), 2);
            assert_eq!(decode(handoff.handoff.startup[0].clone()), tiny);
            assert_eq!(decode(handoff.handoff.startup[1].clone()), start);
            let (large, last) = {
                let observed = observations.lock().unwrap();
                assert_eq!(
                    observed
                        .iter()
                        .filter(|(length, _)| *length > PADDING_BYTES)
                        .count(),
                    1,
                    "one actual coalesced socket batch supplies the larger backing"
                );
                (
                    observed
                        .iter()
                        .find(|(length, _)| *length > PADDING_BYTES)
                        .unwrap()
                        .1
                        .clone(),
                    observed.last().unwrap().1.clone(),
                )
            };
            assert!(
                last.upgrade().is_some(),
                "the ordinary unread sibling still owns its actual received allocation"
            );
            let CoreMessage::Batch(packets) = connected.recv().await.unwrap() else {
                panic!("actual post-handoff sibling")
            };
            assert_eq!(packets.len(), 1);
            assert_eq!(decode(packets.into_iter().next().unwrap()), after);
            // Retire the distinct small backing while keeping the original tiny startup frame.
            drop(handoff.handoff.startup.pop());
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
            drop(closing);
            assert!(
                last.upgrade().is_none(),
                "weak observations must not retain ordinary consumed socket input"
            );
            assert!(
                large.upgrade().is_none(),
                "tiny retained relay startup must release discarded sibling batch backing"
            );
            assert_eq!(
                decode(handoff.handoff.startup[0].clone()),
                tiny,
                "detaching storage preserves the original packet bytes"
            );
            drop(handoff);
            assert!(large.upgrade().is_none());
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("actual relay backing fixture stalled");
}
