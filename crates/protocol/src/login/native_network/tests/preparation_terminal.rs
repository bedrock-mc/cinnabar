//! Native startup terminals must supersede held presentation without consuming accepted input.

use super::super::{
    negotiated,
    transport::early_spawn_tests::{FixtureRoute, Route, fixture_route, receive, send},
};
use crate::login::{session_join::prepare_with_read_ahead, startup};
use crate::session_transport::{Inbound, SessionTransport};
use crate::{ClientRandomId, LoginSettings, Packet, ProtocolError, SessionLogin};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings,
};
use bedrock_protocol_1_26_51::{
    enums::{ConnectionDisconnectFailReason, PlayStatusType},
    packets,
    types::{ActorRuntimeID, DisconnectMessages},
    wire,
};
use std::{future::Future, pin::pin, task::Poll, time::Duration};
use tokio::sync::oneshot;

#[tokio::test]
async fn native_disconnect_during_held_preparation_keeps_reason_and_accepted_fifo() {
    let terminal = Packet::new(packets::Disconnect {
        reason: ConnectionDisconnectFailReason::Serverfull,
        messages: DisconnectMessages::DisconnectPacketMessages {
            message: "presentation interrupted".into(),
            filtered_message: "filtered presentation interrupted".into(),
        },
        ..Default::default()
    });
    let result = held_preparation(terminal, false).await;
    let error = result
        .expect("native Disconnect must finish startup while presentation remains held")
        .expect_err("a disconnected startup must not succeed");
    let disconnect = error
        .server_disconnect()
        .expect("original Disconnect reason");
    assert_eq!(disconnect.reason, "Serverfull");
    assert_eq!(
        disconnect.message.as_deref(),
        Some("presentation interrupted")
    );
    assert_eq!(
        disconnect.filtered_message.as_deref(),
        Some("filtered presentation interrupted")
    );
}

#[tokio::test]
async fn native_transfer_during_held_preparation_keeps_target_and_accepted_fifo() {
    let terminal = Packet::new(packets::Transfer {
        server_address: "next.example.test".into(),
        server_port: wire::U16LE(19133),
        reload_world: true,
        ..Default::default()
    });
    let result = held_preparation(terminal, false).await;
    let error = result
        .expect("native Transfer must finish startup while presentation remains held")
        .expect_err("a transferred startup must not succeed");
    let transfer = error
        .server_transfer()
        .expect("original validated Transfer");
    assert_eq!(transfer.host, "next.example.test");
    assert_eq!(transfer.port, 19133);
    assert!(transfer.reload_world);
}

#[tokio::test]
async fn received_native_disconnect_supersedes_held_preparation_without_consuming_its_fifo() {
    let terminal = Packet::new(packets::Disconnect {
        reason: ConnectionDisconnectFailReason::Serverfull,
        messages: DisconnectMessages::DisconnectPacketMessages {
            message: "buffered presentation interrupted".into(),
            filtered_message: "".into(),
        },
        ..Default::default()
    });
    let result = held_preparation(terminal, true).await;
    let error = result
        .expect("already received native Disconnect must supersede held preparation")
        .expect_err("a disconnected startup must not succeed");
    let disconnect = error.server_disconnect().expect("original Disconnect");
    assert_eq!(disconnect.reason, "Serverfull");
    assert_eq!(
        disconnect.message.as_deref(),
        Some("buffered presentation interrupted")
    );
}

#[tokio::test]
async fn received_native_transfer_supersedes_held_preparation_without_consuming_its_fifo() {
    let terminal = Packet::new(packets::Transfer {
        server_address: "buffered.example.test".into(),
        server_port: wire::U16LE(19134),
        reload_world: true,
        ..Default::default()
    });
    let result = held_preparation(terminal, true).await;
    let error = result
        .expect("already received native Transfer must supersede held preparation")
        .expect_err("a transferred startup must not succeed");
    let transfer = error.server_transfer().expect("original Transfer");
    assert_eq!(transfer.host, "buffered.example.test");
    assert_eq!(transfer.port, 19134);
    assert!(transfer.reload_world);
}

