//! Two actual visitors share one original process and the production owned peer continuations.

use super::*;
use crate::native_account::hosted_world::active::{VISITOR_LIMIT, peers::Peers};
use bedrock_services::test_support::HOSTED_WORLD_SECOND_VISITOR_XUID;
use tokio::sync::oneshot;

const SECOND_NAME: &str = "SecondVisitor";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_signed_visitors_share_the_actual_world_and_retire_independently() {
    let Some(world) = world::World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let issuer = Provider::new();
        let namespace = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut authority =
            Authority::open_two_visitors(&world.manager, namespace.path(), &issuer.jwks, deadline)
                .await;
        let limit = authority
            .host
            .directory
            .as_ref()
            .unwrap()
            .visitor_limit()
            .unwrap()
            .min(VISITOR_LIMIT);
        authority.host.peers = Peers::new(limit);
        let mut process = authority
            .host
            .prepare_dragonfly_bridge(deadline)
            .await
            .unwrap();
        let first_key = Arc::new(SigningKey::generate());
        let second_key = Arc::new(SigningKey::generate());
        let SignedAssociation {
            accepted: first_accepted,
            key: first_host_key,
            client: first_client,
            listener: mut first_listener,
            signaling: first_signaling,
        } = signed_association(Arc::clone(&first_key)).await;
        let SignedAssociation {
            accepted: second_accepted,
            key: second_host_key,
            client: second_client,
            listener: mut second_listener,
            signaling: second_signaling,
        } = signed_association(Arc::clone(&second_key)).await;
        let settings = LoginSettings {
            view_distance_chunks: 2,
            ..Default::default()
        };
        let first_id = ClientRandomId::new(1).unwrap();
        let second_id = ClientRandomId::new(2).unwrap();
        let authenticate = |key: &Arc<SigningKey>, id, xuid, name| {
            let mut claims = protocol::login_client_data(name, id, None, &settings);
            protocol::NativeDeviceProfile::fresh()
                .apply(&mut claims, id, name)
                .unwrap();
            issuer.authentication_for(
                (**key).clone(),
                authority.service_time,
                authority.service_observed,
                claims,
                (xuid, name),
            )
        };
        let first_authentication =
            authenticate(&first_key, first_id, provider::VISITOR_XUID, VISITOR_NAME);
        let second_authentication = authenticate(
            &second_key,
            second_id,
            HOSTED_WORLD_SECOND_VISITOR_XUID,
            SECOND_NAME,
        );
        let first_nonce = authority.nonce.clone();
        let second_nonce = authority
            .host
            .directory
            .as_ref()
            .unwrap()
            .approvals
            .as_ref()
            .unwrap()
            .document()
            .unwrap()[HOSTED_WORLD_SECOND_VISITOR_XUID]
            .as_str()
            .unwrap()
            .to_owned();
        authority
            .host
            .check_approval(HOSTED_WORLD_SECOND_VISITOR_XUID, &second_nonce)
            .unwrap();
        let (waiting, second_waiting) = oneshot::channel();
        let (responded, first_responded) = oneshot::channel();
        let (initialized, second_initialized) = oneshot::channel();
        let (retired, first_retired) = oneshot::channel();
        let server = async {
            let first = authority
                .host
                .accept_dragonfly_peer(
                    &mut process,
                    first_accepted,
                    first_host_key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await?;
            authority.host.peers.admit(first)?;
            // The production setup wait drives the same owned first peer while the second is uninitialized.
            let second = authority
                .host
                .accept_dragonfly_peer(
                    &mut process,
                    second_accepted,
                    second_host_key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await?;
            authority.host.peers.admit(second)?;
            let occupied = authority.host.peers.occupied();
            let first = retire_one(&mut authority.host, deadline).await?;
            retired.send(()).map_err(|_| unavailable())?;
            let second = retire_one(&mut authority.host, deadline).await?;
            Ok::<_, io::Error>((occupied, first, second))
        };
        let first = async {
            let (mut session, (archives, game)) = protocol::test_support::hosted_client(
                first_client,
                first_nonce,
                format!("http://{}", first_signaling.local_addr()),
                first_authentication,
                SessionLogin {
                    display_name: VISITOR_NAME,
                    client_random_id: first_id,
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    relay: None,
                    target: None,
                },
                |packs, game| (packs.into_archives(), game),
            )
            .await
            .map_err(io::Error::other)?;
            session.finish_loading().await.map_err(io::Error::other)?;
            second_waiting.await.map_err(|_| unavailable())?;
            let mut terrain = session.terrain_before_spawn();
            radius(&mut session, 3, &mut terrain).await?;
            // Actual response, not a Ready surrogate, releases the second client's real Initialized send.
            responded.send(()).map_err(|_| unavailable())?;
            second_initialized.await.map_err(|_| unavailable())?;
            session.close().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>((
                game.world.local_player_runtime_id,
                terrain,
                archives
                    .iter()
                    .any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
            ))
        };
        let second = async {
            let (mut session, (archives, game)) = protocol::test_support::hosted_client(
                second_client,
                second_nonce,
                format!("http://{}", second_signaling.local_addr()),
                second_authentication,
                SessionLogin {
                    display_name: SECOND_NAME,
                    client_random_id: second_id,
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    relay: None,
                    target: None,
                },
                |packs, game| (packs.into_archives(), game),
            )
            .await
            .map_err(io::Error::other)?;
            waiting.send(()).map_err(|_| unavailable())?;
            first_responded.await.map_err(|_| unavailable())?;
            session.finish_loading().await.map_err(io::Error::other)?;
            let mut terrain = session.terrain_before_spawn();
            radius(&mut session, 3, &mut terrain).await?;
            initialized.send(()).map_err(|_| unavailable())?;
            first_retired.await.map_err(|_| unavailable())?;
            // The survivor still reaches Dragonfly after the first original peer's completed close.
            radius(&mut session, 4, &mut terrain).await?;
            session.close().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>((
                game.world.local_player_runtime_id,
                terrain,
                archives
                    .iter()
                    .any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
            ))
        };
        let (server, first, second) = tokio::join!(server, first, second);
        let cleanup_deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let process_closed = process.close(cleanup_deadline).await.is_ok();
        drop(process);
        first_listener.close().await.unwrap();
        second_listener.close().await.unwrap();
        drop((first_listener, second_listener, first_key, second_key));
        let first_joined = join_signaling(first_signaling).await;
        let second_joined = join_signaling(second_signaling).await;
        authority.finish(cleanup_deadline).await;
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let world_running = world.manager.status().state == bridge::WorldState::Running;
        assert!(process_closed && first_joined && second_joined && cleanup_empty && world_running);
        let (occupied, first_retirement, second_retirement) =
            server.expect("two actual accepting peer lifetimes failed");
        assert!(
            limit >= 2,
            "actual original world did not admit two visitors"
        );
        assert_eq!(occupied, 2);
        assert_eq!((first_retirement.0, first_retirement.1), (2, 1));
        assert_eq!((second_retirement.0, second_retirement.1), (1, 0));
        let terminal = (
            io::ErrorKind::Other,
            nethernet_tokio::NetherError::ConnectionClosed.to_string(),
        );
        assert_eq!(first_retirement.2, Some(terminal.clone()));
        assert_eq!(second_retirement.2, Some(terminal));
        let first = first.expect("first actual visitor failed during the second setup");
        let second = second.expect("second actual visitor failed after the first retired");
        // Dragonfly assigns self runtime IDs within each connection, independently of other visitors.
        assert!(first.0 != 0 && second.0 != 0);
        assert!(
            first.1 && first.2 && second.1 && second.2,
            "original final resource snapshot or real Dragonfly terrain was lost"
        );
    })
    .await
    .expect("finite actual two-visitor world fixture stalled");
}

