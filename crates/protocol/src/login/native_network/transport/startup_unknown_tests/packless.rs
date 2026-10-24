//! Actual Login can hand over StartGame without any advertised resource-pack phase.

use super::*;
use bedrock_protocol_1_26_51::enums::ServerboundLoadingScreenType;

/// Drives the original settings, Login, spawn and Initialized exchange without pack packets.
async fn serve_packless(peer: &mut AcceptedSession, early_spawn: bool) -> io::Result<()> {
    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
    if !matches!(
        receive(peer, &codec).await?.as_slice(),
        [packets::Packet::RequestNetworkSettings(_)]
    ) {
        return Err(io::Error::other("expected original settings request"));
    }
    send(
        peer,
        &codec,
        Case::Healthy,
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
        receive(peer, &codec).await?.as_slice(),
        [packets::Packet::Login(_)]
    ) {
        return Err(io::Error::other("expected original Login"));
    }
    let spawn = Packet::new(packets::PlayStatus {
        status: PlayStatusType::Playerspawn,
    });
    let mut startup = vec![
        Packet::new(packets::PlayStatus {
            status: PlayStatusType::Loginsuccess,
        }),
        time(1),
    ];
    if early_spawn {
        startup.push(spawn.clone());
    }
    startup.extend([
        Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        }),
        Packet::new(packets::ItemRegistry::default()),
        radius(),
        time(2),
    ]);
    send(peer, &codec, Case::Healthy, Stage::Offer, &startup).await?;
    if !matches!(
        receive(peer, &codec).await?.as_slice(),
        [packets::Packet::ClientCacheStatus(_)]
    ) {
        return Err(io::Error::other("actual LoginSuccess capability changed"));
    }
    if !matches!(
        receive(peer, &codec).await?.as_slice(),
        [packets::Packet::RequestChunkRadius(_), packets::Packet::ServerboundLoadingScreen(start)]
            if start.loading_screen_packet_type == ServerboundLoadingScreenType::Startloadingscreen
    ) {
        return Err(io::Error::other(
            "packless startup invented a pack response or lost the radius/loading request",
        ));
    }
    if !early_spawn {
        send(peer, &codec, Case::Healthy, Stage::Stack, &[spawn]).await?;
    }
    if !matches!(
        receive(peer, &codec).await?.as_slice(),
        [packets::Packet::ServerboundLoadingScreen(end), packets::Packet::SetLocalPlayerAsInitialized(initialized)]
            if end.loading_screen_packet_type == ServerboundLoadingScreenType::Endloadingscreen
                && initialized.player_id.0 == 42
    ) {
        return Err(io::Error::other(
            "original Initialized/loading completion changed",
        ));
    }
    Ok(())
}

type Received = (u64, bool, bool, Vec<Packet>);

/// Returns only after both original sessions, the listener and its bound UDP tuple retire.
async fn exercise_packless(early_spawn: bool) -> (Result<Received, ProtocolError>, io::Result<()>) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = fixture_route(Route::Lan).await;
        let mut listener = fixture.listener;
        let (rejected, mut rejection) = tokio::sync::oneshot::channel();
        let (delivered, delivery) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = listener.accept().await.map_err(io::Error::other)?;
            let result = tokio::select! { biased;
                result = async {
                    serve_packless(&mut peer, early_spawn).await?;
                    delivery.await.map_err(|_| io::Error::other("original FIFO read was dropped"))?;
                    Ok(())
                } => result,
                Ok(()) = &mut rejection => Err(io::Error::other("client rejected packless startup")),
            };
            let retired = peer.close();
            retired.session.closed().await;
            drop(retired);
            result
        };
        let client = async {
            let settings = crate::LoginSettings::default();
            let result = async {
                let joined = tokio::time::timeout(Duration::from_secs(5),
                    LoginSequence::connect_native(
                        &fixture.target,
                        SessionLogin {
                            #[cfg(not(target_arch = "wasm32"))]
                            execution: None,
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
                        |packs, game| (game.world.local_player_runtime_id, packs.is_empty(), packs.required()),
                    )).await.map_err(|_| super::super::super::failure(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "actual packless StartGame left native Login waiting for an offer",
                    )))?;
                let (mut play, (runtime_id, empty, required)) = joined?;
                let received = async {
                    let mut packets = Vec::new();
                    for _ in 0..3 {
                        packets.push(play.recv().await?);
                    }
                    play.finish_loading().await?;
                    Ok::<_, ProtocolError>((runtime_id, empty, required, packets))
                }.await;
                let _ = delivered.send(());
                let closed = play.close().await;
                drop(play);
                closed?;
                received
            }.await;
            if result.is_err() {
                let _ = rejected.send(());
            }
            result
        };
        let (server, client) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        tokio::task::spawn_blocking(move || {
            test_time::eventually("original packless discovery socket retired", || {
                std::net::UdpSocket::bind(fixture.address).is_ok()
            });
        }).await.unwrap();
        (client, server)
    }).await.expect("finite original packless native fixture stalled")
}

/// Checks original readiness and deferred wire order after all real transport owners retire.
fn check(client: Result<Received, ProtocolError>, server: io::Result<()>) {
    let (runtime_id, empty, required, packets) =
        client.expect("packless StartGame did not complete the original native join");
    server.unwrap();
    assert_eq!(runtime_id, 42);
    assert!(empty, "packless startup invented acquired archives");
    assert!(
        !required,
        "packless startup invented a required offer or stack"
    );
    assert_eq!(packets, [time(1), radius(), time(2)]);
}

#[tokio::test]
async fn native_packless_startup_keeps_late_spawn_gate_and_original_fifo() {
    let (client, server) = exercise_packless(false).await;
    check(client, server);
}

#[tokio::test]
async fn native_packless_startup_keeps_early_spawn_prefix_and_original_fifo() {
    let (client, server) = exercise_packless(true).await;
    check(client, server);
}
