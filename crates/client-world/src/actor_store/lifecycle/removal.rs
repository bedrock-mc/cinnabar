//! Retires actor ownership through one path, including spawn ID conflicts.

use super::*;

impl ActorStore {
    /// Removes one actor and all its bindings, advancing a retired local actor's lifetime.
    pub(in crate::actor_store) fn retire_actor(&mut self, sequence: u64, runtime_id: u64) -> bool {
        self.seat_bindings.remove(&runtime_id);
        self.animation.remove_runtime(runtime_id);
        let Some(actor) = self.actors.remove(&runtime_id) else {
            return false;
        };
        if self.remote_state_excluded_runtime_id == Some(runtime_id) {
            self.replace_local_actor_lifetime(sequence);
        }
        self.effect_members.remove(runtime_id);
        self.synchronized_audio.remove_runtime(runtime_id);
        self.unique_to_runtime.remove(&actor.unique_id);
        let lifetime = self.lifetime_for(&actor);
        self.items.remove(lifetime);
        self.actions.remove(lifetime);
        self.remove_links_for(actor.unique_id);
        true
    }

    /// Applies a unique-ID removal with the same retirement used by conflicting spawns.
    pub(super) fn remove_unique(&mut self, sequence: u64, unique_id: i64) -> ActorApplyResult {
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            self.remove_links_for(unique_id);
            return ActorApplyResult::MissingActor;
        };
        self.retire_actor(sequence, runtime_id);
        self.remove_links_for(unique_id);
        self.prune_unlisted_players();
        ActorApplyResult::Removed
    }
}
