//! Real initialized peer progress across the production renewal phase on the original active host.

use super::*;
use crate::{
    local_worlds::test_support::paired::World,
    native_account::{
        ACQUISITION_TIMEOUT, authentication_policy,
        hosted_world::directory::tests::{
            activation::{activation_nonce, install_activation_account, issuer_jwks},
            external::{
                SignedAssociation,
                paired::provider::{Provider, VISITOR_NAME, VISITOR_XUID},
                signed_association_with_key,
            },
        },
        metadata,
    },
};
use bedrock_client::server::ServerHandshakeOptions;
use bedrock_services::{
    minecraft,
    test_support::{HostedAuthority, HostedSocketTerminal},
};
use nethernet::identity::MINECRAFT_AUDIENCE;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate, pkcs8::EncodePublicKey};
use protocol::{ClientRandomId, LoginSettings, SessionLogin, WorldEvent};
use std::num::NonZeroU32;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialized_world_peer_survives_same_key_mint_and_fresh_turn_on_original_host() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let namespace = tempfile::tempdir().unwrap();
        let issuer = Provider::new();
        let cap = NonZeroU32::new(
            world.manager.status().max_players.unwrap().min(
                u32::try_from(super::super::VISITOR_LIMIT)
                    .unwrap()
                    .checked_add(1)
                    .unwrap(),
            ),
        )
        .unwrap();
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedAuthority {
            account: services,
            identity,
            peer,
        } = HostedAuthority::acquire_activation_renewal(
            authentication_policy(),
            config,
            cap,
            bridge::contract::PRESENCE_TITLE_ID.to_string(),
            MINECRAFT_AUDIENCE.into(),
        )
        .await;
        // The same provider verifies both actual host mints and the independent visitor's Login.
        let mut jwks = issuer_jwks(&peer);
        let visitor_jwks: serde_json::Value = serde_json::from_slice(&issuer.jwks).unwrap();
        jwks["keys"]
            .as_array_mut()
            .unwrap()
            .extend(visitor_jwks["keys"].as_array().unwrap().iter().cloned());
        let account = install_activation_account(
            namespace.path(),
            services,
            &identity,
            &serde_json::to_vec(&jwks).unwrap(),
        )
        .await;
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut active = world
            .manager
            .native_hosted_world(Arc::clone(&account), deadline)
            .unwrap()
            .activate(deadline)
            .await
            .expect("original public host activation");
        let invitation = active.invitation();
        let invited = {
            let running = active.run();
            tokio::pin!(running);
            tokio::select! {
                result = &mut running => result.and(Err(io::Error::other("active host ended before invitation"))),
                result = invitation.send(VISITOR_XUID, deadline) => result,
            }
        };
        drop(invitation);
        let nonce = activation_nonce(&active.world, VISITOR_XUID);
        let (service_time, service_observed) = {
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            let observed = Instant::now();
            let time = operation.minecraft().unwrap().service_time();
            operation.persist().unwrap();
            (time, observed)
        };
        let original_key = active.identity.signing_key();
        let original_public = original_key
            .verifying_key()
            .to_public_key_der()
            .unwrap()
            .as_bytes()
            .to_vec();
        let client_key = Arc::new(SigningKey::generate());
        let SignedAssociation {
            accepted,
            key,
            client,
            mut listener,
            signaling,
        } = signed_association_with_key(Arc::clone(&client_key), Arc::clone(&original_key))
            .await;
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
            service_time,
            service_observed,
            claims,
        );
        let (renewed, renewal_finished) = oneshot::channel();
        let server = async {
            let initialized = driving(
                &mut active.server,
                active.world.accept_backend_peer(
                    &mut active.process,
                    accepted,
                    key,
                    ServerHandshakeOptions::default(),
                    deadline,
                ),
            )
            .await?;
            active.world.peers.admit(initialized)?;
            let occupied = active.world.peers.occupied();
            // This invokes the production finite phase directly; it does not advance a fixture clock.
            active.renew_admission().await;
            let renewal = if active.renewal_failure.is_some() {
                Err(active.admission_error())
            } else {
                Ok(())
            };
            let same_key = Arc::ptr_eq(&original_key, &active.identity.signing_key());
            renewed.send(renewal.is_ok()).map_err(|_| unavailable())?;
            loop {
                active.retire_ready().await?;
                if active.world.peers.occupied() == 0 {
                    break;
                }
                driving(&mut active.server, active.world.drive_active()).await?;
            }
            Ok::<_, io::Error>((occupied, same_key, renewal))
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
            .await
            .map_err(io::Error::other)?;
            session.finish_loading().await.map_err(io::Error::other)?;
            let renewed = renewal_finished.await.map_err(|_| unavailable())?;
            let mut terrain = session.terrain_before_spawn();
            let mut response = false;
            if renewed {
                session
                    .send(protocol::request_chunk_radius_packet(3, 3))
                    .await
                    .map_err(io::Error::other)?;
                while !(terrain && response) {
                    match session.recv_world_event(0).await.map_err(io::Error::other)? {
                        WorldEvent::ChunkRadiusUpdated(3) => response = true,
                        WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) => terrain = true,
                        _ => {}
                    }
                }
            }
            session.close().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>((archives, game, terrain, response))
        };
        let (server, visitor) = tokio::join!(server, visitor);
        let listener_closed = listener.close().await;
        drop((listener, client_key, original_key));
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
        let mut closing = active.begin_close(Instant::now() + ACQUISITION_TIMEOUT);
        let closed = closing.finish_close().await;
        drop(closing);
        let observed = peer.finish_activation_renewal().await;
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let world_running = world.manager.status().state == bridge::WorldState::Running;

        invited.expect("original active-owner invitation");
        listener_closed.expect("original independent association listener retirement");
        closed.expect("original active host retirement");
        assert!(signaling_joined && cleanup_empty && world_running);
        let (occupied, same_key, renewal) = server.expect("original initialized peer failed");
        renewal.expect("actual same-owner renewal phase failed");
        assert_eq!(occupied, 1);
        assert!(same_key, "renewal replaced the original signing key owner");
        assert_eq!(observed.activation.minted_public_key, original_public);
        assert_eq!(observed.renewed_public_key, original_public);
        assert_ne!(observed.original_turn_request, observed.refreshed_turn_request);
        assert_eq!(observed.activation.invited_xuid, VISITOR_XUID);
        assert_eq!(observed.activation.max_members, cap.get());
        assert_eq!(
            observed.activation.rta,
            HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof)
        );
        assert!(matches!(
            observed.activation.messaging,
            HostedSocketTerminal::Close | HostedSocketTerminal::Eof
        ));
        let (archives, game, terrain, response) = visitor.expect("actual visitor failed after renewal");
        assert!(
            archives
                .iter()
                .any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
            "original hash-verified resource pack was lost"
        );
        assert!(
            game.world.local_player_runtime_id != 0 && terrain && response,
            "real initialized peer did not receive backend terrain/radius response after renewal"
        );
    })
    .await
    .expect("finite original-host renewal and actual peer fixture stalled");
}

mod expiry;
