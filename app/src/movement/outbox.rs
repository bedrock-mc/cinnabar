//! Outbound transport hand-off for completed physics samples.
//!
//! The bounded retry FIFO lives on [`MovementTicker`]; this module owns the
//! single point where a queued completed sample is encoded, staged as an
//! immutable admission record, and handed to the network transport — or,
//! while the provisional spawn-settle gate suppresses transmission, drained
//! and withheld instead.

use protocol::{Packet, player_auth_input};

use super::{
    MovementOutboxReconciliation, MovementSendError, MovementTicker, PhysicsAuthorityFault,
    PhysicsSendIdentity, PhysicsTickEvidenceContext,
};

/// Capacity of every movement retry queue: queued samples, staged sends,
/// sent-history confirmations, and retained tick evidence.
pub const OUTBOX_CAPACITY: usize = 32;

pub(crate) fn flush_player_auth_inputs<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet) -> Result<(), E>,
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
        let Some(sample) = ticker.pop_pending() else {
            break;
        };
        let packet = player_auth_input(sample.snapshot).map_err(MovementSendError::Encode)?;
        let identity = ticker.next_send_identity(&sample);
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        if let Err(error) = send(identity, packet) {
            let sample = ticker
                .restore_admitted(identity)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker
                .retry_front(sample)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker.outbox_reconciliation = MovementOutboxReconciliation::TransportRestored;
            return Err(MovementSendError::Transport(error));
        }
        sent += 1;
    }
    ticker.refresh_outbox_reconciliation();
    Ok(sent)
}
