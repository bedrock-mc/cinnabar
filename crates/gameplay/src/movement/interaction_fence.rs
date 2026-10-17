//! An unadmitted interaction owns its place before one immutable movement tick.
use super::*;
use std::sync::Arc;

/// Dropping the attempt releases its fence on cancellation, expiry or admission.
#[derive(Debug)]
pub struct InteractionFence(Arc<Reservation>);

#[derive(Debug)]
pub(super) struct Reservation {
    authority: (SessionGeneration, PositionAuthorityGeneration),
    before: u64,
}

impl InteractionFence {
    /// Returns the original movement tick which must remain behind this attempt.
    pub fn before_tick(&self) -> u64 {
        self.0.before
    }
}

impl MovementTicker {
    /// Reserves one head's position without retaining the attempt inside movement maintenance.
    pub fn reserve_interaction_before(&mut self, tick: u64) -> InteractionFence {
        let authority = self.interaction_authority_identity();
        if let Some(reservation) = self
            .interaction_fence
            .upgrade()
            .filter(|r| r.authority == authority)
        {
            assert_eq!(
                reservation.before, tick,
                "only one unadmitted interaction may own the cursor"
            );
            return InteractionFence(reservation);
        }
        let reservation = Arc::new(Reservation {
            authority,
            before: tick,
        });
        self.interaction_fence = Arc::downgrade(&reservation);
        InteractionFence(reservation)
    }

    /// Allows older movement to drain while preserving the attempt's original tick.
    pub(super) fn interaction_blocks_tick(&self, tick: u64) -> bool {
        self.interaction_fence.upgrade().is_some_and(|r| {
            r.authority == self.interaction_authority_identity() && tick >= r.before
        })
    }

    /// Checks the original action position without resampling a later pose or tick.
    pub fn interaction_tick_available(&self, tick: u64) -> bool {
        self.is_first_unsent_tick(tick)
            || self.between_ticks_sample().is_some_and(|s| s.tick == tick)
    }
}

#[cfg(test)]
mod tests;
