use super::*;
use crate::actor_animation::{
    ActorAnimationStats, ActorRigSnapshot, LocalPlayerAnimationTickInput, LocalPlayerRigResolution,
    LocalPlayerRigSnapshot,
};
use crate::{ActorEquipmentSnapshot, RemoteActionSnapshot, RemoteActionStats};

impl ActorStore {
    pub(crate) fn player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        let actor = self.actors.get(&runtime_id)?;
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            return None;
        };
        if self.player_unique_ids.get(&actor.unique_id)?.as_ref() != Some(uuid) {
            return None;
        }
        self.players
            .get(uuid)
            .filter(|profile| profile.unique_id == actor.unique_id)
    }

    pub(crate) fn player_profile_by_unique_id(&self, unique_id: i64) -> Option<&PlayerProfile> {
        self.player_profile_lookup_by_unique_id(unique_id)
            .ok()
            .flatten()
    }

    pub(crate) fn player_profile_lookup_by_unique_id(
        &self,
        unique_id: i64,
    ) -> Result<Option<&PlayerProfile>, ()> {
        let Some(uuid) = self.player_unique_ids.get(&unique_id) else {
            return Ok(None);
        };
        let Some(uuid) = uuid.as_ref() else {
            return Err(());
        };
        Ok(self
            .players
            .get(uuid)
            .filter(|profile| profile.unique_id == unique_id))
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
    pub(crate) fn local_player_rig(
        &self,
        geometry: &protocol::PlayerSkinGeometry,
    ) -> Option<LocalPlayerRigSnapshot<'_>> {
        self.animation.local_player(geometry)
    }
    pub(crate) fn local_player_rig_resolution(
        &self,
        geometry: &protocol::PlayerSkinGeometry,
    ) -> LocalPlayerRigResolution {
        self.animation.local_player_resolution(geometry)
    }
    pub(crate) fn advance_local_player_animation(&mut self, input: LocalPlayerAnimationTickInput) {
        self.animation.advance_local_player_tick(input);
    }
    pub(crate) fn reset_local_player_animation(&mut self) {
        self.animation.reset_local_player();
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
