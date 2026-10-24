use super::*;
use crate::{
    local_worlds::test_support::NativeHostFixture,
    native_account::{Cached, Owner, authentication_policy, metadata, oauth_expiry},
};
use bedrock_services::{
    minecraft,
    test_support::{HostedAuthority, HostedMembership},
};
use launcher::accounts::{AccountProfile, AccountStore};

pub(in crate::native_account::hosted_world) mod activation;
mod contention;
mod dispatch;
pub(in crate::native_account::hosted_world) mod external;
mod unchanged;

/// Completes only nonblocking fixture storage transactions, retaining temporary lease contention.
fn transaction<T>(what: &str, mut perform: impl FnMut() -> io::Result<T>) -> T {
    let mut completed = None;
    test_time::eventually(what, || match perform() {
        Ok(value) => {
            completed = Some(value);
            true
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("{what}: {error}"),
    });
    completed.expect("transaction completed")
}

/// Installs the actual acquired account through existing lease, identity verification and persistence owners.
fn install(
    account: &Arc<NativeAccount>,
    services: minecraft::Minecraft,
    identity: &bedrock_services::xbox::Authorization,
) {
    let mut operation = account.begin().unwrap();
    let lease = transaction("acquire hosted proof fixture lease", || {
        account.store.try_native_lease()
    });
    let previous = operation.cached.take().and_then(|cached| cached.identity);
    operation.cached = Some(Cached {
        provider: None,
        generation: lease.generation().into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Minecraft(Box::new(services)),
        identity: previous,
    });
    operation.lease = Some(lease);
    operation
        .verify_identity(identity.xuid(), identity.gamertag())
        .unwrap();
    operation.persist().unwrap();
}

/// Rejects a replacement proof at both immediate approval boundaries while preserving original checkpoint ownership.
#[tokio::test]
async fn approval_rejects_a_fresh_same_generation_proof_but_accepts_the_original_checkpoint() {
    tokio::time::timeout(super::super::super::ACQUISITION_TIMEOUT, async {
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
        let deadline = Instant::now() + super::super::super::ACQUISITION_TIMEOUT;
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
