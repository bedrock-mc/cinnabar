//! Same original public host, acknowledged directory and actual Dragonfly backend through RTA loss.

use super::*;
use crate::native_account::hosted_world::directory::tests::activation::{
    interrupt_rta, same_host_member,
};
use bridge::host_world::{ActiveBackendPeer, contract::Identity};

/// A physical subscription replacement must not discard the original backend receive continuation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_host_rta_reconnect_preserves_the_original_backend_and_approval() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(
        ACQUISITION_TIMEOUT * 2,
        Box::pin(async {
            let namespace = tempfile::tempdir().unwrap();
            let cap = NonZeroU32::new(
                world.manager.status().max_players.unwrap().min(
                    u32::try_from(VISITOR_LIMIT)
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
                mut peer,
            } = HostedAuthority::acquire_activation(
                authentication_policy(),
                config,
                cap,
                bridge::contract::PRESENCE_TITLE_ID.to_string(),
                MINECRAFT_AUDIENCE.into(),
            )
            .await;
            let mut recovery = peer.hold_host_rta_reconnect();
            let jwks = serde_json::to_vec(&issuer_jwks(&peer)).unwrap();
            let account =
                install_activation_account(namespace.path(), services, &identity, &jwks).await;
            let deadline = Instant::now() + ACQUISITION_TIMEOUT;
            let mut active = world
                .manager
                .native_hosted_world(Arc::clone(&account), deadline)
                .unwrap()
                .activate(deadline)
                .await
                .expect("actual original host activation");
            let nonce = activation_nonce(&active.world, VISITOR_XUID);
            let original_approval = active.world.check_approval(VISITOR_XUID, &nonce).unwrap();
            active.world.recheck_approval(&original_approval).unwrap();
            let settings = LoginSettings {
                view_distance_chunks: 1,
                ..Default::default()
            };
            let random_id = ClientRandomId::new(1).unwrap();
            let mut claims = protocol::login_client_data(VISITOR_NAME, random_id, None, &settings);
            protocol::NativeDeviceProfile::fresh()
                .apply(&mut claims, random_id, VISITOR_NAME)
                .unwrap();
            let data =
                serde_json::value::RawValue::from_string(serde_json::to_string(&claims).unwrap())
                    .unwrap();
            let HostedBackend::Dragonfly(process) = &mut active.process else {
                panic!("original Dragonfly fixture selected another backend");
            };
            let backend = active
                .world
                .open_backend(
                    &original_approval,
                    &mut process.process,
                    Identity {
                        xuid: VISITOR_XUID.into(),
                        uuid: "00000000-0000-4000-8000-000000000001".into(),
                        display_name: VISITOR_NAME.into(),
                        playfab_id: String::new(),
                        playfab_title_id: String::new(),
                    },
                    data,
                    deadline,
                )
                .await
                .unwrap();
            let backend = active
                .world
                .attach_backend(&original_approval, backend, deadline)
                .await
                .unwrap();
            protocol::NativeHostWorld::parse(backend.world())
                .expect("actual original typed world output");
            let mut backend = active
                .world
                .initialize_backend(&original_approval, backend, false, 1, deadline)
                .await
                .unwrap();
            let prefix = active
                .world
                .peer_io(Some(deadline), backend.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                !prefix.is_empty(),
                "original initialized backend never produced a packet"
            );
            backend.received(deadline).await.unwrap();

            interrupt_rta(&mut active.world, &mut recovery).await;
            let unavailable_during_loss =
                active.world.check_approval(VISITOR_XUID, &nonce).is_err()
                    && active.world.recheck_approval(&original_approval).is_err();
            let received = {
                let original_recv = backend.recv();
                tokio::pin!(original_recv);
                active
                    .world
                    .peer_io(Some(deadline), &mut original_recv)
                    .await
            };
            let resumed = received
                .as_ref()
                .is_ok_and(|body| body.as_ref().is_some_and(|body| !body.is_empty()));
            if resumed {
                backend.received(deadline).await.unwrap();
            }
            let rebound = if resumed {
                recovery.rebound().await
            } else {
                false
            };
            let fresh_approval = active.world.check_approval(VISITOR_XUID, &nonce).is_ok();
            let original_member = same_host_member(&active.world, &original_approval);
            let original_peer_authority = active.world.recheck_approval(&original_approval).is_ok();
            let cleanup_deadline = Instant::now() + ACQUISITION_TIMEOUT;
            let backend_closed = close_backend(backend, cleanup_deadline).await;
            recovery.stop();
            let mut closing = active.begin_close(cleanup_deadline);
            let closed = closing.finish_close().await;
            drop(closing);
            let observed = peer.finish_activation().await;
            let facts = recovery.observation();
            let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
            let world_running = world.manager.status().state == bridge::WorldState::Running;
            drop((account, world));

            backend_closed.expect("original backend close/accepted-prefix drain");
            closed.expect("original host listener/process/membership retirement");
            assert!(cleanup_empty && world_running && facts.first_retired);
            assert_eq!((facts.publications, facts.leaves), (1, 1));
            assert!(
                matches!(
                    observed.rta,
                    HostedSocketTerminal::Close
                        | HostedSocketTerminal::Eof
                        | HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof)
                ),
                "the last accepted original or replacement RTA socket did not retire"
            );
            assert!(matches!(
                observed.messaging,
                HostedSocketTerminal::Close | HostedSocketTerminal::Eof
            ));
            assert!(
                unavailable_during_loss,
                "unconfirmed original RTA still admitted a cached approval"
            );
            assert!(
                resumed && rebound && original_member && fresh_approval && original_peer_authority,
                "original host backend/approval did not recover after physical RTA loss"
            );
            assert_eq!(
                (facts.upgrades, facts.acknowledgments, facts.rebinds),
                (2, 2, 1)
            );
            assert!(facts.synchronized);
        }),
    )
    .await
    .expect("finite original hosted RTA recovery control stalled");
}

/// Drains only original accepted backend packets while completing its consuming close owner.
async fn close_backend(backend: ActiveBackendPeer, deadline: Instant) -> io::Result<()> {
    let mut closing = backend.begin_close(deadline);
    loop {
        match closing.finish_close().await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                closing.recv().await?.ok_or_else(ended)?;
                closing.received().await?;
            }
            Err(error) => return Err(error),
        }
    }
}

mod approval;
