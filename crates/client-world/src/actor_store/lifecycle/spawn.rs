use super::*;
use protocol_ids::Attribute;

impl ActorStore {
    /// Replaces conflicting IDs and installs one authoritative actor lifetime.
    pub(in crate::actor_store) fn apply_spawn(
        &mut self,
        sequence: u64,
        spawn: ActorSpawnEvent,
    ) -> ActorApplyResult {
        let replaces_runtime = self.actors.contains_key(&spawn.runtime_id);
        let replaces_unique = self.unique_to_runtime.contains_key(&spawn.unique_id);
        if self.actors.len() >= self.max_actors && !replaces_runtime && !replaces_unique {
            return ActorApplyResult::CapacityRejected;
        }
        if spawn
            .links
            .iter()
            .any(|link| link.dimension != self.dimension)
        {
            return ActorApplyResult::StaleDimension;
        }

        let links = std::sync::Arc::clone(&spawn.links);
        let mut replaced = false;
        if let Some(previous) = self.actors.remove(&spawn.runtime_id) {
            self.seat_bindings.remove(&previous.runtime_id);
            self.effect_members.remove(previous.runtime_id);
            self.synchronized_audio.remove_runtime(previous.runtime_id);
            let lifetime = self.lifetime_for(&previous);
            self.unique_to_runtime.remove(&previous.unique_id);
            self.animation.remove_runtime(previous.runtime_id);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
            self.remove_links_for(previous.unique_id);
            replaced = true;
        }
        if let Some(previous_runtime) = self.unique_to_runtime.remove(&spawn.unique_id) {
            self.seat_bindings.remove(&previous_runtime);
            if let Some(previous) = self.actors.remove(&previous_runtime) {
                self.effect_members.remove(previous.runtime_id);
                self.synchronized_audio.remove_runtime(previous.runtime_id);
                let lifetime = self.lifetime_for(&previous);
                self.items.remove(lifetime);
                self.actions.remove(lifetime);
                self.remove_links_for(previous.unique_id);
            }
            self.animation.remove_runtime(previous_runtime);
            replaced = true;
        }
        let runtime_id = spawn.runtime_id;
        let unique_id = spawn.unique_id;
        let held_item = spawn.held_item.clone();
        let interaction_traits = Self::resolve_actor_interactions(
            &spawn.kind,
            self.entity_assets.as_deref(),
            &self.custom_actor_facts,
        );
        let fact = self.resolve_entity_fact(&spawn.kind);
        let mut actor = ActorSnapshot::from_spawn(spawn, sequence, fact, &mut self.hitbox_budget);
        actor.interaction_traits = interaction_traits;
        self.bind_actor_seat(&actor);
        self.effect_members.update(&mut actor);
        self.actors.insert(runtime_id, actor);
        self.queue_rocket_launch_audio(runtime_id);
        if self.remote_state_excluded_runtime_id == Some(runtime_id) {
            if self.actors[&runtime_id]
                .attributes
                .contains_key(Attribute::Health.wire_name())
            {
                self.pending_local_health = None;
                self.pending_local_damage = None;
            } else {
                self.apply_pending_local_health(runtime_id);
            }
            self.apply_pending_local_death(runtime_id);
            self.apply_held_local_metadata(runtime_id);
        }
        self.unique_to_runtime.insert(unique_id, runtime_id);
        self.board_linked_riders();
        // A respawned boat restarts its prediction from the new spawn.
        self.note_local_vehicle_pose(runtime_id);
        self.adopt_spawned_unique_id(runtime_id);
        self.prune_unlisted_players();
        if let Some(actor) = self.actors.get(&runtime_id) {
            self.animation
                .insert(self.session_id, self.dimension, actor);
            if self.remote_state_excluded_runtime_id != Some(runtime_id) {
                self.items
                    .insert_spawn(self.lifetime_for(actor), sequence, held_item);
            }
        }
        for link in links.iter().copied() {
            if self.apply_link_inner(link) == ActorApplyResult::CapacityRejected {
                return ActorApplyResult::CapacityRejected;
            }
        }
        if replaced {
            ActorApplyResult::Replaced
        } else {
            ActorApplyResult::Inserted
        }
    }
}
