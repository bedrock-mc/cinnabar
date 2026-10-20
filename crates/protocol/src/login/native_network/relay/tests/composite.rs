//! Valid original network batches must retain their order across bounded pump reframing.

use super::*;

/// Deferred and unread sibling batches are split only at original per-body limits, without loss.
#[tokio::test]
async fn deferred_and_unread_siblings_reframe_within_the_original_packet_bound() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            47,
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
        let limits = BatchLimits::default();
        let per_batch = limits.max_packets / 2 + 1;
        let total = per_batch * 2;
        let times = (0..total)
            .map(|index| {
                let mut packet = crate::Packet::new(packets::SetTime {
                    time: wire::ZigZag32(i32::try_from(index).unwrap()),
                });
                packet.header.from_subclient = u32::try_from(index % 4).unwrap();
                packet.header.to_subclient = u32::try_from(3 - index % 4).unwrap();
                packet
            })
            .collect::<Vec<_>>();
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
            let mut deferred = vec![start.clone()];
            deferred.extend_from_slice(&times[..per_batch]);
            let mut unread = vec![crate::Packet::new(packets::ResourcePackStack::default())];
            unread.extend_from_slice(&times[per_batch..]);
            assert!(deferred.len() <= limits.max_packets);
            assert!(unread.len() <= limits.max_packets);
            send(&peer, &codec, &deferred).await;
            send(&peer, &codec, &unread).await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ResourcePackClientResponse(_)
            ));
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
            let connection = Connection::new(transport);
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
            let mut delivered = 0;
            while delivered < total {
                let body = connected.connection.recv_body().await.unwrap();
                assert!(
                    body.len() <= limits.max_decoded_bytes,
                    "internal pump body exceeded its decoded byte bound"
                );
                assert!(!body.is_empty());
                for entry in wire::Frames::new(body, limits.max_packets) {
                    let entry = entry
                        .expect("valid deferred siblings exceeded internal pump framing bounds");
                    let header = entry.header();
                    assert_eq!(header.id, packets::PacketId::SetTime as u32);
                    assert_eq!(
                        u32::from(header.sender_subclient),
                        times[delivered].header.from_subclient
                    );
                    assert_eq!(
                        u32::from(header.target_subclient),
                        times[delivered].header.to_subclient
                    );
                    let bytes = entry.body();
                    let mut reader = wire::Reader::from_shared(&bytes);
                    let Packet::SetTime(packet) =
                        Packet::decode_body(packets::PacketId::SetTime, &mut reader).unwrap()
                    else {
                        panic!("original SetTime")
                    };
                    assert_eq!(packet.time.0, i32::try_from(delivered).unwrap());
                    reader.expect_consumed().unwrap();
                    delivered += 1;
                }
            }
            assert_eq!(delivered, total);
            drop(handoff);
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
            drop(closing);
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("actual deferred sibling reframe fixture stalled");
}
