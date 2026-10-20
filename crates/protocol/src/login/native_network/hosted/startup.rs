//! Both original IO legs bind each typed relay transition to its actual prepared-send completion.

use super::*;
use crate::session_wire::CoreMessage;
use bedrock_client::server::session::{
    PendingServerRelay, ServerRelayInitialized, ServerRelayProgress, ServerRelaying,
};

/// Original accepting pack owner paired with its same guest/local upstream through initialization.
/// The caller still owns current account/opening/member approval at every actual send commit.
#[must_use]
pub struct NativeHostedRelay<A> {
    wire: NativeAcceptingIo,
    upstream: NativeHostedUpstream,
    relay: Option<ServerRelaying<A>>,
    ready: Option<ServerRelayInitialized<A>>,
    effect: Option<Effect<A>>,
    terminal: Option<CoreMessage>,
    deadline: Instant,
    failed: bool,
}

struct Effect<A> {
    raw: Bytes,
    source: Source,
    control: Option<PendingServerRelay<A>>,
    prepared: bool,
}

#[derive(Clone, Copy)]
enum Source {
    Client,
    Server,
}

/// Actual forwarded progress; initialization still requires final provider/application validation.
pub enum NativeHostedRelayProgress {
    /// The original effect completed; more actual startup input is required.
    Forwarded,
    /// Actual matching Initialized completed its original upstream send.
    Initialized,
    /// The first original terminal completed its forward; no new startup admission remains.
    Terminal,
}

impl<A> NativeHostedRelay<A> {
    /// Takes both original IO owners and the actual consumed pack-stack acknowledgment.
    pub(super) fn new(
        upstream: NativeHostedUpstream,
        wire: NativeAcceptingIo,
        ready: ServerWorldReady<A>,
        verifier: &impl LoginVerifier,
        now: Instant,
        utc: SystemTime,
    ) -> io::Result<Self> {
        let deadline = upstream.deadline.min(ready.deadline());
        if now >= deadline {
            return Err(expired());
        }
        let relay = ready.relay_world(verifier, now, utc).map_err(|error| {
            let (_owner, cause) = error.into_parts();
            io::Error::other(cause)
        })?;
        Ok(Self {
            wire,
            upstream,
            relay: Some(relay),
            ready: None,
            effect: None,
            terminal: None,
            deadline,
            failed: false,
        })
    }

    /// Borrows only the same original authenticated Login, including while a forward is pending.
    pub fn login(&self) -> &AuthenticatedLogin {
        if let Some(control) = self
            .effect
            .as_ref()
            .and_then(|effect| effect.control.as_ref())
        {
            control.login()
        } else if let Some(ready) = &self.ready {
            ready.login()
        } else {
            self.relay
                .as_ref()
                .expect("original relay retained")
                .login()
        }
    }

    /// Borrows original application authority for an adjacent reserve/commit or attachment check.
    pub fn grant(&self) -> &A {
        if let Some(control) = self
            .effect
            .as_ref()
            .and_then(|effect| effect.control.as_ref())
        {
            control.grant()
        } else if let Some(ready) = &self.ready {
            ready.grant()
        } else {
            self.relay
                .as_ref()
                .expect("original relay retained")
                .grant()
        }
    }

