//! Actual service credential expiry selects renewal in the public active owner, without game visitors.

use super::*;

/// The original healthy service owner renews its actual expiring lease from public run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_run_renews_same_key_at_actual_service_credential_expiry() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, Box::pin(async move {
        let namespace = tempfile::tempdir().unwrap();
        let cap = NonZeroU32::new(
            world.manager.status().max_players.unwrap().min(
                u32::try_from(super::super::super::VISITOR_LIMIT)
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
        } = HostedAuthority::acquire_activation_expiring_renewal(
            authentication_policy(),
            config,
            cap,
            bridge::contract::PRESENCE_TITLE_ID.to_string(),
            MINECRAFT_AUDIENCE.into(),
        )
        .await;
        let jwks = issuer_jwks(&peer);
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
            .expect("original public activation with actual short TURN lease");
        let original_expiry = active.expires;
        let original_key = active.identity.signing_key();
        let original_public = original_key
            .verifying_key()
            .to_public_key_der()
            .unwrap()
            .as_bytes()
            .to_vec();
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
        let (committed, observed) = oneshot::channel();
        active.renewal_committed = Some(committed);
        let renewed = {
            let running = active.run();
            tokio::pin!(running);
            tokio::time::timeout_at((original_expiry + ACQUISITION_TIMEOUT).into(), async {
                tokio::select! { biased;
                    result = &mut running => result.and(Err(io::Error::other("active host ended before expiry renewal"))),
                    result = observed => result.map_err(|_| unavailable()),
                }
            })
            .await
        };
        let elapsed = Instant::now() >= original_expiry;
        let advanced = active.expires > original_expiry;
        let same_key = Arc::ptr_eq(&original_key, &active.identity.signing_key());
        let healthy = !active.phase_started && active.renewal_failure.is_none();
        let mut closing = active.begin_close(Instant::now() + ACQUISITION_TIMEOUT);
        let closed = closing.finish_close().await;
        drop(closing);
        let observed = if matches!(&renewed, Ok(Ok(()))) && closed.is_ok() {
            Some(peer.finish_activation_renewal().await)
        } else {
            peer.abort_activation().await;
            None
        };
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let running = world.manager.status().state == bridge::WorldState::Running;
        drop((original_key, account, world));

        invited.expect("original active-owner invitation");
        renewed.expect("actual lease expiry did not complete renewal").expect("public active run failed");
        closed.expect("original host retirement after actual renewal");
        assert!(elapsed && advanced && same_key && healthy && cleanup_empty && running);
        let observed = observed.expect("original renewal fixture completed before retirement");
        assert_eq!(observed.activation.minted_public_key, original_public);
        assert_eq!(observed.renewed_public_key, original_public);
        assert_ne!(observed.original_turn_request, observed.refreshed_turn_request);
        assert_eq!(observed.activation.max_members, cap.get());
        assert_eq!(observed.activation.invited_xuid, VISITOR_XUID);
        assert_eq!(observed.activation.rta, HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof));
        assert!(matches!(observed.activation.messaging, HostedSocketTerminal::Close | HostedSocketTerminal::Eof));
    }))
    .await
    .expect("finite actual-expiry active host fixture stalled");
}
