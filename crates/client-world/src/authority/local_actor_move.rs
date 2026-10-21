//! Server actor positions retained independently of the client-fed local rig.

use super::*;
use protocol::{ActorMoveEvent, ActorPositionOrigin, PLAYER_NETWORK_OFFSET};

impl WorldAuthority {
    /// Retains every decoded target, but publishes only moves allowed to control this client.
    pub(super) fn commit_local_actor_move(&mut self, sequence: u64, movement: ActorMoveEvent) {
        if movement.runtime_id != self.local_player_runtime_id
            || movement.dimension != self.current_dimension
            || movement.player_mode.is_some()
        {
            return;
        }
        let offset = self
            .actors
            .get(self.local_player_runtime_id)
            .map_or(PLAYER_NETWORK_OFFSET, |actor| {
                actor.network_position_offset()
            });
        let mut position = self
            .local_actor_position
            .unwrap_or(self.resolved_server_position.position);
        for (axis, value) in movement.position.into_iter().enumerate() {
            if let Some(mut value) = value.filter(|value| value.is_finite()) {
                if axis == 1 && movement.position_origin == ActorPositionOrigin::Feet {
                    value += offset;
                }
                position[axis] = value;
            }
        }
        self.local_actor_position = Some(position);
        if movement.controls_local_actor() {
            let resolved = self.resolve_position(position);
            self.push_committed_control(CommittedControlEvent::LocalActorMove {
                sequence,
                movement,
                resolved,
            });
        }
    }
}

#[cfg(test)]
mod tests;
