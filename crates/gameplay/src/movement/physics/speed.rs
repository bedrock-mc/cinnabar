//! Movement attributes advance with local sprint edges, including correction replay.

use super::*;
use crate::movement::speed_authority::EffectiveMovementSpeed;

impl LocalPhysicsController {
    /// Returns the modifier state after the latest completed tick.
    pub(in crate::movement) fn latest_movement_speed(&self) -> Option<EffectiveMovementSpeed> {
        Some(self.controller_history.back()?.speed)
    }

    /// Transfers a successful replay's modifier state to the live attribute owner once.
    pub(crate) fn take_replayed_movement_speed(&mut self) -> Option<EffectiveMovementSpeed> {
        let mut speed = self.replayed_speed.take()?;
        speed.adopt_server_sprinting(self.unrecorded_live_sprinting());
        Some(speed)
    }
}

impl ControllerFrame {
    /// Applies accepted speed edits before this tick's server flag and local sprint edges.
    pub(super) fn prepare_speed(&self, speed: &mut EffectiveMovementSpeed, sprinting: bool) {
        speed.adopt_server_sprinting(Some(sprinting));
        for update in &self.speed_updates {
            update.apply(speed);
        }
        speed.adopt_server_sprinting(self.server_sprinting);
    }

    /// Retains the attribute after local sprint decisions for subsequent ticks and replays.
    pub(super) fn finish_speed(
        &mut self,
        speed: &mut EffectiveMovementSpeed,
        input: &mut MovementInput,
    ) {
        apply_sprint_speed(speed, input, self.start_sprinting);
        self.speed = *speed;
        self.input = *input;
    }
}

/// Applies a local sprint transition before the simulator consumes its input.
pub(super) fn apply_sprint_speed(
    speed: &mut EffectiveMovementSpeed,
    input: &mut MovementInput,
    start_sprinting: bool,
) {
    if start_sprinting {
        speed.set_sprinting(true);
    }
    speed.set_sprinting(input.sprinting);
    if let Some(predicted) = speed.prediction_speed() {
        input.movement_speed = Some(predicted);
    }
}
