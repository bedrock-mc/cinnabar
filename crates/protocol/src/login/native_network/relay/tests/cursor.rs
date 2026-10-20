//! A single original oversized-count batch spans actual packs and raw handoff.

use super::*;
use crate::login::native_network::transport::early_spawn_tests::{
    FixtureRoute, Route, fixture_route,
};
use crate::session_transport::tests::packet_bytes;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn filtered_cursor_preserves_accepted_prefix_before_a_later_invalid_header() {
    use crate::login::native_network::transport::early_spawn_tests::{
        FixtureRoute, Route, fixture_route,
    };
    use crate::session_transport::tests::packet_bytes;

    let expected = crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(7),
    });
    let ((messages, sent), (first, second, closed), listener_closed) =
        tokio::time::timeout(Duration::from_secs(20), async {
            let FixtureRoute {
                mut listener,
                target,
                ..
            } = fixture_route(Route::Lan).await;
            let server = async {
                let peer = listener.accept().await.unwrap();
                let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
                let raw = [packet_bytes(&expected), Bytes::from_static(&[0x80])];
                let messages = codec.encode(&raw).unwrap();
                let count = messages.len();
                let mut sent = Ok(());
                for message in messages {
                    if let Err(error) = peer.session.send(message).await {
                        sent = Err(error);
                        break;
                    }
                }
                peer.session.closed().await;
                drop(peer);
                (count, sent)
            };
            let client = async {
                let transport = transport::Transport::connect(
                    &target,
                    None,
                    None,
                    tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
                )
                .await
                .unwrap();
                let mut connection = Connection::new(transport);
                connection.primary_only();
                let first = connection.recv_body().await;
                let second = connection.recv_body().await;
                let closed = connection.close().await;
                drop(connection);
                (first, second, closed)
            };
            let (server, client) = tokio::join!(server, client);
            let listener_closed = listener.close().await;
            drop(listener);
            (server, client, listener_closed)
        })
        .await
        .expect("finite actual filtered cursor prefix fixture stalled");

    sent.unwrap();
    closed.unwrap();
    listener_closed.unwrap();
    assert_eq!(messages, 1, "fixture split the original packet batch");
    assert_eq!(
        first.expect("a later malformed header discarded the original valid prefix"),
        crate::session_transport::batch_body(std::slice::from_ref(&expected)).unwrap()
    );
    let error = second.expect_err("later malformed header was lost");
    assert!(
        error
            .get_ref()
            .is_some_and(|error| error.is::<wire::DecodeError>()),
        "later malformed header lost its original wire decode cause: {error}"
    );
}

#[tokio::test]
async fn one_incoming_batch_cursor_survives_pack_replay_and_bounded_handoff_reads() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let limits = BatchLimits::default();
        let count = limits.max_packets + 1;
        let split = count / 2;
        let times = (0..count)
            .map(|index| {
                let mut packet = crate::Packet::new(packets::SetTime {
                    time: wire::ZigZag32(i32::try_from(index).unwrap()),
                });
                packet.header.from_subclient = u32::try_from(index % 4).unwrap();
                packet.header.to_subclient = u32::try_from(3 - index % 4).unwrap();
                packet
            })
            .collect::<Vec<_>>();
        let start = crate::Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        });
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, limits);
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::RequestNetworkSettings(_)
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
                packets::Packet::Login(_)
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
                packets::Packet::ClientCacheStatus(_)
            ));
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));

            let mut original = vec![start.clone()];
            original.extend_from_slice(&times[..split]);
            original.push(crate::Packet::new(packets::ResourcePackStack::default()));
            original.extend_from_slice(&times[split..]);
            let mut messages = BatchCodec::new(
                BatchHeader::NetherNet,
                BatchLimits {
                    max_packets: original.len(),
                    ..limits
                },
            )
            .negotiate(CompressionSettings {
                algorithm: CompressionAlgorithm::Deflate,
                threshold: u16::MAX,
            })
            .encode(&original.iter().map(packet_bytes).collect::<Vec<_>>())
            .unwrap();
            assert_eq!(
                messages.len(),
                1,
                "fixture split its original incoming batch"
            );
            assert_eq!(
                messages[0][0],
                u8::MAX,
                "lifetime observation needs the original uncompressed allocation"
            );
            peer.session.send(messages.remove(0)).await.unwrap();
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            peer.session.closed().await;
        };
        let client = async {
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
                .unwrap_or_else(|_| panic!("actual original StartGame handoff"));
            assert_eq!(handoff.handoff.startup.len(), 1);
            assert_eq!(decode(handoff.handoff.startup[0].clone()), start);
            let original = observations.lock().unwrap().last().unwrap().1.clone();
            let mut delivered = 0;
            let mut bodies = 0;
            while delivered < count {
                let body = connected.connection.recv_body().await.unwrap();
                assert!(!body.is_empty());
                assert!(body.len() <= limits.max_decoded_bytes);
                let mut packets = 0;
                for frame in wire::Frames::new(body, limits.max_packets) {
                    let frame = frame.expect("cursor exceeded the original pump count ceiling");
                    assert_eq!(
                        u32::from(frame.header().sender_subclient),
                        times[delivered].header.from_subclient
                    );
                    assert_eq!(
                        u32::from(frame.header().target_subclient),
                        times[delivered].header.to_subclient
                    );
                    assert_eq!(frame.header().id, packets::PacketId::SetTime as u32);
                    let body = frame.body();
                    let mut reader = wire::Reader::from_shared(&body);
                    let packets::Packet::SetTime(time) =
                        packets::Packet::decode_body(packets::PacketId::SetTime, &mut reader)
                            .unwrap()
                    else {
                        panic!("original SetTime")
                    };
                    assert_eq!(time.time.0, i32::try_from(delivered).unwrap());
                    reader.expect_consumed().unwrap();
                    delivered += 1;
                    packets += 1;
                }
                assert!(packets <= limits.max_packets);
                bodies += 1;
            }
            assert_eq!(delivered, count);
            assert_eq!(bodies, 2);
            drop(handoff);
            assert!(
                original.upgrade().is_none(),
                "consumed cursor retained its original wire allocation"
            );
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("finite actual incoming batch cursor fixture stalled");
}