/// Uses the original completed closing owner before releasing its occupied slot.
async fn retire_one(
    host: &mut NativeHostedWorld,
    deadline: Instant,
) -> io::Result<(usize, usize, Option<(io::ErrorKind, String)>)> {
    loop {
        if let Some(mut closing) = host.peers.take_closing() {
            let occupied = host.peers.occupied();
            let result = closing.finish_close(host).await;
            if !closing.is_finished() || host.peers.occupied() != occupied {
                return Err(unavailable());
            }
            host.peers.retired();
            return Ok((
                occupied,
                host.peers.occupied(),
                result.err().map(|error| (error.kind(), error.to_string())),
            ));
        }
        let wake = host.peers.wake();
        host.peer_io(Some(deadline), async {
            wake.notified().await;
            Ok(())
        })
        .await?;
    }
}

/// Waits for real generated world events, retaining terrain already observed by this same client.
async fn radius(
    session: &mut protocol::PlaySession,
    radius: u8,
    terrain: &mut bool,
) -> io::Result<()> {
    session
        .send(protocol::request_chunk_radius_packet(radius, radius))
        .await
        .map_err(io::Error::other)?;
    let mut response = false;
    while !(*terrain && response) {
        match session
            .recv_world_event(0)
            .await
            .map_err(io::Error::other)?
        {
            WorldEvent::ChunkRadiusUpdated(actual) if actual == i32::from(radius) => {
                response = true
            }
            WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) => *terrain = true,
            _ => {}
        }
    }
    Ok(())
}

/// Joins the same actual signaling owner after its original listener and peers retire.
async fn join_signaling(signaling: Arc<HttpSignalingServer>) -> bool {
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

/// Does not include fixture peer, account or world data in diagnostics.
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "original two-visitor fixture owner ended",
    )
}

mod rejection;
