//! Position-only server authority retained with the next completed controller frame.

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ActorPositionUpdate {
    feet: Vec3,
    on_ground: bool,
}

impl ActorPositionUpdate {
    /// Drops previous-position contacts while preserving velocity, abilities and queued impulses.
    pub(super) fn apply(self, state: &mut PlayerState) {
        state.set_position(self.feet);
        state.collisions = sim::AxisCollisions::default();
        state.on_ground = self.on_ground;
    }

    /// Replaces spatial authority without replacing the correction's velocity authority.
    pub(super) fn replace_anchor_position(self, anchor: &mut super::super::PhysicsAnchor) {
        anchor.network_position = [
            self.feet.x as f32,
            self.feet.y as f32 + PLAYER_NETWORK_OFFSET,
            self.feet.z as f32,
        ];
        anchor.on_ground = self.on_ground;
    }
}

impl LocalPhysicsController {
    /// Explicit teleports supersede prior placements while preserving the other replay controls.
    pub(in crate::movement) fn discard_server_actor_positions(&mut self) {
        self.actor_position_pending = None;
        for frame in &mut self.controller_history {
            frame.actor_position = None;
        }
    }

    /// Finds the latest retained placement after a correction, including the unrecorded next frame.
    pub(super) fn actor_position_after(&self, tick: u64) -> Option<(u64, ActorPositionUpdate)> {
        if let (Some(position), Some(state)) = (self.actor_position_pending, self.state.as_ref()) {
            let boundary = state.tick.saturating_add(1);
            if boundary > tick {
                return Some((boundary, position));
            }
        }
        self.controller_history.iter().rev().find_map(|frame| {
            (frame.tick > tick)
                .then_some(frame.actor_position)
                .flatten()
                .map(|position| (frame.tick, position))
        })
    }

    /// Adopts a marked server actor position and retains its next-tick replay boundary.
    pub fn apply_server_actor_position(&mut self, position: [f32; 3], on_ground: Option<bool>) {
        if !position.into_iter().all(f32::is_finite) {
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let update = ActorPositionUpdate {
            feet: Vec3::new(
                f64::from(position[0]),
                f64::from(position[1] - PLAYER_NETWORK_OFFSET),
                f64::from(position[2]),
            ),
            on_ground: on_ground.unwrap_or(state.on_ground),
        };
        update.apply(state);
        self.previous_position = update.feet;
        self.actor_position_pending = Some(update);
        self.anchor_state.note_hard_anchor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty, loaded world gives movement and replay identical collision evidence.
    struct Air;

    impl CollisionWorld for Air {
        fn collision_boxes(
            &self,
            _: sim::Aabb,
        ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
            Ok(sim::CollisionQuery::synthetic(Vec::new()))
        }
    }

    #[test]
    fn actor_position_discards_all_axis_contacts_but_keeps_authoritative_ground_state() {
        for previous_ground in [false, true] {
            for ground in [None, Some(false), Some(true)] {
                let mut physics = LocalPhysicsController::default();
                physics.reanchor_network_position([0.0, 4.0, 0.0], 100, previous_ground);
                physics.state.as_mut().unwrap().collisions = sim::AxisCollisions {
                    x: true,
                    y: true,
                    z: true,
                };
                physics.apply_server_actor_position([1.0, 4.5, 2.0], ground);
                let state = physics.state().unwrap();
                assert_eq!(state.collisions, sim::AxisCollisions::default());
                assert_eq!(state.on_ground, ground.unwrap_or(previous_ground));
            }
        }
    }

    #[test]
    fn actor_position_preserves_motion_input_and_fractional_tick_time() {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 4.0, 0.0], 100, false);
        physics.queue_server_motion([0.25, 0.125, -0.0625], 101);
        physics.previous_jump_held = true;
        physics.jump_edge_pending = true;
        physics.accumulated_seconds = 0.025;
        physics.state.as_mut().unwrap().jump_delay = 4;
        let mut expected = physics.state().unwrap().clone();
        let motions = physics.server_motions.clone();
        physics.apply_server_actor_position([1.0, 4.5, 2.0], Some(true));
        expected.set_position(Vec3::new(1.0, f64::from(4.5 - PLAYER_NETWORK_OFFSET), 2.0));
        expected.on_ground = true;
        assert_eq!(physics.state(), Some(&expected));
        assert_eq!(physics.server_motions, motions);
        assert!(physics.previous_jump_held && physics.jump_edge_pending);
        assert_eq!(physics.ticks_due(Duration::from_millis(25)), 1);
        physics.reanchor_network_position([0.0, 8.0, 0.0], 101, false);
        assert!(physics.actor_position_pending.is_none());
    }

    #[test]
    fn correction_before_the_next_tick_preserves_pending_actor_position() {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 4.0, 0.0], 100, false);
        let sample = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &Air,
            )
            .samples
            .remove(0);
        let position = [1.0, 4.5, 2.0];
        physics.apply_server_actor_position(position, Some(true));
        let plan = physics
            .apply_correction(
                super::super::super::PhysicsAnchor {
                    network_position: sample.position,
                    tick: sample.tick,
                    on_ground: sample.grounded_after_tick,
                    velocity: None,
                },
                PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &Air,
            )
            .unwrap();
        assert_eq!(physics.network_position(), Some(position));
        assert_eq!(plan.final_position, position);
        assert!(physics.state().unwrap().on_ground);
        assert!(physics.actor_position_pending.is_some());
    }

    #[test]
    fn actor_position_is_reapplied_when_a_correction_replays_across_its_boundary() {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 4.0, 0.0], 100, false);
        let sample = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &Air,
            )
            .samples
            .remove(0);
        physics.queue_server_motion([0.25, 0.125, -0.0625], 101);
        physics.apply_server_actor_position([1.0, 4.5, 2.0], Some(false));
        let frame = physics.advance(
            simulation_clock::TICK_DURATION * 2,
            MovementInput::default(),
            &Air,
        );
        assert_eq!(frame.completed_ticks, 2);
        let expected = physics.state().unwrap().clone();
        let plan = physics
            .apply_correction(
                super::super::super::PhysicsAnchor {
                    network_position: sample.position,
                    tick: sample.tick,
                    on_ground: sample.grounded_after_tick,
                    velocity: None,
                },
                PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &Air,
            )
            .unwrap();
        assert_eq!(
            plan.outcome,
            PhysicsCorrectionOutcome::Replayed {
                corrected_tick: sample.tick,
                replayed_ticks: 2
            }
        );
        assert_eq!(physics.state().unwrap().position, expected.position);
        assert_eq!(physics.state().unwrap().velocity, expected.velocity);
    }
}
