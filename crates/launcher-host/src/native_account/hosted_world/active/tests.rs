//! Full production Active.run acceptance through the original protected service and world process.

mod client;

use super::*;
use crate::{
    local_worlds::test_support::paired::World,
    native_account::{
        ACQUISITION_TIMEOUT, authentication_policy,
        hosted_world::directory::tests::{
            activation::{activation_nonce, install_activation_account, issuer_jwks},
            external::paired::provider::{Provider, VISITOR_NAME, VISITOR_XUID},
        },
        metadata,
    },
};
use bedrock_services::{
    minecraft,
    test_support::{HostedActivationVisitors, HostedAuthority, HostedSocketTerminal},
};
use client::{Client, ended, radius};
use nethernet::identity::MINECRAFT_AUDIENCE;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use protocol::{ClientRandomId, LoginSettings};
use std::num::NonZeroU32;

const SECOND_NAME: &str = "SecondVisitor";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_run_routes_two_signed_messaging_visitors_into_the_original_world() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 3, Box::pin(async {
        let namespace = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + ACQUISITION_TIMEOUT * 2;
        let cap = NonZeroU32::new(
            world.manager.status().max_players.unwrap().min(
                u32::try_from(VISITOR_LIMIT).unwrap().checked_add(1).unwrap(),
            ),
        ).unwrap();
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedActivationVisitors {
            authority: HostedAuthority { account: services, identity, peer },
            visitors: [first_service, second_service],
            host_messaging_id,
        } = HostedAuthority::acquire_activation_visitors(
            authentication_policy(), config, cap,
            bridge::contract::PRESENCE_TITLE_ID.to_string(), MINECRAFT_AUDIENCE.into(), deadline,
        ).await;
        let mut issuer = Provider::new();
        let mut jwks = issuer_jwks(&peer);
        let guest_keys: serde_json::Value = serde_json::from_slice(&issuer.jwks).unwrap();
        jwks["keys"].as_array_mut().unwrap().extend(guest_keys["keys"].as_array().unwrap().iter().cloned());
        // Both actual host SDP and independently signed guest Login use the same checked provider cache.
        issuer.jwks = serde_json::to_vec(&jwks).unwrap();
        let account = install_activation_account(namespace.path(), services, &identity, &issuer.jwks).await;
        let mut active = world.manager.native_hosted_world(Arc::clone(&account), deadline).unwrap()
            .activate(deadline).await.expect("actual original host activation");
        let invitation = active.invitation();
        let invited = {
            let running = active.run();
            tokio::pin!(running);
            tokio::select! {
                result = &mut running => result.and(Err(io::Error::other("original active host ended before invitation"))),
                result = invitation.send(VISITOR_XUID, deadline) => result,
            }
        };
        drop(invitation);
        let first_xuid = first_service.xuid;
        let second_xuid = second_service.xuid;
        let first_nonce = activation_nonce(&active.world, first_xuid);
        let second_nonce = activation_nonce(&active.world, second_xuid);
        let (service_time, observed) = {
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            let observed = Instant::now();
            let time = operation.minecraft().unwrap().service_time();
            operation.persist().unwrap();
            (time, observed)
        };
        let settings = LoginSettings { view_distance_chunks: 2, ..Default::default() };
        let authentication = |random_id, xuid, name| {
            let mut claims = protocol::login_client_data(name, random_id, None, &settings);
            protocol::NativeDeviceProfile::fresh().apply(&mut claims, random_id, name).unwrap();
            issuer.authentication_for(SigningKey::generate(), service_time, observed, claims, (xuid, name))
        };
        let first_id = ClientRandomId::new(1).unwrap();
        let second_id = ClientRandomId::new(2).unwrap();
        let first = Client::new(first_service, host_messaging_id, first_nonce.clone(), VISITOR_NAME, first_id,
            authentication(first_id, first_xuid, VISITOR_NAME)).unwrap();
        let second = Client::new(second_service, host_messaging_id, second_nonce.clone(), SECOND_NAME, second_id,
            authentication(second_id, second_xuid, SECOND_NAME)).unwrap();
        let (first_ready, first_initialized) = oneshot::channel();
        let (waiting, second_waiting) = oneshot::channel();
        let (responded, first_responded) = oneshot::channel();
        let (second_ready, second_initialized) = oneshot::channel();
        let first = async {
            let (mut session, packs, game) = first.join(&settings).await?;
            session.finish_loading().await.map_err(io::Error::other)?;
            peer.wait_activation_visitor_writes(1).await;
            first_ready.send(()).map_err(|_| ended())?;
            second_waiting.await.map_err(|_| ended())?;
            let mut terrain = session.terrain_before_spawn();
            radius(&mut session, 3, &mut terrain).await?;
            responded.send(()).map_err(|_| ended())?;
            second_initialized.await.map_err(|_| ended())?;
            session.close().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>((packs, game, terrain))
        };
        let second = async {
            first_initialized.await.map_err(|_| ended())?;
            let (mut session, packs, game) = second.join(&settings).await?;
            // A real pre-Initialized session retains the production host's finite setup phase.
            waiting.send(()).map_err(|_| ended())?;
            first_responded.await.map_err(|_| ended())?;
            session.finish_loading().await.map_err(io::Error::other)?;
            let mut terrain = session.terrain_before_spawn();
            radius(&mut session, 3, &mut terrain).await?;
            second_ready.send(()).map_err(|_| ended())?;
            peer.wait_activation_visitor_writes(3).await;
            radius(&mut session, 4, &mut terrain).await?;
            session.close().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>((packs, game, terrain))
        };
        let clients = async {
            let (first, second) = tokio::join!(first, second);
            let first = first?;
            let second = second?;
            peer.wait_activation_visitor_writes(4).await;
            // A flushed response alone is not application acknowledgment: wait for the same lease too.
            let mut operation = Arc::clone(&account).acquire_verified().await?;
            operation.persist()?;
            drop(operation);
            Ok::<_, io::Error>((first, second))
        };
        let result = {
            let running = active.run();
            tokio::pin!(running);
            tokio::select! { biased;
                result = &mut running => result.and(Err(io::Error::other("original active host ended during visitors"))),
                result = clients => result,
            }
        };
        let completed = result.is_ok();
        let idle = !active.phase_started && active.retiring.is_none() && active.world.peers.occupied() == 0;
        let approvals = active.world.check_approval(first_xuid, &first_nonce).is_ok()
            && active.world.check_approval(second_xuid, &second_nonce).is_ok();
        let mut closing = active.begin_close(Instant::now() + ACQUISITION_TIMEOUT);
        let closed = closing.finish_close().await;
        drop(closing);
        let observed = if completed {
            Some(peer.finish_activation_visitors().await)
        } else {
            peer.abort_activation().await;
            None
        };
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let world_running = world.manager.status().state == bridge::WorldState::Running;
        invited.expect("original account activity-bound invitation");
        let (first, second) = result.expect("actual production Active.run visitor flow failed");
        closed.expect("original active host physical/process/membership cleanup");
        assert!(cleanup_empty && world_running && idle && approvals);
        let observed = observed.unwrap();
        assert_eq!(observed.activation.max_members, cap.get());
        assert_eq!(observed.activation.invited_xuid, first_xuid);
        assert_eq!(observed.activation.created_properties["SupportedConnections"][0]["PmsgId"],
            serde_json::json!(host_messaging_id));
        assert_eq!(observed.member_counts, [2, 3, 2, 1]);
        assert_eq!(observed.activation.rta, HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof));
        for terminal in std::iter::once(observed.activation.messaging).chain(observed.guests) {
            assert!(matches!(terminal, HostedSocketTerminal::Close | HostedSocketTerminal::Eof));
        }
        for (packs, game, terrain) in [first, second] {
            assert!(game.world.local_player_runtime_id != 0 && terrain);
            assert!(packs.iter().any(|pack| pack.pack_id == world.pack && pack.archive == world.archive),
                "original hash-verified final resource snapshot was lost");
        }
    })).await.expect("finite original Active.run Messaging/world control stalled");
}
