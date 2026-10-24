//! Keeps one authoritative snapshot while the local actor has no presentation body.

use super::*;

impl ActorStore {
    /// Retains accepted data in the same snapshot type later adopted by synthetic creation.
    pub(super) fn update_unspawned_local_actor(
        &mut self,
        runtime_id: u64,
        update: impl FnOnce(&mut ActorSnapshot, &mut hitbox::Budget),
    ) {
        if self.remote_state_excluded_runtime_id != Some(runtime_id) {
            return;
        }
        let mut actor = self.pending_local_actor.take().unwrap_or_else(|| {
            ActorSnapshot::from_spawn(
                ActorSpawnEvent {
                    dimension: self.dimension,
                    unique_id: 0,
                    runtime_id,
                    kind: ActorKind::Player {
                        uuid: [0; 16],
                        username: "".into(),
                    },
                    position: [0.0; 3],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: [].into(),
                    attributes: [].into(),
                    properties: [].into(),
                    links: [].into(),
                },
                self.latest_sequence,
                EntityFacts::pinned().player(),
                &mut self.hitbox_budget,
            )
        });
        update(&mut actor, &mut self.hitbox_budget);
        self.pending_local_actor = Some(actor);
    }

    /// Discards data belonging to an actor lifetime that has ended or been replaced.
    pub(crate) fn clear_unspawned_local_actor(&mut self) {
        self.pending_local_actor = None;
    }
}
