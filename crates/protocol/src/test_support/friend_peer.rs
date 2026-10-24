//! Fixed controlled world over the original accepting Login, pack and initialization owners.

use bedrock_client::server::{
    ServerApprovalEvent, ServerHandshakeOptions, ServerLoginEvent, ServerSettingsEvent,
    session::{
        ServerPackDelivery, ServerPackEvent, ServerPackSnapshot, ServerSpawnEvent, ServerWorld,
    },
};
use bedrock_client_tokio::server::AcceptingConnection;
use bedrock_net::login::provider::ProviderVerifier;
use bedrock_net::login::{LoginError, LoginVerifier, TrustedIdentity};
use bedrock_protocol_1_26_51::{packets, types::ActorRuntimeID};
use p384::ecdsa::SigningKey;
use std::{
    io,
    sync::Arc,
    time::{Instant, SystemTime},
};

/// Runtime identity of this fixed controlled world's original StartGame.
pub const RUNTIME_ID: u64 = 1;
/// Post-initialization marker value used by this same peer and its application caller.
pub const MARKER_RADIUS: u8 = 3;

/// Strict provider proof with the original acquired service clock, not generic fixture host UTC.
pub struct Policy {
    verifier: ProviderVerifier,
    clock: crate::NativeServiceClock,
}

impl Policy {
    /// Retains the original public verifier and calibrated monotonic service observation.
    pub fn new(verifier: ProviderVerifier, clock: crate::NativeServiceClock) -> Self {
        Self { verifier, clock }
    }
}

impl LoginVerifier for Policy {
    fn verify(&self, token: &str, _: SystemTime) -> Result<TrustedIdentity, LoginError> {
        self.verifier.verify(
            token,
            self.clock
                .now(Instant::now())
                .map_err(|_| LoginError::InvalidTime)?,
        )
    }
}

/// Same accepted FIFO after actual generated initialization; no public constructor can fabricate it.
pub struct Peer {
    wire: AcceptingConnection,
}

