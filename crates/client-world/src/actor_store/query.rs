use super::*;
use crate::actor_animation::{ActorAnimationStats, ActorRigSnapshot};

impl ActorStore {
    pub(crate) fn render_players(
        &self,
        excluded_runtime_id: Option<u64>,
    ) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        let mut players = self
            .actors
            .values()
            .filter(|actor| Some(actor.runtime_id) != excluded_runtime_id)
            .filter_map(|actor| {
                let ActorKind::Player { uuid, .. } = &actor.kind else {
                    return None;
                };
                let profile = self
                    .players
                    .get(uuid)
                    .filter(|profile| profile.unique_id == actor.unique_id);
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
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn player_count(&self) -> usize {
        self.players.len()
    }
}
