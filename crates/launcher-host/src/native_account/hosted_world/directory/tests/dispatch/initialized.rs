//! Original service authority at Initialized, after an actual current-member Attach and World.

use super::*;
use bridge::host_world::contract::Initialized;

#[tokio::test]
async fn revoked_original_member_is_denied_before_backend_initialized_dispatch() {
    tokio::time::timeout(
        super::super::super::super::super::ACQUISITION_TIMEOUT,
        async {
            let fixture = NativeHostFixture::new();
            let namespace = tempfile::tempdir().unwrap();
            let config = minecraft::Config::desktop(
                protocol::GAME_VERSION,
                u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
                metadata::service_device_id("123").unwrap(),
            );
            let HostedAuthority {
                account: services,
                identity,
                mut peer,
            } = HostedAuthority::acquire_revocation(authentication_policy(), config).await;
            let store = AccountStore::new(namespace.path().join("synthetic-oauth.json"));
            let checkpoint = services.checkpoint().unwrap();
            let credentials = services.credentials();
            let expiry = oauth_expiry(credentials.expires_at()).unwrap();
            let pending = transaction("reserve initialized fixture sign-in", || {
                store.try_begin_native_sign_in("host-initialized-generation", None)
            });
            transaction("publish initialized fixture OAuth", || {
                pending.try_publish(
                    credentials.access_token(),
                    credentials.refresh_token(),
                    &expiry,
                )
            });
            transaction("publish initialized fixture checkpoint", || {
                pending.try_publish_checkpoint(checkpoint.as_bytes())
            });
            transaction("commit initialized fixture sign-in", || {
                pending.try_commit(
                    &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
                )
            });
            let account = Arc::new(NativeAccount::new(
                store,
                namespace.path().join("synthetic-device.json"),
                Some(identity.xuid().into()),
            ));
            install(&account, services, &identity);
            let deadline = Instant::now() + super::super::super::super::super::ACQUISITION_TIMEOUT;
            let mut host = fixture
                .manager
                .native_hosted_world(Arc::clone(&account), deadline)
                .unwrap();
            {
                let directory = host.directory.as_mut().unwrap();
                let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
                let HostedMembership {
                    socket,
                    subscription,
                    binding,
                    session,
                } = peer
                    .host(
                        operation.minecraft().unwrap(),
                        &directory.reference,
                        directory.session_subscription,
                        directory.scope.as_ref().unwrap().rta_generation().unwrap(),
                        "456",
                    )
                    .await;
                directory.socket = Some(socket);
                directory.subscription = Some(subscription);
                directory.subscribed = true;
                directory.binding = Some(binding);
                directory.session = Some(session);
                directory.host_name = identity.gamertag().into();
                directory.approvals = Some(
                    Approvals::new(identity.xuid().into(), directory.opening.max_players())
                        .unwrap(),
                );
                directory.observe().unwrap();
                let approvals = directory.approvals.as_mut().unwrap();
                for owner in approvals.missing() {
                    approvals.issue(&owner, &[7; NONCE_BYTES]).unwrap();
                }
                let revision = approvals.revision();
                directory.pending = Some(ApprovalWrite {
                    revision,
                    document: directory.properties().unwrap(),
                });
                peer.set_properties_bound(
                    operation.minecraft().unwrap(),
                    directory.session.as_mut().unwrap(),
                    binding,
                    &directory.pending.as_ref().unwrap().document,
                )
                .await;
                directory.observe().unwrap();
                directory.confirm_pending().unwrap();
                operation.persist().unwrap();
            }
            let nonce = host
                .directory
                .as_ref()
                .unwrap()
                .approvals
                .as_ref()
                .unwrap()
                .document()
                .unwrap()["456"]
                .as_str()
                .unwrap()
                .to_owned();
            let approval = host.check_approval("456", &nonce).unwrap();
            host.recheck_approval(&approval).unwrap();

            let pending = PendingHostProcess::bind(
                &[7; CAPABILITY_BYTES as usize],
                &[8; OPENING_BYTES as usize],
            )
            .await
            .unwrap();
            let bootstrap = Bootstrap {
                address: pending.bootstrap().address.clone(),
                opening: pending.bootstrap().opening.clone(),
                capability: pending.bootstrap().capability.clone(),
            };
            let child = backend_process(&bootstrap);
            let application = async {
                let mut process = pending.establish(deadline).await.unwrap();
                let backend = process
                    .prepare_open(
                        Identity {
                            xuid: approval.xuid.clone(),
                            uuid: Uuid::from_u128(1).to_string(),
                            display_name: "Fixture".into(),
                            playfab_id: String::new(),
                            playfab_title_id: String::new(),
                        },
                        serde_json::value::RawValue::from_string("{}".into()).unwrap(),
                        deadline,
                    )
                    .await
                    .unwrap()
                    .begin_open()
                    .unwrap()
                    .finish()
                    .await
                    .unwrap();
                let backend = host
                    .attach_backend(&approval, backend, deadline)
                    .await
                    .unwrap();
                assert_eq!(backend.world(), b"bounded-world-output");
                host.recheck_approval(&approval).unwrap();
                peer.revoke_member_ready(host.directory.as_mut().unwrap().socket.as_mut().unwrap())
                    .await;
                // The real event is retained on the original socket until service progress consumes it.
                host.recheck_approval(&approval).unwrap();
                let result = host
                    .initialize_backend(&approval, backend, false, 1, deadline)
                    .await;
                let rejected = result.is_err();
                drop(result);
                process.close(deadline).await.unwrap();
                rejected
            };
            let (rejected, dispatched) = tokio::join!(application, child);
            let revoked = host.recheck_approval(&approval).is_err();
            {
                let directory = host.directory.as_mut().unwrap();
                let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
                peer.leave_bound(
                    operation.minecraft().unwrap(),
                    directory.session.as_mut().unwrap(),
                    directory.binding.unwrap(),
                )
                .await;
                operation.persist().unwrap();
                peer.finish(directory.socket.as_mut().unwrap()).await;
            }
            host.close(deadline).await.unwrap();
            assert!(fixture.manager.native_host_cleanup().unwrap().is_none());
            assert!(!fixture.runner.last().stopped());
            assert!(
                rejected && revoked,
                "actual original membership was not revoked"
            );
            assert!(
                !dispatched,
                "revoked original membership dispatched backend Initialized"
            );
        },
    )
    .await
    .unwrap();
}

