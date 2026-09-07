//! Outbound transport hand-off for completed physics samples.
//!
//! The bounded retry FIFO lives on [`MovementTicker`]; this module owns the
//! single point where a queued completed sample is encoded, staged as an
//! immutable admission record, and handed to the network transport — or,
//! while the provisional spawn-settle gate suppresses transmission, drained
//! and withheld instead.

use protocol::{Packet, PlayerAuthInputError, player_auth_input_with_interactions};
use tokio::sync::watch;

use crate::mining::FrozenCreativeMining;

/// Failure taxonomy of one bounded outbound movement flush.
///
/// Extracted verbatim from the movement root module to respect the per-file
/// architecture line policy; the public path
/// (`crate::movement::MovementSendError`) is unchanged.
#[derive(Debug, PartialEq, Eq)]
pub enum MovementSendError<E> {
    Encode(PlayerAuthInputError),
    Transport(E),
    RestoreOverflow,
    MissingEvidenceContext,
}

/// Terminal reconciliation classification of the outbound physics stream.
///
/// Extracted verbatim from the movement root module to respect the per-file
/// architecture line policy; the public path
/// (`crate::movement::MovementOutboxReconciliation`) is unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MovementOutboxReconciliation {
    #[default]
    NotAuthoritative,
    Drained,
    SocketPending,
    BudgetDeferred,
    TransportRestored,
    FullRestored,
    /// The outbound stream was healthy when the REMOTE side terminated the
    /// transport mid-session. This is a terminal classification only: it is
    /// latched from any receive-side session failure, including server-initiated
    /// kicks, so it means "not an outbox-drain fault", never client exoneration;
    /// the normalized disconnect reason remains the authority for why the
    /// server hung up.
    RemoteClosed,
}

impl MovementOutboxReconciliation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotAuthoritative => "NotAuthoritative",
            Self::Drained => "Drained",
            Self::SocketPending => "SocketPending",
            Self::BudgetDeferred => "BudgetDeferred",
            Self::TransportRestored => "TransportRestored",
            Self::FullRestored => "FullRestored",
            Self::RemoteClosed => "RemoteClosed",
        }
    }
}

use super::{
    MovementTicker, PhysicsAuthorityFault, PhysicsSendIdentity, PhysicsTickEvidenceContext,
};

/// Capacity of every movement retry queue: queued samples, staged sends,
/// sent-history confirmations, and retained tick evidence.
pub const OUTBOX_CAPACITY: usize = 32;

/// A movement-only replacement for an admitted mining packet whose authority
/// is revoked before its socket write begins.
#[derive(Debug)]
pub(crate) struct MiningPacketGuard {
    authority_epoch: u64,
    authority: watch::Receiver<u64>,
    movement_packet: Packet,
}

impl MiningPacketGuard {
    pub(crate) fn sanitize(self, packet: Packet) -> Packet {
        if *self.authority.borrow() == self.authority_epoch {
            packet
        } else {
            self.movement_packet
        }
    }

    #[cfg(test)]
    pub(crate) fn is_current(&self) -> bool {
        *self.authority.borrow() == self.authority_epoch
    }

    #[cfg(test)]
    pub(crate) fn testing(
        authority_epoch: u64,
        authority: watch::Receiver<u64>,
        movement_packet: Packet,
    ) -> Self {
        Self {
            authority_epoch,
            authority,
            movement_packet,
        }
    }
}

#[cfg(test)]
pub(crate) fn flush_player_auth_inputs<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    flush_player_auth_inputs_guarded(
        ticker,
        budget,
        evidence_context,
        |identity, packet, _guard| send(identity, packet),
    )
}