/// Verifies original SDP/Login signatures and service membership before serving one empty-pack world.
/// The predicate receives only the original cryptographically authenticated Login identity and nonce.
pub async fn initialize(
    accepted: nethernet_tokio::AcceptedSession,
    key: Arc<SigningKey>,
    policy: &Policy,
    admitted: impl Fn(&str, &str) -> bool,
    deadline: Instant,
) -> io::Result<Peer> {
    let (mut wire, mut handshake) = AcceptingConnection::from_nether(
        accepted,
        key,
        ServerHandshakeOptions::default(),
        crate::MAX_DEFERRED_PACKET_BYTES,
        deadline,
    )
    .await?;
    let result = async {
        let mut login = loop {
            let packet = wire.receive(deadline).await?;
            match handshake
                .receive(packet, policy, Instant::now(), SystemTime::now())
                .map_err(invalid)?
            {
                ServerSettingsEvent::Settings(pending) => {
                    send(&mut wire, vec![pending.packet().clone().into()], deadline).await?;
                    wire.compression_flushed(pending.compression())?;
                    break pending.flushed(Instant::now()).map_err(invalid)?;
                }
                ServerSettingsEvent::Unhandled {
                    handshake: retained,
                    packet,
                } => {
                    wire.handle_unhandled(&packet)?;
                    handshake = retained;
                }
                ServerSettingsEvent::Refused(_) => return Err(invalid(())),
            }
        };
        let verified = loop {
            let packet = wire.receive(deadline).await?;
            match login
                .receive(packet, policy, Instant::now(), SystemTime::now())
                .map_err(invalid)?
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
        let approved = verified
            .authorize(|login| {
                admitted(login.xuid(), login.nonce())
                    .then_some(())
                    .ok_or_else(|| invalid(()))
            })
            .map_err(invalid)?;
        let ServerApprovalEvent::Ready(ready) = approved
            .accept(policy, None, Instant::now(), SystemTime::now())
            .map_err(invalid)?
        else {
            return Err(invalid(()));
        };
        let (mut packs, packets) = ready
            .start_packs(
                ServerPackSnapshot {
                    offer: Default::default(),
                    stack: Default::default(),
                    contents: Vec::new(),
                },
                ServerPackDelivery::default(),
                policy,
                Instant::now(),
                SystemTime::now(),
            )
            .map_err(invalid)?;
        send(&mut wire, packets, deadline).await?;
        let ready = loop {
            match packs
                .receive(wire.receive(deadline).await?, Instant::now())
                .map_err(invalid)?
            {
                ServerPackEvent::Send { session, packets } => {
                    send(&mut wire, packets, deadline).await?;
                    packs = session;
                }
                ServerPackEvent::WorldReady(ready) => break ready,
                ServerPackEvent::Unhandled { session, packet } => {
                    wire.handle_unhandled(&packet)?;
                    packs = session;
                }
                _ => return Err(invalid(())),
            }
        };
        let (mut spawning, packets) = ready
            .start_world(
                ServerWorld {
                    start_game: Box::new(packets::StartGame {
                        runtime_id: ActorRuntimeID(RUNTIME_ID),
                        ..Default::default()
                    }),
                    items: Default::default(),
                    creative: Default::default(),
                    max_chunk_radius: 4,
                    initial_chunk_radius: None,
                },
                policy,
                Instant::now(),
                SystemTime::now(),
            )
            .map_err(invalid)?;
        send(&mut wire, packets, deadline).await?;
        let initialized = loop {
            match spawning
                .receive(wire.receive(deadline).await?, Instant::now())
                .map_err(invalid)?
            {
                ServerSpawnEvent::Send { session, packets } => {
                    send(&mut wire, packets, deadline).await?;
                    spawning = session;
                }
                ServerSpawnEvent::Initialized(initialized) => break initialized,
                ServerSpawnEvent::Unhandled { session, packet } => {
                    wire.handle_unhandled(&packet)?;
                    spawning = session;
                }
            }
        };
        let initialized = initialized
            .finish(policy, Instant::now(), SystemTime::now())
            .map_err(invalid)?;
        if !admitted(initialized.login().xuid(), initialized.login().nonce()) {
            return Err(invalid(()));
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let mut closing = wire.begin_close();
        let _ = closing.finish_close(deadline).await;
        return Err(error);
    }
    Ok(Peer { wire })
}

impl Peer {
    /// Observes one actual marker packet after initialization, then the original client close.
    pub async fn marker_and_close(
        mut self,
        marker_observed: tokio::sync::oneshot::Sender<()>,
        deadline: Instant,
    ) -> io::Result<bool> {
        let result = async {
            let mut marker = false;
            let mut marker_observed = Some(marker_observed);
            loop {
                match self.wire.receive(deadline).await {
                    Ok(packets::Packet::RequestChunkRadius(request))
                        if request.chunk_radius.0 == i32::from(MARKER_RADIUS) =>
                    {
                        marker = true;
                        if let Some(observed) = marker_observed.take() {
                            let _ = observed.send(());
                        }
                    }
                    Ok(packet) => self.wire.handle_unhandled(&packet)?,
                    // The accepted transport maps its original reliable Ok(None) to this EOF.
                    // Malformed batches/packets and transport failures retain their actual errors.
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        return Ok(marker);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        .await;
        let mut closing = self.wire.begin_close();
        let closed = closing.finish_close(deadline).await;
        result.and_then(|marker| closed.map(|()| marker))
    }
}

/// Commits the exact original generated effect through the retained accepting send receipt.
async fn send(
    wire: &mut AcceptingConnection,
    packets: Vec<packets::Packet>,
    deadline: Instant,
) -> io::Result<()> {
    let mut send = wire.prepare_packets(packets, deadline)?;
    while let Some(capacity) = send.next().await? {
        capacity.commit()?;
    }
    send.finish()
}

/// Avoids exposing synthetic JWTs, nonces or generated packet bodies in failure diagnostics.
fn invalid(_: impl Sized) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "controlled friend peer rejected",
    )
}
