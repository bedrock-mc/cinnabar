use super::*;
use crate::actor_animation::{ActorAnimationStats, ActorRigSnapshot};
use crate::{ActorEquipmentSnapshot, RemoteActionSnapshot, RemoteActionStats};

impl ActorStore {
    pub(crate) fn player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        let actor = self.actors.get(&runtime_id)?;
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            return None;
        };
        self.players
            .get(uuid)
            .filter(|profile| profile.unique_id == actor.unique_id)
    }

    pub(crate) fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        let runtime_id = self.unique_to_runtime.get(&unique_id)?;
        let actor = self.actors.get(runtime_id)?;
        let name = match &actor.kind {
            ActorKind::Player { username, .. } => std::sync::Arc::clone(username),
            ActorKind::Entity { .. } => {
                let ActorMetadataValue::String(name) = actor.metadata.get(&NAMETAG_METADATA_KEY)?
                else {
                    return None;
                };
                std::sync::Arc::clone(name)
            }
        };
        (!name.is_empty()).then_some(name)
    }

    /// Every username on the retained authoritative player list, sorted for
    /// deterministic presentation (the `@a` selector's known answer).
    pub(crate) fn player_list_usernames(&self) -> Vec<std::sync::Arc<str>> {
        let mut names = self
            .players
            .values()
            .map(|profile| std::sync::Arc::clone(&profile.username))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    pub(crate) fn render_players(
        &self,
        excluded_runtime_id: Option<u64>,
    ) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        let mut players = self
            .actors
            .values()
            .filter(|actor| Some(actor.runtime_id) != excluded_runtime_id)
            .filter_map(|actor| {
                let ActorKind::Player { .. } = &actor.kind else {
                    return None;
                };
                let profile = self.player_profile(actor.runtime_id);
                Some((actor, profile))
            })
            .collect::<Vec<_>>();
        players.sort_unstable_by_key(|(actor, _)| actor.runtime_id);
        players
    }
    /// Resolves an actor's authoritative health attribute by unique id, for
    /// the mount-health HUD row. Non-finite or inverted values fail closed.
    pub(crate) fn health_by_unique(&self, unique_id: i64) -> Option<(f32, f32)> {
        let runtime_id = self.unique_to_runtime.get(&unique_id)?;
        let health = self
            .actors
            .get(runtime_id)?
            .attributes
            .get("minecraft:health")?;
        let (current, maximum) = (health.current, health.max);
        if !current.is_finite() || !maximum.is_finite() || maximum <= 0.0 || current < 0.0 {
            return None;
        }
        Some((current.min(maximum), maximum))
    }

    /// Whether the actor with this unique id carries a named attribute, for
    /// capability gates like the mount jump-strength check.
    pub(crate) fn actor_has_attribute_by_unique(&self, unique_id: i64, name: &str) -> bool {
        self.unique_to_runtime
            .get(&unique_id)
            .and_then(|runtime_id| self.actors.get(runtime_id))
            .is_some_and(|actor| actor.attributes.contains_key(name))
    }

    /// Resolves one wire item stack against the retained item registry and
    /// compiled visual routes without mutating any actor state.
    pub(crate) fn canonical_item_stack(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<crate::item::CanonicalItemStack> {
        self.items.canonicalize(stack)
    }

    pub(crate) fn get(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(&runtime_id)
    }
    pub(crate) fn len(&self) -> usize {
        self.actors.len()
    }
    pub(crate) fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.animation.get(runtime_id)
    }
    pub(crate) fn actor_rigs(&self) -> Vec<ActorRigSnapshot<'_>> {
        self.animation.snapshots()
    }
    pub(crate) const fn animation_stats(&self) -> ActorAnimationStats {
        self.animation.stats()
    }
    pub(crate) fn equipment(&self, runtime_id: u64) -> Option<&ActorEquipmentSnapshot> {
        self.items.get(self.lifetime(runtime_id)?)
    }
    pub(crate) fn action(&self, runtime_id: u64) -> Option<&RemoteActionSnapshot> {
        self.actions.get(self.lifetime(runtime_id)?)
    }
    pub(crate) fn action_history(&self, runtime_id: u64) -> &[RemoteActionSnapshot] {
        self.lifetime(runtime_id)
            .map_or(&[], |lifetime| self.actions.history(lifetime))
    }
    pub(crate) const fn action_stats(&self) -> RemoteActionStats {
        self.actions.stats()
    }
    pub(crate) fn pending_item_resolution_count(&self) -> usize {
        self.items.pending_count()
    }
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn player_count(&self) -> usize {
        self.players.len()
    }
}
