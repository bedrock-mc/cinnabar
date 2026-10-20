//! A real rejected second Login must not end an already initialized original world peer.

use super::*;
use crate::native_account::hosted_world::active::setup;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_second_login_preserves_the_first_actual_world_connection() {
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
        let mut process = authority.host.prepare_backend(deadline).await.unwrap();
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
        let second_nonce = format!(
            "{}x",
            authority
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
        );
        let (proceed, recovered) = oneshot::channel();
        let server = async {
            let first = authority
                .host
                .accept_backend_peer(
                    &mut process,
                    first_accepted,
                    first_host_key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await?;
            authority.host.peers.admit(first)?;
            let rejected = authority
                .host
                .accept_backend_peer(
                    &mut process,
                    second_accepted,
                    second_host_key,
                    ServerHandshakeOptions::default(),
                    deadline,
                )
                .await;
            let error = match rejected {
                Ok(peer) => {
                    let mut closing = peer.begin_close(deadline);
                    let _ = closing.finish_close(&mut authority.host).await;
                    Err(io::Error::other(
                        "second fixture nonce unexpectedly passed admission",
                    ))
                }
                Err(error) => Ok(error),
            };
            let rejected = error
                .as_ref()
                .is_ok_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
            let result = match error {
                Ok(error) => setup::recover(&mut authority.host, &process, error).await,
                Err(error) => Err(error),
            };
            let preserved = result.is_ok();
            proceed.send(result).map_err(|_| unavailable())?;
            let retired = retire_one(&mut authority.host, deadline).await?;
            Ok::<_, io::Error>((rejected, preserved, retired))
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
            let recovery = recovered.await.map_err(|_| unavailable())?;
            if let Err(error) = recovery {
                // Even the failing baseline explicitly closes its original client before assertions.
                let _ = session.close().await;
                return Err(error);
            }
            let mut terrain = session.terrain_before_spawn();
            radius(&mut session, 3, &mut terrain).await?;
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
            match protocol::test_support::hosted_client(
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
                |_, _| (),
            )
            .await
            {
                Ok((mut session, ())) => {
                    let _ = session.close().await;
                    false
                }
                Err(_) => true,
            }
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
        let (rejected, preserved, retired) =
            server.expect("original setup/peer cleanup did not finish");
        assert!(
            rejected && second,
            "second real Login did not reach the original rejection boundary"
        );
        assert!(
            preserved,
            "a completed rejected Login ended the original initialized host peer"
        );
        assert_eq!((retired.0, retired.1), (1, 0));
        let (runtime_id, terrain, pack) =
            first.expect("initialized first visitor lost its original world connection");
        assert!(
            runtime_id != 0 && terrain && pack,
            "original first peer did not receive an actual post-rejection Dragonfly response"
        );
    })
    .await
    .expect("finite actual rejected-peer fixture stalled");
}
