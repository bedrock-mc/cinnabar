//! Stationary fixed ticks preserve the input clock while destination data loads.

use super::*;

impl LocalPhysicsController {
    pub(in crate::movement) fn advance_dimension_wait(
        &mut self,
        elapsed: Duration,
        yaw: f32,
        context: PhysicsSampleContext,
        registry: sim::CollisionRegistryIdentity,
        effects: &mut impl MovementEffectSource,
    ) -> LocalPhysicsFrame {
        if !self.is_active() {
            return LocalPhysicsFrame::default();
        }
        if !self.dimension_waiting {
            // Loading cannot retain a collision snapshot of absent terrain.
            // Later corrections therefore snap these ticks instead of replaying
            // ordinary gravity through a stationary loading interval.
            self.history = PredictionHistory::new(self.history_capacity)
                .expect("local physics history capacity is non-zero");
            self.sample_history.clear();
            self.controller_history.clear();
            self.motion_ticks.clear();
            self.motion_anchor = None;
            self.server_motions.clear();
            self.deferred_corrections = Default::default();
            self.previous_jump_held = false;
            self.jump_edge_pending = false;
            self.input_edges = Default::default();
            self.fly_toggle_pending = false;
            self.processed_jump_arc_active = false;
            self.modes.reset();
            self.last_environment = sim::MovementEnvironment::default();
            self.eye_offset = eye::LocalEyeOffset::default();
            self.visual_correction = visual_correction::VisualCorrection::default();
            self.dimension_waiting = true;
        }
        let state = self.state.as_mut().expect("active local state checked");
        state.velocity = Vec3::ZERO;
        state.movement = Vec3::ZERO;
        state.requested_movement = Vec3::ZERO;
        state.collisions = sim::AxisCollisions::default();
        state.jump_delay = 0;
        self.previous_position = state.position;
        let mut frame = fixed_ticks::frame(
            elapsed,
            &mut self.accumulated_seconds,
            &mut self.discard_next_elapsed,
        );
        let allowed = frame
            .due_ticks
            .min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;
        // No collision/environment query occurs while immobile. Empty chunk
        // provenance records exactly that, retaining the selected registry.
        let world_identity =
            WorldCollisionIdentity::new(registry, []).expect("empty collision identity is bounded");
        self.last_world_identity = Some(world_identity.clone());
        for tick_index in 0..allowed {
            let Some(tick) = state.tick.checked_add(1) else {
                frame.blocked_tick_index = Some(tick_index);
                frame.blocked = Some(SimulationError::TickOverflow);
                break;
            };
            state.tick = tick;
            effects.commit_successful_tick();
            self.prediction_sync.tick();
            frame.completed_ticks += 1;
            frame.samples.push(PhysicsMovementSample {
                tick,
                position: [
                    state.position.x as f32,
                    state.position.y as f32 + PLAYER_NETWORK_OFFSET,
                    state.position.z as f32,
                ],
                movement: [0.0; 3],
                velocity: [0.0; 3],
                move_vector: [0.0; 2],
                raw_move_vector: [0.0; 2],
                analogue_move_vector: [0.0; 2],
                pitch: context.pitch,
                yaw,
                head_yaw: context.head_yaw,
                camera_orientation: context.camera_orientation,
                jumping: false,
                sneaking: false,
                input: Default::default(),
                sprinting: false,
                input_mode: context.input_mode,
                grounded_before_tick: state.on_ground,
                grounded_after_tick: state.on_ground,
                horizontal_collision: false,
                vertical_collision: false,
                jump_repeated: false,
                processed: ProcessedMovementState::default(),
                world_identity: world_identity.clone(),
            });
        }
        self.dropped_tick_count = self.dropped_tick_count.saturating_add(frame.dropped_ticks);
        frame
    }
}

#[cfg(test)]
#[path = "dimension_wait_tests.rs"]
mod tests;
