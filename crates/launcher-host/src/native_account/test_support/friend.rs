//! A sealed acquired-account scenario for ordinary application friend admission.
//! Presenter attachment is qualified fixture setup, not an AuthSupervisor restoration proof.

mod issuer;
pub mod listener;

use super::super::{read_identity::ReadIdentity, *};
use bedrock_services::test_support::{
    HOSTED_WORLD_ISSUER, HostedAuthority, HostedFriend, HostedFriendDialect, HostedFriendHost,
    HostedFriendMembership, HostedPeer, HostedSocketTerminal,
};
use launcher::accounts::{AccountProfile, AccountStore};
use std::{path::PathBuf, time::Instant};

/// Owns the actual acquired SDK account, selected store and finite service peers.
/// No constructor accepts an arbitrary account, selection, nonce or service endpoint.
pub struct Fixture {
    account: Arc<NativeAccount>,
    identity: ReadIdentity,
    peer: HostedPeer,
    host: Option<HostedFriendHost>,
    membership: Option<HostedFriendMembership>,
    issuer: issuer::Issuer,
    service_time: std::time::SystemTime,
    observed: Instant,
    path: PathBuf,
    deadline: Instant,
}

/// Original verified presentation input, constructible only from this acquired fixture.
pub struct Presentation {
    account: Arc<NativeAccount>,
    profile: AccountProfile,
    identity: ReadIdentity,
    path: PathBuf,
}

/// Coarse service facts projected only after the original SDK peer tasks have joined.
pub struct Observation {
    /// Actual original handle joins.
    pub joins: usize,
    /// Actual conditional member removals.
    pub leaves: usize,
    /// Whether the real nonce-bearing snapshot flushed.
    pub nonce_published: bool,
    /// Whether the original multiplayer mint carried a connection proof key.
    pub minted: bool,
    /// Whether protected client signaling was upgraded.
    pub signaling_started: bool,
    /// The client socket retired through its original abort/EOF or Close; the host closed cleanly.
    pub signaling_retired: bool,
    /// The original held RTA socket observed its expected local-abort EOF.
    pub rta_retired: bool,
}

impl Presentation {
    /// Rechecks the original generation and selection after asynchronous fixture setup.
    pub fn is_current(&self) -> bool {
        self.identity.current()
    }

    /// Transfers the same checked owner and selected profile; this grants no friend selection.
    pub fn into_parts(self) -> (Arc<NativeAccount>, AccountProfile, PathBuf) {
        (self.account, self.profile, self.path)
    }
}

impl Fixture {
    /// Acquires the fixed Messaging sibling through the original signed service protocol.
    pub async fn messaging(path: PathBuf, device: PathBuf, deadline: Instant) -> Self {
        Self::acquire(path, device, HostedFriendDialect::Messaging, deadline).await
    }

    /// Acquires the fixed Legacy sibling without changing the client's random local network ID.
    pub async fn legacy(path: PathBuf, device: PathBuf, deadline: Instant) -> Self {
        Self::acquire(path, device, HostedFriendDialect::Legacy, deadline).await
    }