/// Records only the real Initialized frame or the actual peer EOF after its World response.
async fn backend_process(bootstrap: &Bootstrap) -> bool {
    let (mut control, mut output) = child_connection(bootstrap, 0).await;
    send_json(
        &mut output,
        Kind::Snapshot,
        &Snapshot {
            protocol_version: u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            max_players: MAX_PLAYERS,
            max_chunk_radius: 1,
            required: false,
            packs: Vec::new(),
        },
    )
    .await;
    let frame = control.recv().await.unwrap().unwrap();
    assert_eq!(frame.kind, Kind::Open);
    let open: bridge::host_world::contract::Open = serde_json::from_slice(&frame.body).unwrap();
    let (mut peer, mut output_peer) = child_connection(bootstrap, open.peer).await;
    send_json(
        &mut output,
        Kind::Opened,
        &PeerResult {
            peer: open.peer,
            code: 0,
        },
    )
    .await;
    assert_eq!(peer.recv().await.unwrap().unwrap().kind, Kind::Attach);
    output_peer
        .admit(Kind::World, b"bounded-world-output")
        .unwrap();
    output_peer.flush().await.unwrap();
    let dispatched = match peer.recv().await.unwrap() {
        Some(frame) => {
            assert_eq!(frame.kind, Kind::Initialized);
            let initialized: Initialized = serde_json::from_slice(&frame.body).unwrap();
            assert!(!initialized.cache_enabled);
            assert_eq!(initialized.radius, 1);
            assert!(peer.recv().await.unwrap().is_none());
            true
        }
        None => false,
    };
    assert_eq!(control.recv().await.unwrap().unwrap().kind, Kind::Close);
    output.admit(Kind::Closed, &[]).unwrap();
    output.flush().await.unwrap();
    dispatched
}
