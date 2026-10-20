//! Real shared-account contention around the original signed directory/IO owner.

use super::*;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

/// Normal account work retains this host and its exact IO result; original proof replacement still rejects.
#[tokio::test]
async fn original_host_io_survives_unrelated_account_operation() {
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
        } = HostedAuthority::acquire(authentication_policy(), config).await;
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
        // A real unrelated verified operation has temporarily moved the original cached SDK owner.
        let contender = Arc::clone(&account).acquire_verified().await.unwrap();
        let mut calls = 0;
        let mut selected = Box::pin(host.peer_io(Some(deadline), async {
            calls += 1;
            Ok(17u8)
        }));
        let initial = selected
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        let retained_while_busy = initial.is_pending();
        drop(contender);
        let first_result = match initial {
            Poll::Ready(result) => result,
            Poll::Pending => selected.as_mut().await,
        };
        drop(selected);
        let first_preserved = first_result.is_ok_and(|value| value == 17) && calls == 1;

        // A different operation can begin in the same turn the actual IO completes.
        let contender = std::cell::RefCell::new(None);
        let mut calls = 0;
        let mut selected = Box::pin(host.peer_io(Some(deadline), async {
            calls += 1;
            *contender.borrow_mut() = Some(account.begin().unwrap());
            Ok(23u8)
        }));
        let initial = selected
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        let completed_result_retained = initial.is_pending();
        drop(contender.borrow_mut().take());
        let second_result = match initial {
            Poll::Ready(result) => result,
            Poll::Pending => selected.as_mut().await,
        };
        drop(selected);
        let second_preserved = second_result.is_ok_and(|value| value == 23) && calls == 1;
        let unchanged_approval = host.recheck_approval(&approval).is_ok();
        let original_checkpoint = {
            let lease = transaction("read original hosted proof checkpoint", || {
                store.try_native_lease()
            });
            bedrock_services::xbox::Checkpoint::from_bytes(lease.checkpoint().unwrap().unwrap())
                .unwrap()
        };
        let (fresh, fresh_identity) = peer.fresh_account().await;
        assert_eq!(fresh_identity.xuid(), identity.xuid());
        assert!(
            !fresh.owns_rta(
                host.directory
                    .as_ref()
                    .unwrap()
                    .socket
                    .as_ref()
                    .unwrap()
                    .connection()
            )
        );
        install(&account, fresh, &fresh_identity);
        // Retain actual results until exact original remote cleanup finishes, including in the red run.
        let replacement_admitted = host.check_approval("456", &nonce).is_ok();
        let replacement_rechecked = host.recheck_approval(&approval).is_ok();
        let (restored, restored_identity) = peer.restore_account(original_checkpoint).await;
        assert_eq!(restored_identity.xuid(), identity.xuid());
        assert!(
            restored.owns_rta(
                host.directory
                    .as_ref()
                    .unwrap()
                    .socket
                    .as_ref()
                    .unwrap()
                    .connection()
            )
        );
        install(&account, restored, &restored_identity);
        host.check_approval("456", &nonce)
            .expect("restored original checkpoint approval");
        host.recheck_approval(&approval)
            .expect("restored original checkpoint recheck");
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
            retained_while_busy && first_preserved,
            "unrelated account contention retired the original host IO instead of retaining it"
        );
        assert!(
            completed_result_retained && second_preserved,
            "account contention discarded an already completed original host IO result"
        );
        assert!(
            unchanged_approval,
            "the original approval changed during account contention"
        );
        assert!(
            !replacement_admitted,
            "a fresh same-generation proof authorized an original hosted approval"
        );
        assert!(
            !replacement_rechecked,
            "a fresh same-generation proof rechecked an original hosted approval"
        );
    })
    .await
    .unwrap();
}
