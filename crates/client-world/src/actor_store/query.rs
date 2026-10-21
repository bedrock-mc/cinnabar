use super::*;
use crate::{
    ActorEquipmentSnapshot, RemoteActionSnapshot, RemoteActionStats, item::ActorArmorSnapshot,
};
use actor_animation::{ActorAnimationStats, ActorRigSnapshot};
use protocol_ids::{ActorDataKey, Attribute};

impl ActorStore {
    /// The newest sequence the store applied; it rejects anything at or below it.
    pub(crate) const fn latest_sequence(&self) -> u64 {
        self.latest_sequence
    }

    pub(crate) fn ridden_unique_id(&self, rider_unique_id: i64) -> Option<i64> {
        self.rider_to_ridden.get(&rider_unique_id).copied()
    }

    pub(crate) fn player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        let actor = self.actors.get(&runtime_id)?;
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            return None;
        };
        let profile = if self.animation.has_skin_preparation() {
            self.ready_appearances.profiles.get(uuid)
        } else {
            self.players
                .get(uuid)
                .or_else(|| self.unlisted_players.get(uuid))
        };
        profile.filter(|profile| profile.unique_id == actor.unique_id)
    }

    /// Reads the current roster identity independently of skin preparation or display names.
    pub(crate) fn player_roster_identity(&self, runtime_id: u64) -> Option<(&str, &[u8; 16])> {
        let actor = self.actors.get(&runtime_id)?;
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            return None;
        };
        self.players
            .get(uuid)
            .filter(|profile| profile.unique_id == actor.unique_id)
            .map(|profile| (&*profile.xuid, uuid))
    }

    pub(crate) fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        let runtime_id = self.unique_to_runtime.get(&unique_id)?;
        let actor = self.actors.get(runtime_id)?;
        let name = match &actor.kind {
            ActorKind::Player { username, .. } => std::sync::Arc::clone(username),
            ActorKind::Entity { .. } => {
                let ActorMetadataValue::String(name) =
                    actor.metadata.get(&ActorDataKey::Name.wire_value())?
                else {
                    return None;
                };
                std::sync::Arc::clone(name)
            }
        };
        (!name.is_empty()).then_some(name)
    }

    /// Rendered name tags use synced actor data, including overrides on players.
    /// An empty tag hides it; a missing player tag uses its spawn name after appearance readiness.
    pub(crate) fn actor_name_tag(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        let actor = self.snapshot_by_unique(unique_id)?;
        if !self.appearance_ready(actor) {
            return None;
        }
        let name = match actor.metadata.get(&ActorDataKey::Name.wire_value()) {
            Some(ActorMetadataValue::String(name)) => std::sync::Arc::clone(name),
            _ => match &actor.kind {
                ActorKind::Player { username, .. } => std::sync::Arc::clone(username),
                ActorKind::Entity { .. } => return None,
            },
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

    /// Named player-list entries as `(username, xuid, uuid)`, sorted by username; the XUID may be empty.
    pub(crate) fn player_list_roster(
        &self,
    ) -> Vec<(std::sync::Arc<str>, std::sync::Arc<str>, [u8; 16])> {
        let mut roster = self
            .players
            .iter()
            .filter(|(_, profile)| !profile.username.is_empty())
            .map(|(uuid, profile)| {
                (
                    std::sync::Arc::clone(&profile.username),
                    std::sync::Arc::clone(&profile.xuid),
                    *uuid,
                )
            })
            .collect::<Vec<_>>();
        roster.sort_unstable();
        roster
    }

    /// Borrows the retained skin of a listed UUID, whether or not its actor has spawned.
    pub(crate) fn player_list_skin(&self, uuid: &[u8; 16]) -> Option<&PlayerSkin> {
        self.players.get(uuid).map(|profile| &profile.skin)
    }

    /// Counts named entries in the authoritative roster.
    pub(crate) fn player_list_count(&self) -> usize {
        self.players
            .values()
            .filter(|profile| !profile.username.is_empty())
            .count()
    }

    /// Non-spectator players present in the world, whether or not their appearance is drawable.
    pub(crate) fn present_players(
        &self,
        excluded_runtime_id: Option<u64>,
    ) -> impl Iterator<Item = &ActorSnapshot> {
        self.actors.values().filter(move |actor| {
            Some(actor.runtime_id) != excluded_runtime_id
                && matches!(actor.kind, ActorKind::Player { .. })
                && self.camera_aim_assist_eligible(actor) == Some(true)
        })
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
                if !self.appearance_ready(actor) {
                    return None;
                }
                let profile = self.player_profile(actor.runtime_id);
                Some((actor, profile))
            })
            .collect::<Vec<_>>();
        players.sort_unstable_by_key(|(actor, _)| actor.runtime_id);
        players
    }
    pub(crate) fn snapshot_by_unique(&self, unique_id: i64) -> Option<&ActorSnapshot> {
        self.actors.get(self.unique_to_runtime.get(&unique_id)?)
    }

    /// Resolves an actor's authoritative health attribute by unique id, for
    /// the mount-health HUD row. Non-finite or inverted values fail closed.
    pub(crate) fn health_by_unique(&self, unique_id: i64) -> Option<(f32, f32)> {
        let runtime_id = self.unique_to_runtime.get(&unique_id)?;
        let health = self
            .actors
            .get(runtime_id)?
            .attributes
            .get(Attribute::Health.wire_name())?;
        let (current, maximum) = (health.current, health.max);
        if !current.is_finite() || !maximum.is_finite() || maximum <= 0.0 || current < 0.0 {
            return None;
        }
        Some((current.min(maximum), maximum))
    }

    /// Position and view angles `(position, yaw, pitch)` of the actor with this unique id.
    pub(crate) fn pose_by_unique(&self, unique_id: i64) -> Option<([f32; 3], f32, f32)> {
        let actor = self.actors.get(self.unique_to_runtime.get(&unique_id)?)?;
        Some((actor.position, actor.yaw, actor.pitch))
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

    /// Reads the current numeric registry's intrinsic shine disposition.
    pub(crate) fn item_glint(&self, network_id: i32) -> assets::item::glint::ItemGlint {
        self.items.item_glint(network_id)
    }
    pub(crate) fn item_identifier(&self, network_id: i32) -> Option<std::sync::Arc<str>> {
        self.items.identifier_for_network_id(network_id)
    }

    /// Reads protection resolved for this numeric item registry entry.
    pub(crate) fn item_armour(&self, network_id: i32) -> assets::item::armour::ItemArmour {
        self.items.item_armour(network_id)
    }

    /// Reads durability resolved for this numeric item registry entry.
    pub(crate) fn item_durability(
        &self,
        network_id: i32,
    ) -> assets::item::durability::ItemDurability {
        self.items.item_durability(network_id)
    }

    pub(crate) fn seed_item_registry(&mut self, registry: protocol::ItemRegistryEvent) -> bool {
        self.items.apply_registry(registry)
    }

    /// Retains the identity of the facts used to resolve item presentation.
    pub(crate) fn item_presentation_generation(&self) -> &std::sync::Arc<()> {
        self.items.presentation_generation()
    }

    pub(crate) fn get(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(&runtime_id)
    }
    /// Returns numerical use admission shared by gameplay, presentation and audio.
    pub(crate) fn item_use_profile(&self, network_id: i32) -> crate::EffectiveItemUse {
        self.items.item_use_profile(network_id)
    }
    /// Retrieves the item's effective attack facts without guessing from its name.
    pub(crate) fn item_attack_timing(
        &self,
        identifier: &str,
    ) -> Option<&protocol::ItemAttackTiming> {
        self.items.attack_timing(identifier)
    }
    pub(crate) fn len(&self) -> usize {
        self.actors.len()
    }
    pub(crate) fn actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        self.actors.values()
    }
    pub(crate) fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.appearance_ready(self.actors.get(&runtime_id)?)
            .then(|| self.animation.get(runtime_id))
            .flatten()
    }
    pub(crate) fn render_frame(&self, partial_tick: f32) -> actor_animation::ActorRenderFrame<'_> {
        actor_animation::ActorRenderFrame::new(
            &self.animation,
            &self.actors,
            self.camera_rotation,
            self.camera_position,
            partial_tick,
        )
    }
    /// Full-body pose for the local HUD while first-person hands have a separate pose.
    pub(crate) fn actor_ui_pose(
        &self,
        runtime_id: u64,
    ) -> Option<&[actor_animation::BoneTransform]> {
        self.animation.ui_pose(runtime_id)
    }
    pub(crate) fn actor_retargeted_pose(
        &self,
        runtime_id: u64,
        alpha: f32,
        targets: &[Option<actor_animation::BoneTransform>],
    ) -> Option<Vec<actor_animation::BoneTransform>> {
        self.animation.retargeted_pose(runtime_id, alpha, targets)
    }
    /// Borrows the retargeted pose while retaining tick transforms in the caller's cache.
    pub(crate) fn actor_retargeted_pose_cached<'c>(
        &self,
        runtime_id: u64,
        alpha: f32,
        targets: &[Option<actor_animation::BoneTransform>],
        cache: &'c mut actor_animation::JavaRetargetCache,
    ) -> Option<&'c [actor_animation::BoneTransform]> {
        self.animation
            .retargeted_pose_cached(runtime_id, alpha, targets, cache)
    }
    pub(crate) fn actor_retargeted_layers(
        &self,
        runtime_id: u64,
        alpha: f32,
        targets: impl FnMut(
            &[Box<str>],
            &[actor_animation::BoneTransform],
            &mut Vec<Option<actor_animation::BoneTransform>>,
        ) -> Option<()>,
        cache: &mut actor_animation::JavaRetargetCache,
    ) -> Option<Vec<actor_animation::SkinRenderLayer>> {
        self.animation
            .retargeted_layers(runtime_id, alpha, targets, cache)
    }
    pub(crate) fn actor_rigs(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.animation.snapshots().filter(|rig| {
            self.actors
                .get(&rig.actor.runtime_id)
                .is_some_and(|actor| self.appearance_ready(actor))
        })
    }
    pub(crate) fn actor_particle_controllers(
        &self,
    ) -> impl Iterator<Item = actor_animation::ActorParticleController<'_>> {
        self.animation.particle_controllers()
    }
    pub(crate) const fn animation_stats(&self) -> ActorAnimationStats {
        self.animation.stats()
    }
    pub(crate) fn equipment(&self, runtime_id: u64) -> Option<&ActorEquipmentSnapshot> {
        self.items.get(self.lifetime(runtime_id)?)
    }
    pub(crate) fn equipment_in_hand(
        &self,
        runtime_id: u64,
        hand: protocol::ActorHandedness,
    ) -> Option<&ActorEquipmentSnapshot> {
        self.items.get_in_hand(self.lifetime(runtime_id)?, hand)
    }
    pub(crate) fn armor(&self, runtime_id: u64) -> Option<&ActorArmorSnapshot> {
        self.items.armor(runtime_id)
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
