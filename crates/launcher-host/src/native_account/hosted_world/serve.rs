//! Actual accepting Login/pack/world continuations paired with the running Go world.

mod backend;
mod bds;
pub(super) mod play;
mod send;
pub(super) use backend::HostedBackend;
mod packs;
pub use play::{ClosingHostedPeer, NativeHostedPeer};
use send::SendAuthority;
mod process;
pub use process::NativeHostedProcess;

use super::{HostedApproval, HostedLoginApproval, NativeHostedLoginPolicy, NativeHostedWorld};
use bedrock_client::server::{
    ServerAcknowledgmentEvent, ServerApprovalEvent, ServerHandshake, ServerHandshakeOptions,
    ServerLoginEvent, ServerPacksReady, ServerSettingsEvent,
    session::{AcceptedSession, ServerSpawnEvent},
};
use bridge::host_world::{
    ActiveBackendPeer, BackendWorldReady, HostProcess, PendingBackendPeer, contract::Identity,
};
use p384::ecdsa::SigningKey;
use protocol::{NativeAcceptingIo, NativeHostWorld, native_host_packs};
use std::{io, sync::Arc, time::Instant};

impl NativeHostedWorld {
    /// Applies current service state and original membership before synchronous Attach admission.
    /// Cryptographic Login and typed ServerWorldReady remain with the calling pack owner.
    pub(super) async fn attach_backend(
        &mut self,
        approval: &HostedApproval,
        backend: PendingBackendPeer,
        deadline: Instant,
    ) -> io::Result<BackendWorldReady> {
        self.directory_mut()?
            .service_barrier(Some(deadline))
            .await?;
        self.wait_approval(approval, Some(deadline)).await?;
        let backend = backend.begin_attach(deadline)?;
        let backend = self.peer_io(Some(deadline), backend.finish()).await?;
        self.wait_approval(approval, Some(deadline)).await?;
        Ok(backend)
    }

    /// Applies current service state and original membership before synchronous Initialized admission.
    /// Cryptographic Login and typed ServerInitialized::finish remain with the caller.
    pub(super) async fn initialize_backend(
        &mut self,
        approval: &HostedApproval,
        backend: BackendWorldReady,
        cache_enabled: bool,
        radius: u32,
        deadline: Instant,
    ) -> io::Result<ActiveBackendPeer> {
        self.directory_mut()?
            .service_barrier(Some(deadline))
            .await?;
        self.wait_approval(approval, Some(deadline)).await?;
        let backend = backend.begin_initialized(cache_enabled, radius, deadline)?;
        let backend = self.peer_io(Some(deadline), backend.finish()).await?;
        self.wait_approval(approval, Some(deadline)).await?;
        Ok(backend)
    }

    /// Drains prior control, then applies current service authority before synchronous Open admission.
    /// Verified Login metadata and the selected process owner remain with the caller.
    pub(super) async fn open_backend(
        &mut self,
        approval: &HostedApproval,
        process: &mut HostProcess,
        identity: Identity,
        client_data: Box<serde_json::value::RawValue>,
        deadline: Instant,
    ) -> io::Result<PendingBackendPeer> {
        self.wait_approval(approval, Some(deadline)).await?;
        let prepared = self
            .peer_io(
                Some(deadline),
                process.prepare_open(identity, client_data, deadline),
            )
            .await?;
        self.directory_mut()?
            .service_barrier(Some(deadline))
            .await?;
        self.wait_approval(approval, Some(deadline)).await?;
        let opening = prepared.begin_open()?;
        let backend = self.peer_io(Some(deadline), opening.finish()).await?;
        self.wait_approval(approval, Some(deadline)).await?;
        Ok(backend)
    }

