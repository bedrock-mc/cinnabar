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