    /// Returns the minimum original upstream/accepting deadline; no continuation resets it.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Retains one actual original input before returning to the caller's authority postcheck.
    /// Canceling a pending read does not consume or replace decoded FIFO siblings.
    pub async fn receive(&mut self) -> io::Result<()> {
        self.live()?;
        if self.effect.is_some() || self.ready.is_some() {
            return Err(invalid());
        }
        let received = tokio::time::timeout_at(self.deadline.into(), async {
            tokio::select! {
                packet = self.wire.receive_raw(self.deadline) => packet.map(|raw| (Source::Client, raw)),
                packet = self.upstream.receive() => packet.map(|raw| (Source::Server, raw)),
            }
        }).await.unwrap_or_else(|_| Err(expired()));
        let (source, raw) = match received {
            Ok(value) => value,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        self.effect = Some(Effect {
            raw,
            source,
            control: None,
            prepared: false,
        });
        Ok(())
    }

    /// Prepares the retained original effect once, without committing the actual permit/cipher.
    /// Returned disjoint Login/grant borrows permit fresh checks after actual capacity waits.
    /// Drive its original capacity/receipt, then call `complete_forward` instead of send `finish`.
    pub fn prepare_forward(
        &mut self,
    ) -> io::Result<(&AuthenticatedLogin, &A, NativeAcceptingSend<'_>)> {
        self.live()?;
        if let Err(error) = self.prepare_effect() {
            self.failed = true;
            return Err(error);
        }
        self.borrow_prepared()
    }

    /// Retains the original typed/raw effect before exposing its destination's reservation.
    fn prepare_effect(&mut self) -> io::Result<()> {
        let effect = self.effect.as_mut().ok_or_else(invalid)?;
        if effect.prepared {
            return Err(invalid());
        }
        let header = header(&effect.raw)?;
        let primary = header.sender_subclient == 0 && header.target_subclient == 0;
        let control = primary
            && matches!(
                (effect.source, packets::PacketId::from_raw(header.id)),
                (
                    Source::Server,
                    Some(
                        packets::PacketId::StartGame
                            | packets::PacketId::ItemRegistry
                            | packets::PacketId::ChunkRadiusUpdated
                            | packets::PacketId::PlayStatus
                    )
                ) | (
                    Source::Client,
                    Some(
                        packets::PacketId::RequestChunkRadius
                            | packets::PacketId::SetLocalPlayerAsInitialized
                            | packets::PacketId::ClientCacheStatus
                    )
                )
            );
        if primary
            && matches!(
                packets::PacketId::from_raw(header.id),
                Some(packets::PacketId::Disconnect | packets::PacketId::Transfer)
            )
        {
            self.terminal = super::super::relay::terminal(&effect.raw)?;
        }
        if control {
            let packet = decode(&effect.raw, matches!(effect.source, Source::Client))?;
            let relay = self.relay.take().ok_or_else(invalid)?;
            let pending = match effect.source {
                Source::Client => relay.forward_client(packet, Instant::now()),
                Source::Server => relay.forward_server(packet, Instant::now()),
            };
            effect.control = match pending {
                Ok(pending) => Some(pending),
                Err(error) => {
                    let (relay, cause) = error.into_parts();
                    self.relay = Some(relay);
                    self.failed = true;
                    return Err(io::Error::other(cause));
                }
            };
        }
        let connection = match effect.source {
            Source::Client => &mut self.upstream.connection,
            Source::Server => &mut self.wire.connection,
        };
        let body = connection.raw_body(effect.raw.clone())?;
        connection.prepare_send(body, self.deadline)?;
        effect.prepared = true;
        Ok(())
    }

    /// Reborrows the same destination's original prefix/permit/receipt after cancellation.
    pub fn resume_forward(
        &mut self,
    ) -> io::Result<(&AuthenticatedLogin, &A, NativeAcceptingSend<'_>)> {
        self.live()?;
        self.borrow_prepared()
    }

    /// Splits original authority and destination fields so checks remain possible after capacity waits.
    fn borrow_prepared(
        &mut self,
    ) -> io::Result<(&AuthenticatedLogin, &A, NativeAcceptingSend<'_>)> {
        let effect = self
            .effect
            .as_ref()
            .filter(|effect| effect.prepared)
            .ok_or_else(invalid)?;
        let (login, grant) = if let Some(control) = &effect.control {
            (control.login(), control.grant())
        } else {
            let relay = self.relay.as_ref().ok_or_else(invalid)?;
            (relay.login(), relay.grant())
        };
        let connection = match effect.source {
            Source::Client => &mut self.upstream.connection,
            Source::Server => &mut self.wire.connection,
        };
        connection.prepared_deadline()?;
        Ok((login, grant, NativeAcceptingSend { connection }))
    }

    /// Requires the SAME destination's complete retained send before advancing typed startup.
    /// Local core admission is not a claim that the remote peer processed the effect.
    pub fn complete_forward(&mut self) -> io::Result<NativeHostedRelayProgress> {
        self.live()?;
        let effect = self
            .effect
            .as_ref()
            .filter(|effect| effect.prepared)
            .ok_or_else(invalid)?;
        match effect.source {
            Source::Client => self.upstream.connection.finish_send()?,
            Source::Server => self.wire.connection.finish_send()?,
        }
        let effect = self.effect.take().expect("checked original effect");
        match effect.source {
            Source::Client => self.wire.connection.finish_current(),
            Source::Server => self.upstream.connection.finish_current(),
        }
        if let Some(control) = effect.control {
            match control.flushed(Instant::now()) {
                Ok(ServerRelayProgress::Continue(relay)) => self.relay = Some(relay),
                Ok(ServerRelayProgress::Initialized(ready)) => self.ready = Some(ready),
                Err(error) => {
                    let (control, cause) = error.into_parts();
                    self.effect = Some(Effect {
                        control: Some(control),
                        ..effect
                    });
                    self.failed = true;
                    return Err(io::Error::other(cause));
                }
            }
        }
        if self.terminal.is_some() {
            self.failed = true;
            Ok(NativeHostedRelayProgress::Terminal)
        } else if self.ready.is_some() {
            Ok(NativeHostedRelayProgress::Initialized)
        } else {
            Ok(NativeHostedRelayProgress::Forwarded)
        }
    }

    /// Borrows the first actual decoded terminal, never a synthesized receipt or disconnect.
    pub fn terminal(&self) -> Option<&CoreMessage> {
        self.terminal.as_ref()
    }

    /// Transfers only actual initialized original IO and provider revalidated accepting authority.
    pub fn finish(
        mut self,
        verifier: &impl LoginVerifier,
        now: Instant,
        utc: SystemTime,
    ) -> io::Result<(NativeAcceptingIo, NativeHostedUpstream, AcceptedSession<A>)> {
        self.live()?;
        if self.effect.is_some() || now >= self.deadline {
            return Err(invalid());
        }
        let accepted = self
            .ready
            .take()
            .ok_or_else(invalid)?
            .finish(verifier, now, utc)
            .map_err(|error| {
                let (_owner, cause) = error.into_parts();
                io::Error::other(cause)
            })?;
        self.upstream.initialized = true;
        Ok((self.wire, self.upstream, accepted))
    }

    /// Seals both send directions and retains their actual original receipt/transport cleanup.
    pub fn begin_close(mut self, deadline: Instant) -> NativeHostedRelayClosing {
        self.wire.connection.retire_send(deadline);
        NativeHostedRelayClosing {
            wire: self.wire,
            upstream: self.upstream.begin_close(deadline),
            deadline,
            failure: None,
            closed: false,
        }
    }

    /// Keeps all startup reads/new forwards under the original cutoff and terminal state.
    fn live(&self) -> io::Result<()> {
        if self.failed {
            return Err(invalid());
        }
        check(self.deadline)
    }
}

/// Both original startup legs remain owned through receipt settlement and finite retirement.
#[must_use]
pub struct NativeHostedRelayClosing {
    wire: NativeAcceptingIo,
    upstream: NativeHostedUpstreamClosing,
    deadline: Instant,
    failure: Option<std::sync::Arc<io::Error>>,
    closed: bool,
}

impl NativeHostedRelayClosing {
    /// Attempts both original legs even after the first failure; borrowed cancellation retains them.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        if !self.closed {
            let (wire, upstream) = tokio::join!(
                self.wire.connection.settle_send(),
                self.upstream.finish_close()
            );
            for result in [wire.map(|_| ()), upstream] {
                if let Err(error) = result {
                    self.failure
                        .get_or_insert_with(|| std::sync::Arc::new(error));
                }
            }
            let result =
                tokio::time::timeout_at(self.deadline.into(), self.wire.connection.close())
                    .await
                    .unwrap_or_else(|_| Err(expired()));
            if let Err(error) = result {
                self.failure
                    .get_or_insert_with(|| std::sync::Arc::new(error));
            }
            self.closed = true;
        }
        match &self.failure {
            Some(error) => Err(io::Error::new(error.kind(), std::sync::Arc::clone(error))),
            None => Ok(()),
        }
    }
}