pub(crate) fn flush_player_auth_inputs_guarded<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet, Option<MiningPacketGuard>) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    if !ticker.physics_is_authorized() {
        ticker.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        return Ok(0);
    }
    if ticker.terminal_drain || ticker.has_unresolved_position_authority_change() {
        ticker.refresh_outbox_reconciliation();
        return Ok(0);
    }
    if ticker.tx_gate.suppressing() {
        // Provisional spawn-settle window (see the `settle` module): the
        // simulation, admission, and tick scheduling continue unchanged, but
        // queued completed samples are withheld from the transport instead of
        // being encoded or staged. Suppressed ticks never reach the wire and
        // are never replayed; no evidence context is required because nothing
        // is encoded on this path.
        ticker.withhold_settled_outbox(budget);
        ticker.refresh_outbox_reconciliation();
        return Ok(0);
    }
    if !ticker.outbox.is_empty() && evidence_context.is_none() {
        return Err(MovementSendError::MissingEvidenceContext);
    }

    let mut sent = 0;
    for _ in 0..budget {
        if ticker.tick_evidence.len() == OUTBOX_CAPACITY {
            ticker.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            break;
        }
        let Some(mut sample) = ticker.pop_pending() else {
            break;
        };
        // Opt-in HandledTeleport acknowledgement: the flag is applied at
        // flush time on the popped record, so retries restore it with the bit
        // intact, and pending state is consumed only after the transport
        // accepts the packet. See the `teleport_ack` module.
        let carried_teleport_ack = ticker.project_pending_teleport_ack(&mut sample);
        let mining_guard = sample
            .mining
            .as_ref()
            .map(|_| {
                let movement_packet = player_auth_input_with_interactions(
                    sample.snapshot,
                    &protocol::PlayerAuthInputInteractions::default(),
                )?;
                Ok::<_, PlayerAuthInputError>(MiningPacketGuard {
                    authority_epoch: *ticker.mining_epoch_publisher.borrow(),
                    authority: ticker.mining_epoch_publisher.subscribe(),
                    movement_packet,
                })
            })
            .transpose()
            .map_err(MovementSendError::Encode)?;
        let interactions = sample
            .mining
            .as_ref()
            .map_or_else(protocol::PlayerAuthInputInteractions::default, |mining| {
                mining.interactions.clone()
            });
        let packet = player_auth_input_with_interactions(sample.snapshot, &interactions)
            .map_err(MovementSendError::Encode)?;
        let identity = ticker.next_send_identity(&sample);
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        if let Err(error) = send(identity, packet, mining_guard) {
            let sample = ticker
                .restore_admitted(identity)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker
                .retry_front(sample)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker.outbox_reconciliation = MovementOutboxReconciliation::TransportRestored;
            return Err(MovementSendError::Transport(error));
        }
        if carried_teleport_ack {
            ticker.consume_pending_teleport_ack();
        }
        sent += 1;
    }
    ticker.refresh_outbox_reconciliation();
    Ok(sent)
}

impl MovementTicker {
    pub(crate) fn accepts_creative_mining(&self) -> bool {
        self.physics_is_authorized()
            && !self.terminal_drain
            && !self.has_unresolved_position_authority_change()
            && !self.tx_gate.suppressing()
    }

    /// Drops interaction payloads whose frozen target, selection, ability, or
    /// session is no longer authorized. Movement samples remain queued.
    pub(crate) fn retain_creative_mining(&mut self, current: Option<&FrozenCreativeMining>) {
        let stale = self
            .outbox
            .iter()
            .filter_map(|sample| sample.mining.as_ref())
            .chain(
                self.pending_sends
                    .iter()
                    .filter_map(|pending| pending.sample.mining.as_ref()),
            )
            .any(|mining| current.is_none_or(|current| !mining.still_authorized_by(current)));
        if stale {
            self.invalidate_creative_mining();
        }
    }

    /// Attaches one complete creative break only to its exact unsent physics
    /// tick. `None` leaves the caller's input edge pending for a later tick.
    pub(crate) fn attach_creative_mining(&mut self, frozen: FrozenCreativeMining) -> Option<u64> {
        if !self.accepts_creative_mining() {
            return None;
        }
        let tick = frozen.frame.physics_tick;
        let sample = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick)?;
        if frozen.frame.position_authority_generation != self.reanchor_epoch
            || sample.session_generation != frozen.frame.session_generation
            || sample.snapshot.input_mode != frozen.input_mode
            || sample.world_identity != frozen.ray.movement_world_identity
            || sample.mining.is_some()
        {
            return None;
        }
        sample.mining = Some(frozen.into_tick_payload(sample.snapshot.position));
        Some(tick)
    }

    pub(crate) fn has_queued_creative_mining(&self) -> bool {
        self.outbox.iter().any(|sample| sample.mining.is_some())
            || self
                .pending_sends
                .iter()
                .any(|pending| pending.sample.mining.is_some())
    }

    pub(crate) const fn mining_authority_identity(&self) -> (u64, u64) {
        (self.session_generation, self.reanchor_epoch)
    }

    fn invalidate_creative_mining(&mut self) {
        let next = self.mining_epoch_publisher.borrow().wrapping_add(1);
        self.mining_epoch_publisher.send_replace(next);
        for queued in &mut self.outbox {
            queued.mining = None;
        }
        for pending in &mut self.pending_sends {
            pending.sample.mining = None;
        }
    }

    /// Invalidates every transport-owned sample after a position-authority
    /// change and publishes the new epoch atomically with that invalidation.
    pub(super) fn position_authority_changed(&mut self) {
        self.reanchor_epoch = self.reanchor_epoch.wrapping_add(1);
        self.epoch_publisher.send_if_modified(|published| {
            if *published == self.reanchor_epoch {
                false
            } else {
                *published = self.reanchor_epoch;
                true
            }
        });
        self.invalidate_creative_mining();
        for pending in &mut self.pending_sends {
            pending.retry_after_cancellation = false;
        }
        self.sent_history.clear();
        self.refresh_outbox_reconciliation();
    }
}