    /// Pairs one actual established transport with this original acknowledged hosted opening.
    ///
    /// The caller owns the server's authenticated signaling/SDP policy and passes that exact
    /// server key allocation. This does not start a listener, runtime, account worker or advertise
    /// a route. Keep this whole future pinned; canceling it closes its actual transport/backend.
    /// The existing host owner drives RTA and retains remote membership independently of the peer.
    pub async fn accept_dragonfly_peer(
        &mut self,
        process: &mut NativeHostedProcess,
        accepted: nethernet_tokio::AcceptedSession,
        key: Arc<SigningKey>,
        options: ServerHandshakeOptions,
        deadline: Instant,
    ) -> io::Result<NativeHostedPeer> {
        self.check_bridge(process, deadline).await?;
        let policy = self.login_policy(deadline).await?;
        let turn_control = Arc::new(accepted.session.sender());
        let (mut wire, handshake) = self
            .peer_io(
                Some(deadline),
                NativeAcceptingIo::from_nether(accepted, key, options, deadline),
            )
            .await?;
        let result = async {
            let ready = self.login(&mut wire, handshake, &policy, deadline).await?;
            self.wait_login(ready.login(), &policy, ready.grant())
                .await?;
            let player = ready.login().player_identity().ok_or_else(rejected)?;
            let identity = Identity {
                xuid: ready.login().xuid().into(),
                uuid: player.uuid().to_string(),
                display_name: player.display_name().into(),
                playfab_id: player.playfab_id().into(),
                playfab_title_id: player.playfab_title_id().into(),
            };
            // RawValue owns the exact provider-verified object, including unused lexemes and fields.
            let data = ready.login().client_data().to_owned();
            let backend = self
                .open_backend(
                    &ready.grant().approval,
                    &mut process.process,
                    identity,
                    data,
                    deadline,
                )
                .await?;
            self.wait_login(ready.login(), &policy, ready.grant())
                .await?;
            self.check_bridge(process, deadline).await?;
            self.packs_and_world(
                &mut wire,
                ready,
                backend,
                &process.process,
                policy,
                deadline,
            )
            .await
        }
        .await;
        match result {
            Ok((backend, accepted)) => {
                Ok(NativeHostedPeer::new(wire, backend, accepted, turn_control))
            }
            Err(error) => {
                Self::retire_wire(&mut wire, deadline).await;
                let mut retired = wire.begin_close();
                let _ = retired.finish_close(deadline).await;
                Err(error)
            }
        }
    }

    /// Polls the one existing directory owner beside this exact retained IO operation.
    pub(super) async fn peer_io<T>(
        &mut self,
        deadline: Option<Instant>,
        future: impl std::future::Future<Output = io::Result<T>>,
    ) -> io::Result<T> {
        let directory = self.directory.as_mut().ok_or_else(super::closed)?;
        directory
            .peer_io(deadline, future, Some(&mut self.peers))
            .await
    }

