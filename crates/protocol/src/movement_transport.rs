//! Sequenced movement admission identities and last-moment interaction invalidation.

use crate::Packet;
use simulation_clock::PhysicsTick;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use tokio::sync::watch;

/// Identity preserved through movement admission, acknowledgement and cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsSendIdentity {
    pub session_generation: SessionGeneration,
    pub tick: PhysicsTick,
    pub admission_id: u64,
    pub reanchor_epoch: PositionAuthorityGeneration,
}

/// An admitted interaction and its optional preceding action, with a movement-only fallback.
#[derive(Debug)]
pub struct InteractionPacketGuard {
    authority_epoch: u64,
    authority: watch::Receiver<u64>,
    movement_packet: Packet,
    preceding_packet: Option<Packet>,
}

impl InteractionPacketGuard {
    /// Freezes the interaction epoch together with its movement-only fallback.
    pub fn new(
        authority_epoch: u64,
        authority: watch::Receiver<u64>,
        movement_packet: Packet,
    ) -> Self {
        Self {
            authority_epoch,
            authority,
            movement_packet,
            preceding_packet: None,
        }
    }

    /// Keeps a standalone action in the same admission and cancellation unit as its input.
    #[must_use]
    pub fn with_preceding_packet(mut self, packet: Packet) -> Self {
        self.preceding_packet = Some(packet);
        self
    }

    /// Removes stale interactions and their preceding actions immediately before writing.
    pub fn sanitize(self, packet: Packet) -> (Option<Packet>, Packet) {
        if *self.authority.borrow() == self.authority_epoch {
            (self.preceding_packet, packet)
        } else {
            (None, self.movement_packet)
        }
    }
}

/// Admission failure for an atomic ordered packet batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSendError {
    Full,
    Closed,
}
