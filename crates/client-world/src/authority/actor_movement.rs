use super::WorldAuthority;
use protocol::{ActorMoveEvent, ActorPositionOrigin};
use world::ChunkKey;

/// Forced actor moves snap when their displacement reaches half a block.
const ACTOR_TELEPORT_DISTANCE_SQUARED: f32 = 0.25;

impl WorldAuthority {
    /// Resolves MoveActor snapping against its decoded target and currently resident terrain.
    pub(super) fn resolve_actor_teleport(&self, movement: &mut ActorMoveEvent) {
        if movement.player_mode.is_some()
            || movement.position_origin != ActorPositionOrigin::NetworkOffset
            || movement.runtime_id == self.local_player_runtime_id
            || movement.dimension != self.current_dimension
        {
            return;
        }
        let Some(actor) = self.actors.get(movement.runtime_id) else {
            return;
        };
        let mut target = actor.last_received_native_position();
        for (coordinate, update) in target.iter_mut().zip(movement.position) {
            if let Some(value) = update.filter(|value| value.is_finite()) {
                *coordinate = value;
            }
        }
        let current = actor.native_position();
        let delta: [f32; 3] = std::array::from_fn(|axis| target[axis] - current[axis]);
        let distance_squared = delta[2] * delta[2] + delta[1] * delta[1] + delta[0] * delta[0];
        let column = ChunkKey::new(
            movement.dimension,
            (target[0].floor() as i32).div_euclid(16),
            (target[2].floor() as i32).div_euclid(16),
        );
        movement.teleported = (movement.teleported
            && distance_squared >= ACTOR_TELEPORT_DISTANCE_SQUARED)
            || !self.terrain.contains_column(column);
    }
}

#[cfg(test)]
#[path = "actor_movement_tests.rs"]
mod tests;
