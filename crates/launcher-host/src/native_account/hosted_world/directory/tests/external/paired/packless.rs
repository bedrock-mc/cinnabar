//! Real two-leg hosted protocol admission without an upstream pack advertisement.

use super::*;
use crate::local_worlds::test_support::packless::World;
use protocol::test_support::{hosted_packless as upstream, packets::chunk_radius_updated_packet};

mod closing;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_hosted_packless_upstream_preserves_startup_initialized_and_live_marker() {
    run(false).await;
}

/// Retains the entire original scenario and optionally observes its BDS closing prefix.
async fn run(closing_prefix: bool) {
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let udp = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let udp_address = udp.local_addr().unwrap();
        drop(udp);
        let upstream_signaling = Arc::new(
            HttpSignalingServer::bind((Ipv4Addr::LOCALHOST, 0).into(), HttpServerConfig::default())
                .await.unwrap(),
        );
        let upstream_address = upstream_signaling.local_addr();
        let mut upstream_listener = NetherServer::bind_with(
            Arc::clone(&upstream_signaling),
            ConnectionConfig {
                identity: Some(Arc::new(ServerIdentity::generate("self", SystemTime::now()).unwrap())),
                server_ice: Some(ServerIceConfig {
                    turn_streams: Vec::new(),
                    turn_tls: None,
                    ice: Default::default(),
                    bind_addr: udp_address,
                }),
                ..Default::default()
            },
        ).await.unwrap();
        let world = World::open(upstream_address);
        let issuer = Provider::new();
        let namespace = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut authority = Authority::open(&world.manager, namespace.path(), &issuer.jwks, deadline).await;
        let mut process = authority.host.prepare_backend(deadline).await.unwrap();
        let client_key = Arc::new(SigningKey::generate());
        let SignedAssociation {
            accepted, key, client, mut listener, signaling,
        } = signed_association(Arc::clone(&client_key)).await;
        let downstream_address = signaling.local_addr();
        let settings = LoginSettings { view_distance_chunks: 2, ..Default::default() };
        let random_id = ClientRandomId::new(1).unwrap();
        let mut claims = protocol::login_client_data(VISITOR_NAME, random_id, None, &settings);
        protocol::NativeDeviceProfile::fresh().apply(&mut claims, random_id, VISITOR_NAME).unwrap();
        let authentication = issuer.authentication(
            (*client_key).clone(), authority.service_time, authority.service_observed, claims,
        );
        let nonce = authority.nonce.clone();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (startup_sent, mut startup_observed) = tokio::sync::oneshot::channel();
        let (local_prefix, server_prefix, visitor_prefix) = if closing_prefix {
            let (ready, ready_receive) = tokio::sync::oneshot::channel();
            let (start, start_receive) = tokio::sync::oneshot::channel();
            let (marker, marker_receive) = tokio::sync::oneshot::channel();
            let (closed, closed_receive) = tokio::sync::oneshot::channel();
            (
                Some((ready, start_receive)),
                Some((ready_receive, marker_receive, start, closed)),
                Some((marker, closed_receive)),
            )
        } else {
            (None, None, None)
        };
        let local = async {
            let mut stopped = stopped;
            let accepted = tokio::select! { biased;
                accepted = upstream_listener.accept() => Some(accepted),
                Ok(()) = &mut stopped => None,
            };
            let Some(accepted) = accepted else { return (None, true); };
            let mut peer = accepted.unwrap();
            let result = tokio::select! { biased;
                result = async {
                    match local_prefix {
                        Some((ready, start)) => upstream::run_with_closing_prefix(
                            &mut peer, startup_sent, ready, start,
                        ).await,
                        None => upstream::run(&mut peer, startup_sent).await,
                    }
                } => Some(result),
                Ok(()) = &mut stopped => None,
            };
            let retired = peer.close();
            let joined = tokio::time::timeout_at(deadline.into(), retired.session.closed()).await.is_ok();
            drop(retired);
            (result, joined)
        };
        let server = async {
            let result = async {
                let peer = authority.host.accept_backend_peer(
                    &mut process, accepted, key, ServerHandshakeOptions::default(), deadline,
                ).await?;
                if let Some((ready, marker, start, closed)) = server_prefix {
                    let result = crate::native_account::hosted_world::serve::play::test_support::close_bds_prefix(
                        peer, &mut authority.host, ready, marker, start, deadline,
                    ).await;
                    let _ = closed.send(());
                    return result.map(closing::Outcome::Prefix);
                }
                let (mut closing, played) = peer.run(&mut authority.host).await;
                let closed = closing.finish_close(&mut authority.host).await;
                Ok::<_, io::Error>(closing::Outcome::Played(
                    played.map_err(|e| e.kind()), closed.map_err(|e| e.kind()),
                ))
            }.await;
            // Original rejection cancels only this fixture's held upstream continuation.
            if result.is_err() {
                let _ = stop.send(());
            }
            result
        };
        let visitor = async {
            let (mut play, (archives, runtime_id)) = protocol::test_support::hosted_client(
                client, nonce, format!("http://{downstream_address}:{}", downstream_address.port()), authentication,
                SessionLogin {
                    #[cfg(not(target_arch = "wasm32"))]
                    execution: None,
                    display_name: VISITOR_NAME, client_random_id: random_id, cache: None, skin: None,
                    settings: &settings, tap: None, relay: None, target: None,
                },
                |packs, game| (packs.into_archives(), game.world.local_player_runtime_id),
            ).await?;
            let result = async {
                let mut prefix = Vec::new();
                for _ in 0..3 { prefix.push(play.recv().await?); }
                play.finish_loading().await?;
                play.send(protocol::request_chunk_radius_packet(
                    upstream::MARKER_RADIUS, upstream::MARKER_RADIUS,
                )).await?;
                let marker = play.recv().await?;
                let closing = if let Some((received, closed)) = visitor_prefix {
                    received.send(()).map_err(|_| protocol::ProtocolError::Bridge(
                        io::Error::from(io::ErrorKind::BrokenPipe).into(),
                    ))?;
                    let packets = [play.recv().await?, play.recv().await?];
                    closed.await.map_err(|_| protocol::ProtocolError::Bridge(
                        io::Error::from(io::ErrorKind::BrokenPipe).into(),
                    ))?;
                    Some(packets)
                } else {
                    None
                };
                Ok::<_, protocol::ProtocolError>((archives.is_empty(), runtime_id, prefix, marker, closing))
            }.await;
            let closed = play.close().await;
            drop(play);
            closed?;
            result
        };
        let (server, visitor, (local, upstream_joined)) = tokio::join!(server, visitor, local);
        let startup_sent = startup_observed.try_recv().is_ok();
        let cleanup_deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let backend_closed = process.close(cleanup_deadline).await.is_ok();
        drop(process);
        let downstream_closed = listener.close().await.is_ok();
        drop(listener);
        let upstream_closed = upstream_listener.close().await.is_ok();
        drop(upstream_listener);
        drop(client_key);
        let downstream_http = shutdown(signaling).await;
        let upstream_http = shutdown(upstream_signaling).await;
        authority.finish(cleanup_deadline).await;
        let membership_empty = world.manager.native_host_cleanup().unwrap().is_none();
        world.manager.shutdown();
        let manager_stopped = world.runner.last().stopped();
        tokio::task::spawn_blocking(move || {
            test_time::eventually("original hosted packless listeners retired", || {
                std::net::UdpSocket::bind(udp_address).is_ok()
            });
        }).await.unwrap();
        // Every original transport, manager and membership owner is retired before the compatibility oracle.
        assert!(upstream_joined && backend_closed && downstream_closed && upstream_closed
            && downstream_http && upstream_http && membership_empty && manager_stopped,
            "original hosted packless physical cleanup failed");
        assert!(startup_sent, "original upstream never sent its actual packless startup");
        assert!(server.is_ok(),
            "actual hosted packless startup was refused after original cleanup: {server:?}; visitor: {:?}",
            visitor.as_ref().map(|_| ()).map_err(ToString::to_string));
        assert!(matches!(local, Some(Ok(()))), "original upstream did not observe initialization, marker and EOF: {local:?}");
        match server.unwrap() {
            closing::Outcome::Played(played, closed) => {
                assert!(!closing_prefix);
                assert_eq!(played, Err(io::ErrorKind::UnexpectedEof));
                assert_eq!(closed, played, "original hosted close replaced its play terminal");
            }
            closing::Outcome::Prefix(observation) => {
                assert!(closing_prefix);
                closing::assert_observed(observation);
            }
        }
        let (empty, runtime_id, prefix, marker, closing) = visitor.unwrap();
        assert!(empty, "packless upstream invented archives");
        assert_eq!(runtime_id, upstream::RUNTIME_ID);
        assert_eq!(prefix, upstream::prefix());
        assert_eq!(marker, chunk_radius_updated_packet(i32::from(upstream::MARKER_RADIUS)));
        if closing_prefix {
            assert_eq!(closing.unwrap(), upstream::closing_prefix());
        } else {
            assert!(closing.is_none());
        }
    }).await.expect("finite original hosted packless fixture stalled");
}

/// Joins the original HTTP owner only after both of its original transport handles have closed.
async fn shutdown(signaling: Arc<HttpSignalingServer>) -> bool {
    match Arc::try_unwrap(signaling) {
        Ok(signaling) => {
            signaling.shutdown().await;
            true
        }
        Err(signaling) => {
            drop(signaling);
            false
        }
    }
}
