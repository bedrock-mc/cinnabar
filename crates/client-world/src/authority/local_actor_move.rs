//! Server actor positions retained independently of the client-fed local rig.

use super::*;
use protocol::{ActorMoveEvent, ActorPositionOrigin, PLAYER_NETWORK_OFFSET};

/// The newest decoded server target, independent of cross-lane commit order.
#[derive(Debug, Clone, Copy)]
pub(super) struct LocalActorPosition {
    sequences: [u64; 3],
    position: [f32; 3],
}

impl WorldAuthority {
    /// Merges each axis in wire order when actor updates overtake earlier spatial controls.
    pub(super) fn retain_local_actor_position(
        &mut self,
        sequence: u64,
        position: [Option<f32>; 3],
    ) -> [f32; 3] {
        let target = self.local_actor_position.get_or_insert(LocalActorPosition {
            sequences: [0; 3],
            position: self.resolved_server_position.position,
        });
        for (axis, value) in position.into_iter().enumerate() {
            if let Some(value) = value
                && sequence >= target.sequences[axis]
            {
                target.position[axis] = value;
                target.sequences[axis] = sequence;
            }
        }
        target.position
    }

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
        let mut updates = movement.position;
        for (axis, update) in updates.iter_mut().enumerate() {
            *update = update.filter(|value| value.is_finite());
            if let Some(value) = update
                && axis == 1
                && movement.position_origin == ActorPositionOrigin::Feet
            {
                *value += offset;
            }
        }
        let position = self.retain_local_actor_position(sequence, updates);
        if movement.controls_local_actor() {
            let resolved = self.resolve_position(sequence, position);
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
