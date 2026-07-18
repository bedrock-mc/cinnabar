use protocol::ActorMoveEvent;

use super::*;

/// Maximum committed actor movements retained for one live stream session.
pub const COMMITTED_ACTOR_MOVE_CAPACITY: usize = 4_096;

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedActorPose {
    pub lifetime: ActorLifetimeId,
    pub movement_revision: u64,
    pub previous_pose: ActorPose,
    pub current_pose: ActorPose,
    pub received_pose: ActorPose,
    pub interpolation_ticks_remaining: u8,
    pub on_ground: Option<bool>,
    pub source_tick: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedActorMove {
    pub session_id: u64,
    pub dimension: i32,
    pub sequence: u64,
    pub movement: ActorMoveEvent,
    pub applied: Option<CommittedActorPose>,
}

impl WorldStream {
    pub(super) fn record_committed_actor_move(
        &mut self,
        sequence: u64,
        movement: ActorMoveEvent,
        result: ActorApplyResult,
    ) {
        if self.committed_actor_moves.len() == COMMITTED_ACTOR_MOVE_CAPACITY {
            self.actor_move_commit_dropped_count =
                self.actor_move_commit_dropped_count.saturating_add(1);
            return;
        }
        let applied = matches!(result, ActorApplyResult::Updated)
            .then(|| self.actors.get(movement.runtime_id))
            .flatten()
            .filter(|actor| actor.movement_revision == sequence)
            .map(|actor| CommittedActorPose {
                lifetime: ActorLifetimeId {
                    session_id: self.actor_session_id,
                    dimension: self.current_dimension,
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                },
                movement_revision: actor.movement_revision,
                previous_pose: actor.previous_pose,
                current_pose: ActorPose {
                    position: actor.position,
                    pitch: actor.pitch,
                    yaw: actor.yaw,
                    head_yaw: actor.head_yaw,
                },
                received_pose: actor.received_pose,
                interpolation_ticks_remaining: actor.interpolation_ticks_remaining,
                on_ground: actor.on_ground,
                source_tick: actor.source_tick,
            });
        self.committed_actor_moves.push_back(CommittedActorMove {
            session_id: self.actor_session_id,
            dimension: movement.dimension,
            sequence,
            movement,
            applied,
        });
    }

    pub(super) fn reset_committed_actor_moves(&mut self) {
        self.committed_actor_moves.clear();
        self.actor_move_commit_dropped_count = 0;
    }

    pub fn take_committed_actor_moves(&mut self) -> Vec<CommittedActorMove> {
        self.committed_actor_moves.drain(..).collect()
    }

    #[must_use]
    pub fn pending_actor_move_commit_count(&self) -> usize {
        self.committed_actor_moves.len()
    }

    #[must_use]
    pub const fn actor_move_commit_dropped_count(&self) -> u64 {
        self.actor_move_commit_dropped_count
    }
}
