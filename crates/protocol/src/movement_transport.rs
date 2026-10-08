//! Sequenced movement admission identities and last-moment interaction invalidation.

use crate::Packet;
use tokio::sync::watch;

/// Identity preserved through movement admission, acknowledgement and cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsSendIdentity {
    pub session_generation: u64,
    pub tick: u64,
    pub admission_id: u64,
    pub reanchor_epoch: u64,
}

/// A movement-only replacement when an admitted interaction loses its authority.
#[derive(Debug)]
pub struct InteractionPacketGuard {
    authority_epoch: u64,
    authority: watch::Receiver<u64>,
    movement_packet: Packet,
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
        }
    }

    /// Removes a stale interaction immediately before transport begins its write.
    pub fn sanitize(self, packet: Packet) -> Packet {
        if *self.authority.borrow() == self.authority_epoch {
            packet
        } else {
            self.movement_packet
        }
    }
}

/// Admission failure for an atomic ordered packet batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSendError {
    Full,
    Closed,
}
