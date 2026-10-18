//! Equipment eligibility retained until admission within one movement authority.

use client_interaction::movement::MovementTicker;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// Retains unsent per-hand work without retaining stale inventory packet payloads.
#[derive(Default)]
pub(crate) struct HeldEquipmentSchedule {
    scope: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    observed_completed_tick: Option<u64>,
    mainhand: Option<u64>,
    offhand: Option<u64>,
}

impl HeldEquipmentSchedule {
    /// Admits new hand changes only against newly observed local physics ticks.
    pub(super) fn observe(
        &mut self,
        movement: &MovementTicker,
        recent_ticks: usize,
        mainhand_changed: bool,
        offhand_changed: bool,
    ) {
        let scope = movement.interaction_authority_identity();
        if !movement.physics_is_authorized() {
            *self = Self::default();
            return;
        }
        if self.scope != Some(scope) {
            *self = Self {
                scope: Some(scope),
                ..Self::default()
            };
        }
        let completed = movement.completed_tick();
        let first = if self
            .observed_completed_tick
            .is_none_or(|tick| completed > tick)
        {
            movement
                .first_unsent_sample_in_frame(recent_ticks)
                .and_then(|sample| {
                    let tick = self
                        .observed_completed_tick
                        .map_or(sample.tick, |previous| {
                            sample.tick.max(previous.saturating_add(1))
                        });
                    movement.unsent_sample_at(tick).map(|_| tick)
                })
        } else {
            None
        };
        self.observed_completed_tick = Some(completed);
        for (changed, pending) in [
            (mainhand_changed, &mut self.mainhand),
            (offhand_changed, &mut self.offhand),
        ] {
            if !changed {
                *pending = None;
            } else if pending.is_none() {
                *pending = first;
            }
        }
    }

    /// Checks whether this input may carry the still-unadmitted mainhand change.
    pub(super) fn mainhand_due(&self, tick: u64) -> bool {
        self.mainhand.is_some_and(|first| tick >= first)
    }

    /// Checks whether this input may carry the still-unadmitted offhand change.
    pub(super) fn offhand_due(&self, tick: u64) -> bool {
        self.offhand.is_some_and(|first| tick >= first)
    }

    /// Clears only the hand whose current packet entered the transport FIFO.
    pub(super) fn mainhand_admitted(&mut self) {
        self.mainhand = None;
    }

    /// Clears only the hand whose current packet entered the transport FIFO.
    pub(super) fn offhand_admitted(&mut self) {
        self.offhand = None;
    }
}
