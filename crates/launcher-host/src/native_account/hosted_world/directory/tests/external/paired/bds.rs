//! Same production backend preparation/acceptance with the original real BDS runner and signed guest.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_bds_peer_uses_the_original_http_or_lan_world_and_retires() {
    for lan in [false, true] {
        let Some(world) = crate::local_worlds::test_support::paired_bds::World::open(lan) else {
            return;
        };
        world
            .manager
            .native_host_intent()
            .expect("actual BDS route supports the original host intent");
        tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let issuer = Provider::new();
        let namespace = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut authority =
            Authority::open(&world.manager, namespace.path(), &issuer.jwks, deadline).await;
        let mut process = authority
            .host
            .prepare_backend(deadline)
            .await
            .unwrap();
        let client_key = Arc::new(SigningKey::generate());
        let SignedAssociation {
            accepted,
            key,
            client,
            mut listener,
            signaling,
        } = signed_association(Arc::clone(&client_key)).await;
        let address = format!("http://{}", signaling.local_addr());
        let settings = LoginSettings {
            view_distance_chunks: 2,
            ..Default::default()
        };
        let random_id = ClientRandomId::new(1).unwrap();
        let mut claims = protocol::login_client_data(VISITOR_NAME, random_id, None, &settings);
        protocol::NativeDeviceProfile::fresh()
            .apply(&mut claims, random_id, VISITOR_NAME)
            .unwrap();
        let authentication = issuer.authentication(
            (*client_key).clone(),
            authority.service_time,
            authority.service_observed,
            claims,
        );
        let nonce = authority.nonce.clone();
        let server = async {
            let peer = authority
                .host
                .accept_backend_peer(
                    &mut process,
                    accepted,
                    key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await?;
            let (mut closing, play) = peer.run(&mut authority.host).await;
            let closed = closing.finish_close(&mut authority.host).await;
            Ok::<_, io::Error>((
                play.err().map(|error| (error.kind(), error.to_string())),
                closed.err().map(|error| (error.kind(), error.to_string())),
            ))
        };
        let visitor = async {
            let (mut session, (archives, game)) = protocol::test_support::hosted_client(
                client,
                nonce,
                address,
                authentication,
                SessionLogin {
                    display_name: VISITOR_NAME,
                    client_random_id: random_id,
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    relay: None,
                    target: None,
                },
                |packs, game| (packs.into_archives(), game),
            )
            .await?;
            // This is the sole original client initialization, not a fixture-created runtime ID.
            session.finish_loading().await?;
            let mut terrain = session.terrain_before_spawn();
            // Startup preserves its original radius grant in the play FIFO. Consume it before
            // issuing another request so that it cannot satisfy the live-response assertion.
            let admitted_radius = loop {
                match session.recv_world_event(0).await? {
                    WorldEvent::ChunkRadiusUpdated(radius) => break radius,
                    WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) => terrain = true,
                    _ => {}
                }
            };
            assert!(admitted_radius > 0, "actual startup radius was not admitted");
            session
                .send(protocol::request_chunk_radius_packet(3, 3))
                .await?;
            let mut response = false;
            while !(terrain && response) {
                match session.recv_world_event(0).await? {
                    WorldEvent::ChunkRadiusUpdated(radius) if radius == admitted_radius => response = true,
                    WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) => terrain = true,
                    _ => {}
                }
            }
            session.close().await?;
            Ok::<_, protocol::ProtocolError>((archives, game, terrain, response))
        };
        let (server, visitor) = tokio::join!(server, visitor);
        let cleanup_deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let process_closed = process.close(cleanup_deadline).await.is_ok();
        drop(process);
        listener.close().await.unwrap();
        drop(listener);
        drop(client_key);
        let signaling_joined = match Arc::try_unwrap(signaling) {
            Ok(signaling) => {
                signaling.shutdown().await;
                true
            }
            Err(signaling) => {
                drop(signaling);
                false
            }
        };
        authority.finish(cleanup_deadline).await;
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let world_running = world.manager.status().state == bridge::WorldState::Running;
        assert!(
            process_closed && signaling_joined && cleanup_empty && world_running,
            "paired transport/process/membership cleanup did not preserve the original live world"
        );
        let (play, closed) =
            server.expect("actual provider Login/packs/StartGame/Initialized did not complete");
        assert_eq!(
            play,
            Some((
                io::ErrorKind::Other,
                nethernet_tokio::NetherError::ConnectionClosed.to_string(),
            )),
            "unexpected paired play termination: {play:?}; visitor result: {:?}",
            visitor.as_ref().map(|_| ()).map_err(ToString::to_string)
        );
        assert_eq!(
            closed, play,
            "first original termination was replaced during explicit close"
        );
        let (_archives, game, terrain, response) =
            visitor.expect("actual paired native client failed");
        assert!(
            game.world.local_player_runtime_id != 0 && terrain && response,
            "actual BDS GameData, terrain or live radius response did not reach the client"
        );
    })
    .await
    .expect("finite actual BDS paired hosted-world fixture stalled");
    }
}
