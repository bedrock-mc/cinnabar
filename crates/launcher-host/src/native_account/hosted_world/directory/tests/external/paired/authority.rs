//! Original finite signed account/membership fixture bound to the actual running world opening.

use super::super::*;
use bedrock_services::test_support::{HOSTED_WORLD_ISSUER, HostedPeer};
use std::{path::Path, time::SystemTime};

pub(super) struct Authority {
    pub(super) host: NativeHostedWorld,
    pub(super) nonce: String,
    pub(super) service_time: SystemTime,
    pub(super) service_observed: Instant,
    account: Arc<NativeAccount>,
    peer: HostedPeer,
}

impl Authority {
    /// Reuses real signed fixture exchanges, normal account installation and actual acknowledged nonce.
    pub(super) async fn open(
        manager: &crate::local_worlds::Manager,
        directory: &Path,
        jwks: &[u8],
        deadline: Instant,
    ) -> Self {
        Self::open_with_visitors(manager, directory, jwks, deadline, false).await
    }

    /// Reuses the same account install, signed binding and acknowledged document for two real members.
    pub(super) async fn open_two_visitors(
        manager: &crate::local_worlds::Manager,
        directory: &Path,
        jwks: &[u8],
        deadline: Instant,
    ) -> Self {
        Self::open_with_visitors(manager, directory, jwks, deadline, true).await
    }

    /// Keeps the finite SDK scenario choice outside every original account and approval operation.
    async fn open_with_visitors(
        manager: &crate::local_worlds::Manager,
        directory: &Path,
        jwks: &[u8],
        deadline: Instant,
        two_visitors: bool,
    ) -> Self {
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedAuthority {
            account: services,
            identity,
            mut peer,
        } = if two_visitors {
            HostedAuthority::acquire_world_two_visitors(authentication_policy(), config).await
        } else {
            HostedAuthority::acquire_world(authentication_policy(), config).await
        };
        let store = AccountStore::new(directory.join("synthetic-oauth.json"));
        let checkpoint = services.checkpoint().unwrap();
        let credentials = services.credentials();
        let expiry = oauth_expiry(credentials.expires_at()).unwrap();
        let pending = transaction("reserve paired host fixture sign-in", || {
            store.try_begin_native_sign_in("paired-host-generation", None)
        });
        transaction("publish paired host fixture OAuth", || {
            pending.try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &expiry,
            )
        });
        transaction("publish paired host fixture checkpoint", || {
            pending.try_publish_checkpoint(checkpoint.as_bytes())
        });
        transaction("commit paired host fixture sign-in", || {
            pending.try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
        });
        let account = Arc::new(NativeAccount::new(
            store,
            directory.join("synthetic-device.json"),
            Some(identity.xuid().into()),
        ));
        install(&account, services, &identity);
        let mut host = manager
            .native_hosted_world(Arc::clone(&account), deadline)
            .unwrap();
        let service_time;
        let service_observed;
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
                    super::provider::VISITOR_XUID,
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
            directory.pending = Some(ApprovalWrite {
                revision: approvals.revision(),
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
            let services = operation.minecraft().unwrap();
            services.discover().await.unwrap();
            let issuer = services.authorization_issuer().unwrap().to_owned();
            assert_eq!(issuer, HOSTED_WORLD_ISSUER);
            service_observed = Instant::now();
            service_time = services.service_time();
            operation.cached.as_mut().unwrap().provider = Some(
                protocol::test_support::hosted_provider(
                    &issuer,
                    minecraft::USER_AGENT,
                    jwks,
                    Instant::now(),
                )
                .unwrap(),
            );
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
            .unwrap()[super::provider::VISITOR_XUID]
            .as_str()
            .unwrap()
            .to_owned();
        host.check_approval(super::provider::VISITOR_XUID, &nonce)
            .unwrap();
        Self {
            host,
            nonce,
            service_time,
            service_observed,
            account,
            peer,
        }
    }

    /// Finishes the unchanged finite revocation/conditional-leave script after real peer/IPC retirement.
    pub(super) async fn finish(mut self, deadline: Instant) {
        self.peer
            .revoke_member_ready(
                self.host
                    .directory
                    .as_mut()
                    .unwrap()
                    .socket
                    .as_mut()
                    .unwrap(),
            )
            .await;
        self.host
            .directory
            .as_mut()
            .unwrap()
            .service_barrier(Some(deadline))
            .await
            .unwrap();
        let revoked = self
            .host
            .check_approval(super::provider::VISITOR_XUID, &self.nonce)
            .is_err();
        {
            let directory = self.host.directory.as_mut().unwrap();
            let mut operation = Arc::clone(&self.account).acquire_verified().await.unwrap();
            self.peer
                .leave_bound(
                    operation.minecraft().unwrap(),
                    directory.session.as_mut().unwrap(),
                    directory.binding.unwrap(),
                )
                .await;
            operation.persist().unwrap();
            self.peer.finish(directory.socket.as_mut().unwrap()).await;
        }
        self.host.close(deadline).await.unwrap();
        assert!(
            revoked,
            "actual paired host cleanup did not observe original membership revocation"
        );
    }
}