    /// Executes only the reusable typed accepting handshake, with exact flush continuations.
    async fn login(
        &mut self,
        wire: &mut NativeAcceptingIo,
        mut handshake: ServerHandshake,
        policy: &NativeHostedLoginPolicy,
        deadline: Instant,
    ) -> io::Result<ServerPacksReady<HostedLoginApproval>> {
        let mut login = loop {
            let packet = self.peer_io(Some(deadline), wire.receive(deadline)).await?;
            self.wait_policy(policy).await?;
            match handshake
                .receive(
                    packet,
                    policy,
                    Instant::now(),
                    policy.utc().map_err(|_| rejected())?,
                )
                .map_err(|_| rejected())?
            {
                ServerSettingsEvent::Settings(pending) => {
                    self.send_setup(
                        wire.prepare_packets(vec![pending.packet().clone().into()], deadline)?,
                        SendAuthority::Policy(policy),
                    )
                    .await?;
                    self.wait_policy(policy).await?;
                    wire.compression_flushed(pending.compression())?;
                    break pending.flushed(Instant::now()).map_err(|_| rejected())?;
                }
                ServerSettingsEvent::Refused(status) => {
                    self.send_setup(
                        wire.prepare_packets(vec![status.into()], deadline)?,
                        SendAuthority::Policy(policy),
                    )
                    .await?;
                    return Err(rejected());
                }
                ServerSettingsEvent::Unhandled {
                    handshake: retained,
                    packet,
                } => {
                    wire.handle_unhandled(&packet)?;
                    handshake = retained;
                }
            }
        };
        let verified = loop {
            let packet = self.peer_io(Some(deadline), wire.receive(deadline)).await?;
            self.wait_policy(policy).await?;
            match login
                .receive(
                    packet,
                    policy,
                    Instant::now(),
                    policy.utc().map_err(|_| rejected())?,
                )
                .map_err(|_| rejected())?
            {
                ServerLoginEvent::Verified(verified) => break verified,
                ServerLoginEvent::Unhandled {
                    login: retained,
                    packet,
                } => {
                    wire.handle_unhandled(&packet)?;
                    login = retained;
                }
            }
        };
        let grant = self.wait_approve_login(verified.login(), policy).await?;
        let approved = verified
            .authorize(|_| Ok::<_, io::Error>(grant))
            .map_err(|_| rejected())?;
        self.wait_login(approved.login(), policy, approved.grant())
            .await?;
        let approval = approved
            .accept(
                policy,
                None,
                Instant::now(),
                policy.utc().map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
        let mut acknowledgment = match approval {
            ServerApprovalEvent::Ready(ready) => return Ok(ready),
            ServerApprovalEvent::KeyExchange(pending) => {
                self.wait_login_grant(policy, pending.grant()).await?;
                self.send_setup(
                    wire.prepare_packets(vec![pending.packet().clone().into()], deadline)?,
                    SendAuthority::KeyExchange(policy, pending.grant()),
                )
                .await?;
                self.wait_login_grant(policy, pending.grant()).await?;
                let (acknowledgment, ciphers) =
                    pending.flushed(Instant::now()).map_err(|_| rejected())?;
                wire.encryption_flushed(*ciphers)?;
                acknowledgment
            }
        };
        loop {
            let packet = self.peer_io(Some(deadline), wire.receive(deadline)).await?;
            self.wait_login_grant(policy, acknowledgment.grant())
                .await?;
            match acknowledgment
                .receive(
                    packet,
                    policy,
                    Instant::now(),
                    policy.utc().map_err(|_| rejected())?,
                )
                .map_err(|_| rejected())?
            {
                ServerAcknowledgmentEvent::Ready(ready) => return Ok(ready),
                ServerAcknowledgmentEvent::Unhandled {
                    acknowledgment: retained,
                    packet,
                } => {
                    wire.handle_unhandled(&packet)?;
                    acknowledgment = retained;
                }
            }
        }
    }

    /// Keeps the actual pack continuation/source lease until the real client runtime-ID handoff.
    async fn packs_and_world(
        &mut self,
        wire: &mut NativeAcceptingIo,
        ready: ServerPacksReady<HostedLoginApproval>,
        mut backend: PendingBackendPeer,
        process: &HostProcess,
        policy: NativeHostedLoginPolicy,
        deadline: Instant,
    ) -> io::Result<(ActiveBackendPeer, AcceptedSession<HostedLoginApproval>)> {
        let snapshot = native_host_packs(process.snapshot())?;
        let ready = self
            .deliver_packs(
                wire,
                ready,
                snapshot,
                packs::PackSource::Dragonfly(&mut backend),
                &policy,
                deadline,
            )
            .await?;
        let backend = self
            .attach_backend(&ready.grant().approval, backend, deadline)
            .await?;
        self.wait_login(ready.login(), &policy, ready.grant())
            .await?;
        let (mut packets, world) = NativeHostWorld::parse(backend.world())?.into_parts();
        let (mut spawning, startup) = ready
            .start_world(
                world,
                &policy,
                Instant::now(),
                policy.utc().map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
        packets.extend(startup);
        self.send_setup(
            wire.prepare_packets(packets, deadline)?,
            SendAuthority::Login(spawning.login(), &policy, spawning.grant()),
        )
        .await?;
        let initialized = loop {
            self.wait_login(spawning.login(), &policy, spawning.grant())
                .await?;
            let packet = self.peer_io(Some(deadline), wire.receive(deadline)).await?;
            self.wait_login(spawning.login(), &policy, spawning.grant())
                .await?;
            match spawning
                .receive(packet, Instant::now())
                .map_err(|_| rejected())?
            {
                ServerSpawnEvent::Send { session, packets } => {
                    self.wait_login(session.login(), &policy, session.grant())
                        .await?;
                    self.send_setup(
                        wire.prepare_packets(packets, deadline)?,
                        SendAuthority::Login(session.login(), &policy, session.grant()),
                    )
                    .await?;
                    spawning = session;
                }
                ServerSpawnEvent::Initialized(initialized) => break initialized,
                ServerSpawnEvent::Unhandled { session, packet } => {
                    wire.handle_unhandled(&packet)?;
                    spawning = session;
                }
            }
        };
        self.wait_login(initialized.login(), &policy, initialized.grant())
            .await?;
        let accepted = initialized
            .finish(
                &policy,
                Instant::now(),
                policy.utc().map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
        let radius = u32::try_from(accepted.chunk_radius()).map_err(|_| rejected())?;
        let backend = self
            .initialize_backend(
                &accepted.grant().approval,
                backend,
                accepted.client_cache_enabled(),
                radius,
                deadline,
            )
            .await?;
        self.wait_login(accepted.login(), &policy, accepted.grant())
            .await?;
        Ok((backend, accepted))
    }
}

/// Does not format provider tokens, signed client data or hosted-world identifiers.
fn rejected() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native hosted peer rejected",
    )
}
