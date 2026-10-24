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
                self.local_actor_revision,
                EntityFacts::pinned().player(),
                &mut self.hitbox_budget,
            )
        });
        update(&mut actor, &mut self.hitbox_budget);
        self.pending_local_actor = Some(actor);
    }

    /// Starts the source lifetime before any data for its presentation actor can arrive.
    pub(crate) fn replace_local_actor_lifetime(&mut self, revision: u64) {
        self.pending_local_actor = None;
        self.local_actor_revision = revision;
    }

    /// Returns the stable source lifetime, including while its presentation actor is absent.
    pub(crate) fn local_actor_revision(&self) -> u64 {
        self.local_actor_revision
    }

    /// Reads accepted local data before or after presentation actor creation.
    pub(crate) fn actor_authority(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(&runtime_id).or_else(|| {
            self.pending_local_actor
                .as_ref()
                .filter(|actor| actor.runtime_id == runtime_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unspawned_local_data_does_not_cross_dimension_reset() {
        let mut store = ActorStore::new(1, 0);
        store.exclude_remote_state_for(7);
        let name = protocol_ids::Attribute::Movement.wire_name();
        store.apply(
            1,
            1,
            ActorEvent::Attributes(protocol::ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                tick: 0,
                attributes: [ActorAttribute {
                    name: name.into(),
                    min: 0.0,
                    max: 1.0,
                    current: 0.2,
                    default: Some(0.2),
                    modifiers: [].into(),
                }]
                .into(),
            }),
        );
        store.reset_dimension(1, 2, 1);
        store.sync_local_player(7, 70, &super::super::local_tests::local_feed(0.0, 0.0));
        assert!(!store.actors[&7].attributes.contains_key(name));
    }
}
