//! A held actual subscription acknowledgment exercises the public activation caller's shorter cutoff.

use super::*;

/// A longer original opening cannot extend the cutoff supplied to public activation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_activation_keeps_shorter_caller_cutoff_during_actual_rta_subscription() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(
        ACQUISITION_TIMEOUT * 2,
        Box::pin(async move {
            let namespace = tempfile::tempdir().unwrap();
            let cap = NonZeroU32::new(
                world.manager.status().max_players.unwrap().min(
                    u32::try_from(crate::native_account::hosted_world::active::VISITOR_LIMIT)
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
            let (
                HostedAuthority {
                    account: services,
                    identity,
                    peer,
                },
                mut hold,
            ) = HostedAuthority::acquire_activation_held_subscription(
                authentication_policy(),
                config,
                cap,
                bridge::contract::PRESENCE_TITLE_ID.to_string(),
                nethernet::identity::MINECRAFT_AUDIENCE.into(),
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
            let original_deadline = Instant::now() + ACQUISITION_TIMEOUT;
            let original = world
                .manager
                .native_hosted_world(Arc::clone(&account), original_deadline)
                .unwrap();
            let shorter = ACQUISITION_TIMEOUT / 6;
            let caller_deadline = Instant::now() + shorter;
            let observation_deadline = caller_deadline + shorter;
            let mut requested = false;
            let result = {
                let activating = original.activate(caller_deadline);
                tokio::pin!(activating);
                tokio::time::timeout_at(observation_deadline.into(), async {
                    tokio::select! { biased;
                        _ = hold.requested() => {
                            requested = true;
                            activating.await
                        }
                        result = &mut activating => result,
                    }
                })
                .await
            };
            let error = match result {
                Ok(Err(error)) => Some(error),
                Ok(Ok(active)) => {
                    let mut closing = active.begin_close(Instant::now() + ACQUISITION_TIMEOUT);
                    let _ = closing.finish_close().await;
                    drop(closing);
                    None
                }
                Err(_) => None,
            };
            // The held handler is canceled and joined before its release can send a late acknowledgment.
            let prefix = peer.abort_activation().await;
            drop(hold);
            let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
            let running = world.manager.status().state == bridge::WorldState::Running;
            drop((account, world));

            assert!(
                requested,
                "activation never reached the actual protected RTA subscription"
            );
            VerifyingKey::from_public_key_der(&prefix.minted_public_key)
                .expect("actual original mint request before subscription");
            assert!(prefix.verified_http_requests > 0 && cleanup_empty && running);
            assert_eq!(
                error.as_ref().map(io::Error::kind),
                Some(io::ErrorKind::TimedOut),
                "public activation outlived its shorter caller cutoff: {error:?}"
            );
        }),
    )
    .await
    .expect("finite public activation cutoff fixture stalled");
}