    /// Uses the original account transaction, protected SDK checkout and identity persistence.
    async fn acquire(
        path: PathBuf,
        device: PathBuf,
        dialect: HostedFriendDialect,
        deadline: Instant,
    ) -> Self {
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            uuid::Uuid::new_v4(),
        );
        let HostedFriend {
            authority:
                HostedAuthority {
                    account: services,
                    identity,
                    peer,
                },
            host,
            membership,
        } = HostedAuthority::acquire_friend(
            authentication_policy(),
            config,
            dialect,
            nethernet::identity::MINECRAFT_AUDIENCE.into(),
            deadline,
        )
        .await;
        let issuer = issuer::Issuer::new(&peer);
        let service_time = services.service_time();
        let observed = Instant::now();
        let checkpoint = services.checkpoint().unwrap();
        let store = AccountStore::new(path.clone());
        let credentials = services.credentials();
        let expiry = oauth_expiry(credentials.expires_at()).unwrap();
        let pending = store
            .try_begin_native_sign_in("controlled-friend-generation", None)
            .unwrap();
        pending
            .try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &expiry,
            )
            .unwrap();
        pending
            .try_publish_checkpoint(checkpoint.as_bytes())
            .unwrap();
        pending
            .try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
            .unwrap();
        let account = Arc::new(NativeAccount::new(
            store,
            device,
            Some(identity.xuid().into()),
        ));
        let captured = {
            let mut operation = account.begin().unwrap();
            let lease = account.store.try_native_lease().unwrap();
            operation.cached = Some(Cached {
                generation: lease.generation().into(),
                binding: lease.binding(),
                checkpoint_revision: lease.checkpoint_revision(),
                owner: Owner::Minecraft(Box::new(services)),
                identity: None,
                provider: Some(
                    protocol::test_support::hosted_provider(
                        HOSTED_WORLD_ISSUER,
                        minecraft::USER_AGENT,
                        &issuer.jwks,
                        observed,
                    )
                    .unwrap(),
                ),
            });
            operation.lease = Some(lease);
            operation
                .verify_identity(identity.xuid(), identity.gamertag())
                .unwrap();
            operation.persist().unwrap();
            ReadIdentity::capture(&operation)
        };
        Self {
            account,
            identity: captured,
            peer,
            host: Some(host),
            membership: Some(membership),
            issuer,
            service_time,
            observed,
            path,
            deadline,
        }
    }

    /// Validates the same acquired account before publishing only its presentation to the menu.
    pub async fn presentation(&self) -> io::Result<Presentation> {
        let profile = Arc::clone(&self.account)
            .validate_generation(&self.identity.generation)
            .await?;
        if !self.identity.current() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "friend fixture selection retired",
            ));
        }
        Ok(Presentation {
            account: Arc::clone(&self.account),
            profile,
            identity: self.identity.clone(),
            path: self.path.clone(),
        })
    }

    /// Arms the original protected service's finite interruption before the first friend RTA socket.
    pub fn hold_rta_reconnect(&mut self) -> bedrock_services::test_support::HostedRtaReconnect {
        self.peer.hold_rta_reconnect()
    }

    /// Holds this original signed member's applied rebind response until caller cancellation.
    pub fn hold_rebind_response(&mut self) -> bedrock_services::test_support::HostedRebindHold {
        self.peer.hold_friend_rebind_response()
    }

    /// Retains this original verified PUT before application while one cleanup read sees old state.
    pub fn hold_rebind_application(&mut self) -> bedrock_services::test_support::HostedRebindHold {
        self.peer.hold_friend_rebind_application()
    }

    /// Waits for the actual original handle-join response flush, without admitting its nonce.
    pub async fn joined(&mut self) -> io::Result<()> {
        tokio::time::timeout_at(
            self.deadline.into(),
            self.membership.as_mut().unwrap().joined(),
        )
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "friend fixture membership not observed",
            )
        })
    }

    /// Releases the same service barrier after its actual join response was observed.
    pub fn admit(&mut self) {
        self.membership.take().unwrap().admit();
    }

    /// Binds the original host socket; the host XUID comes from the actual decoded catalogue row.
    pub async fn listen(
        &mut self,
        host_xuid: &str,
        deadline: Instant,
    ) -> io::Result<listener::Host> {
        let identity = self
            .issuer
            .host_identity(host_xuid, self.service_time, self.observed)?;
        listener::Host::bind(
            self.host.take().unwrap(),
            identity,
            self.issuer.verifier()?,
            protocol::NativeServiceClock::new(self.service_time, self.observed),
            deadline,
        )
        .await
    }

    /// Joins the original service owners after ordinary conditional leave and transport retirement.
    pub async fn finish(self) -> Observation {
        drop(self.membership);
        if let Some(mut host) = self.host {
            host.socket.as_mut().unwrap().abort();
        }
        let observed = self.peer.finish_friend().await;
        // The client transport aborts its protected socket on retirement; the host finishes Close.
        let client_retired = matches!(
            &observed.signaling,
            Some(
                HostedSocketTerminal::Close
                    | HostedSocketTerminal::Eof
                    | HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof)
            )
        );
        let host_closed = matches!(
            &observed.host,
            Some(HostedSocketTerminal::Close | HostedSocketTerminal::Eof)
        );
        Observation {
            joins: observed.joins,
            leaves: observed.leaves,
            nonce_published: observed.nonce_published,
            minted: !observed.minted_public_key.is_empty(),
            signaling_started: observed.signaling.is_some(),
            signaling_retired: client_retired && host_closed,
            rta_retired: observed.rta == HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof),
        }
    }

    /// Aborts and joins the same finite service owners after any unsuccessful prefix.
    /// This neither performs conditional leave nor reports successful member/socket retirement.
    pub async fn abort(self) {
        drop(self.membership);
        if let Some(mut host) = self.host {
            host.socket.as_mut().unwrap().abort();
        }
        self.peer.abort_activation().await;
    }
}
