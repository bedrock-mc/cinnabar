//! Original acknowledged nonce authority across actual RTA resync and signed HTTP 304.

use super::*;

/// A complete unchanged service refresh preserves original approvals and their exact owner binding.
#[tokio::test]
async fn original_approval_survives_actual_resync_and_unchanged_304() {
    tokio::time::timeout(crate::native_account::ACQUISITION_TIMEOUT, async {
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
        } = HostedAuthority::acquire_unchanged(authentication_policy(), config).await;
        let store = AccountStore::new(namespace.path().join("synthetic-oauth.json"));
        let checkpoint = services.checkpoint().unwrap();
        let credentials = services.credentials();
        let expiry = oauth_expiry(credentials.expires_at()).unwrap();
        let pending = transaction("reserve hosted proof fixture sign-in", || {
            store.try_begin_native_sign_in("host-proof-generation", None)
        });
        transaction("publish hosted proof fixture OAuth", || {
            pending.try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &expiry,
            )
        });
        transaction("publish hosted proof fixture checkpoint", || {
            pending.try_publish_checkpoint(checkpoint.as_bytes())
        });
        transaction("commit hosted proof fixture sign-in", || {
            pending.try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
        });
        let account = Arc::new(NativeAccount::new(
            store.clone(),
            namespace.path().join("synthetic-device.json"),
            Some(identity.xuid().into()),
        ));
        install(&account, services, &identity);
        let deadline = Instant::now() + crate::native_account::ACQUISITION_TIMEOUT;
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
                Approvals::new(identity.xuid().into(), directory.opening.max_players()).unwrap(),
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
        let approval = host
            .check_approval("456", &nonce)
            .expect("actual original host approval");
        host.recheck_approval(&approval)
            .expect("actual original host recheck");
        let original_custom = host
            .directory
            .as_ref()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .description()
            .properties
            .as_ref()
            .unwrap()
            .custom
            .clone();
        peer.resync_unchanged_ready(host.directory.as_mut().unwrap().socket.as_mut().unwrap())
            .await;
        let refreshed = host.drive().await;
        let original_rechecked = host.recheck_approval(&approval).is_ok();
        let original_admitted = host.check_approval("456", &nonce).is_ok();
        let same_custom = host
            .directory
            .as_ref()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .description()
            .properties
            .as_ref()
            .unwrap()
            .custom
            == original_custom;
        // Preserve every result until original conditional leave and both finite peers retire.
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
            refreshed.is_ok(),
            "original RTA/signed 304 refresh failed: {refreshed:?}"
        );
        assert!(
            same_custom,
            "304 changed the complete original acknowledged document"
        );
        assert!(
            original_rechecked && original_admitted,
            "unchanged signed 304 refresh revoked the original acknowledged approval"
        );
    })
    .await
    .unwrap();
}
