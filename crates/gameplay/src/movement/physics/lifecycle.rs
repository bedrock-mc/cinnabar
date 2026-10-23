//! Resets and anchors the local movement controller.

use super::*;

impl LocalPhysicsController {
    /// Clears session-owned prediction, accepted sync state and pending correction work.
    pub fn deactivate(&mut self) {
        self.prediction_sync.clear();
        self.sync_state.reset();
        self.state = None;
        self.eye_offset = eye::LocalEyeOffset::default();
        self.visual_correction = visual_correction::VisualCorrection::default();
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.skip_travel_pending = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.jump_pulse_pending = false;
        self.jump_pulse_held = false;
        self.jump_pulse_scope = None;
        self.processed_jump_arc_active = false;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.actor_position_pending = None;
        self.deferred_corrections = Default::default();
        self.server_control_flags = None;
        self.live_posture = Default::default();
        self.riptide_pending = None;
        self.boosts.clear();
        self.modes.reset();
        self.last_environment = sim::MovementEnvironment::default();
        self.dimension_waiting = false;
        self.anchor_state.reset();
        self.history = PredictionHistory::new(self.history_capacity)
            .expect("local physics history capacity is non-zero");
    }

    /// Replaces prediction state from a server network-position anchor.
    ///
    /// Bedrock player movement positions carry the protocol network offset;
    /// collision simulation uses the feet origin. Non-finite anchors disable
    /// local prediction instead of allowing invalid state to reach collision.
    ///
    /// This is a hard reset used by StartGame, session/dimension replacement,
    /// teleports, and un-replayable corrections. It deliberately clears the
    /// retained axis collisions along with the rest of the history: no prior
    /// motion survives the reset, so nothing can justify the discrete
    /// ladder-climb branch until a fresh tick re-derives them.
    pub fn reanchor_network_position(
        &mut self,
        network_position: [f32; 3],
        tick: u64,
        on_ground: bool,
    ) {
        if !network_position.into_iter().all(f32::is_finite) {
            self.deactivate();
            return;
        }
        let feet = Vec3::new(
            f64::from(network_position[0]),
            f64::from(network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(network_position[2]),
        );
        let mut state = PlayerState::new(feet);
        state.tick = tick;
        state.on_ground = on_ground;
        self.state = Some(state);
        self.previous_position = feet;
        self.eye_offset = eye::LocalEyeOffset::default();
        self.visual_correction = visual_correction::VisualCorrection::default();
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.skip_travel_pending = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.jump_pulse_pending = false;
        self.jump_pulse_held = false;
        self.jump_pulse_scope = None;
        self.processed_jump_arc_active = false;
        self.dropped_tick_count = 0;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.actor_position_pending = None;
        self.deferred_corrections = Default::default();
        self.modes.reset();
        self.live_posture = Default::default();
        self.riptide_pending = None;
        self.last_environment = sim::MovementEnvironment::default();
        self.dimension_waiting = false;
        // Every hard anchor starts a fresh bounded probe epoch: the new
        // position is probed before its first simulated tick, and any prior
        // failure budget or frozen embedded-anchor hold is replaced.
        self.anchor_state.note_hard_anchor();
        self.history = PredictionHistory::new(self.history_capacity)
            .expect("local physics history capacity is non-zero");
    }
}
