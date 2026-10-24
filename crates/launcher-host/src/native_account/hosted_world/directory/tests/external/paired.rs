//! One complete production accepting path into the actual Dragonfly process, with real client IO.

mod authority;
mod bds;
mod multiple;
mod packless;
pub(in crate::native_account::hosted_world) mod provider;

use super::*;
use crate::local_worlds::test_support::paired as world;
use crate::native_account::hosted_world::serve::play::test_support::first_incoming_terminal;
use authority::Authority;
use protocol::{ClientRandomId, LoginSettings, SessionLogin, WorldEvent};
use provider::{Provider, VISITOR_NAME};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_hosted_peer_reaches_the_actual_dragonfly_world_and_retires() {
    signed_hosted_peer(|address| format!("http://{address}:{}", address.port())).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_hosted_hostname_login_reaches_the_original_world_and_retires() {
    signed_hosted_peer(|address| format!("localhost:{}", address.port())).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_hosted_udp_service_login_reaches_the_original_world_and_retires() {
    signed_hosted_peer(|_| "localhost:domain".into()).await;
}

/// Changes only signed address metadata; the actual association, world and retirement remain original.
async fn signed_hosted_peer(address: impl FnOnce(std::net::SocketAddr) -> String) {
    let Some(world) = world::World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let issuer = Provider::new();
        let namespace = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut authority =
            Authority::open(&world.manager, namespace.path(), &issuer.jwks, deadline).await;
        let mut process = authority
            .host
            .prepare_dragonfly_bridge(deadline)
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
        let address = address(signaling.local_addr());
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
                .accept_dragonfly_peer(
                    &mut process,
                    accepted,
                    key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await?;
            let (mut closing, play) = peer.run(&mut authority.host).await;
            let incoming = first_incoming_terminal(&mut closing).await;
            let closed = closing.finish_close(&mut authority.host).await;
            Ok::<_, io::Error>((play, closed, incoming))
        };
        let visitor = async {
            let (mut session, (archives, game)) = protocol::test_support::hosted_client(
                client,
                nonce,
                address,
                authentication,
                SessionLogin {
                    #[cfg(not(target_arch = "wasm32"))]
                    execution: None,
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
            session
                .send(protocol::request_chunk_radius_packet(3, 3))
                .await?;
            let mut terrain = session.terrain_before_spawn();
            let mut response = false;
            while !(terrain && response) {
                match session.recv_world_event(0).await? {
                    WorldEvent::ChunkRadiusUpdated(3) => response = true,
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
        let (play, closed, incoming) =
            server.expect("actual provider Login/packs/StartGame/Initialized did not complete");
        let (forwarding, incoming) =
            incoming.expect("the original accepting wire had already been locally retired");
        let nether_closed = play.as_ref().err().and_then(nether_connection_closed);
        let play = play.err().map(|error| (error.kind(), error.to_string()));
        let closed = closed.err().map(|error| (error.kind(), error.to_string()));
        assert_eq!(
            incoming.kind(),
            io::ErrorKind::UnexpectedEof,
            "the original wire ended before local close with {incoming:?}"
        );
        assert!(
            play.as_ref().map(|(kind, _)| *kind) == Some(io::ErrorKind::UnexpectedEof)
                || (forwarding
                    && play.as_ref().map(|(kind, _)| *kind) == Some(io::ErrorKind::Other)
                    && nether_closed == Some(true)
                    && visitor.is_ok()),
            "unexpected paired play termination: {play:?}; typed Nether ConnectionClosed: {nether_closed:?}; visitor result: {:?}",
            visitor.as_ref().map(|_| ()).map_err(ToString::to_string)
        );
        assert_eq!(
            closed, play,
            "first original termination was replaced during explicit close"
        );
        let (archives, game, terrain, response) =
            visitor.expect("actual paired native client failed");
        assert!(
            archives
                .iter()
                .any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
            "actual final-resource archive was not hash-verified and delivered unchanged"
        );
        assert!(
            game.world.local_player_runtime_id != 0 && terrain && response,
            "actual Dragonfly GameData, terrain or live radius response did not reach the client"
        );
    })
    .await
    .expect("finite actual paired hosted-world fixture stalled");
}

/// Inspects the original typed Nether cause through the existing cached-error wrappers.
fn nether_connection_closed(mut error: &io::Error) -> Option<bool> {
    loop {
        let cause = error.get_ref()?;
        if let Some(error) = cause.downcast_ref::<nethernet_tokio::NetherError>() {
            return Some(matches!(
                error,
                nethernet_tokio::NetherError::ConnectionClosed
            ));
        }
        error = cause.downcast_ref::<Arc<io::Error>>()?.as_ref();
    }
}