/// Runs the original real association, Login, packs and spawn before holding one preparation job.
async fn held_preparation(
    terminal: Packet,
    received_prefix: bool,
) -> Option<Result<usize, ProtocolError>> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            http,
            approval,
            ..
        } = fixture_route(Route::Lan).await;
        assert!(http.is_none() && approval.is_none());
        let deferred = time(11);
        let prefix = time(17);
        let suffix = time(19);
        let (startup_ready, ready) = oneshot::channel();
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
            let mut spawning = vec![
                Packet::new(packets::ResourcePackStack::default()),
                deferred.clone(),
                Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Playerspawn,
                }),
                Packet::new(packets::StartGame {
                    runtime_id: ActorRuntimeID(42),
                    ..Default::default()
                }),
                Packet::new(packets::ItemRegistry::default()),
            ];
            if received_prefix {
                spawning.extend([prefix.clone(), terminal.clone(), suffix.clone()]);
            }
            send(&peer, &codec, &spawning).await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::RequestChunkRadius(_)
            ));
            ready.await.unwrap();
            if !received_prefix {
                send(
                    &peer,
                    &codec,
                    &[prefix.clone(), terminal.clone(), suffix.clone()],
                )
                .await;
            }
            peer.session.closed().await;
        };
        let client = async {
            let settings = LoginSettings::default();
            let login = SessionLogin {
                display_name: "Fixture",
                client_random_id: ClientRandomId::new(1).unwrap(),
                cache: None,
                skin: None,
                settings: &settings,
                tap: None,
                relay: None,
                target: None,
            };
            let mut negotiated = negotiated::negotiate(
                &target,
                &login,
                None,
                tokio::time::Instant::now() + Duration::from_secs(15),
            )
            .await
            .unwrap();
            let startup_packets = negotiated.connection.take_deferred();
            let mut transport = SessionTransport::from_native(negotiated.connection);
            transport.adopt_startup(startup_packets);
            let mut actual_startup = None;
            let _spawned =
                startup::await_spawn(&mut transport, settings.view_distance_chunks, |start, _| {
                    actual_startup = Some(start.runtime_id.0)
                })
                .await
                .unwrap();
            assert_eq!(actual_startup, Some(42));
            transport.finish_startup();
            let (release, held) = oneshot::channel::<usize>();
            let mut preparation = pin!(async { held.await.unwrap() });
            assert!(futures::poll!(preparation.as_mut()).is_pending());
            startup_ready.send(()).unwrap();
            let before_release = std::future::poll_fn(|cx| {
                let outcome = {
                    let mut waiting = pin!(prepare_with_read_ahead(
                        &mut transport,
                        preparation.as_mut(),
                    ));
                    waiting.as_mut().poll(cx)
                };
                match outcome {
                    Poll::Ready(result) => Poll::Ready(Some(result)),
                    Poll::Pending
                        if transport.has_retained_native_batch()
                            || transport.has_retained_native_terminal() =>
                    {
                        Poll::Ready(None)
                    }
                    Poll::Pending => Poll::Pending,
                }
            })
            .await;
            let mut accepted = Vec::new();
            for _ in 0..4 {
                let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                    panic!("native packet FIFO was replaced by a synthetic terminal")
                };
                accepted.push(raw.decode().unwrap());
            }
            release.send(7).unwrap();
            assert_eq!(preparation.await, 7);
            transport.close().await.unwrap();
            drop(transport);
            (before_release, accepted)
        };
        let ((), (before_release, accepted)) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        assert_eq!(accepted, [deferred, prefix, terminal, suffix]);
        before_release
    })
    .await
    .expect("finite native startup/preparation terminal fixture stalled")
}

/// Produces an ordinary generated play packet with an unambiguous FIFO value.
fn time(value: i32) -> Packet {
    Packet::new(packets::SetTime {
        time: wire::ZigZag32(value),
    })
}
