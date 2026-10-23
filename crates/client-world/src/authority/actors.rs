use super::*;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

impl WorldAuthority {
    /// Publishes local placement, reanchoring animation only when presentation changes.
    pub fn set_local_actor_presentation_position(&mut self, position: [f32; 3], reanchor: bool) {
        self.actors.set_local_presentation_position(
            self.local_player_runtime_id,
            position,
            reanchor,
        );
    }

    /// Admits and polls appearance work without advancing animation ticks; true while work remains.
    pub fn poll_actor_appearance_preparation(&mut self) -> bool {
        self.actors.prepare_appearances(false);
        self.actors.appearance_preparation_pending()
    }

    /// Changes only the actor visual after the corresponding terrain mesh is published.
    pub fn apply_actor_block_sync(&mut self, sync: protocol::ActorBlockSyncMessage) -> bool {
        self.actors.apply_terrain_sync(sync)
    }
    /// Borrows remote players and their retained profile records.
    pub fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        self.actors
            .render_players(Some(self.local_player_runtime_id))
    }
    /// Remote non-spectator players, including those whose appearance is still preparing.
    pub fn present_remote_players(&self) -> impl Iterator<Item = &ActorSnapshot> {
        self.actors
            .present_players(Some(self.local_player_runtime_id))
    }
    /// Returns the retained display name for an actor identity.
    pub fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_display_name(unique_id)
    }
    /// Synced actor name tag, distinct from the player's scoreboard username.
    pub fn actor_name_tag(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_name_tag(unique_id)
    }
    /// The title an actor-backed container screen shows: its filtered name when set,
    /// otherwise its name tag.
    pub fn actor_container_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_container_name(unique_id)
    }
    /// Every username on the retained authoritative player list, sorted.
    pub fn player_list_usernames(&self) -> Vec<std::sync::Arc<str>> {
        self.actors.player_list_usernames()
    }
    /// Named player-list entries as `(username, xuid, uuid)`, sorted by username.
    pub fn player_list_roster(&self) -> Vec<(std::sync::Arc<str>, std::sync::Arc<str>, [u8; 16])> {
        self.actors.player_list_roster()
    }
    /// Borrows a listed player's admitted skin without requiring a spawned actor.
    pub fn player_list_skin(&self, uuid: &[u8; 16]) -> Option<&protocol::PlayerSkin> {
        self.actors.player_list_skin(uuid)
    }

    /// Counts named players on the authoritative roster.
    pub fn player_count(&self) -> usize {
        self.actors.player_list_count()
    }
    /// Returns equipment capabilities admitted for this actor, including unresolved entries.
    pub fn actor_mount_equipment(&self, unique_id: i64) -> assets::entity::mount::MountEquipment {
        self.actors
            .snapshot_by_unique(unique_id)
            .map_or(assets::entity::mount::MountEquipment::Unresolved, |actor| {
                self.actors.mount_equipment(actor.runtime_id)
            })
    }
    /// The authoritative `(current, maximum)` health of the actor with this
    /// unique id, if it is known and well-formed.
    pub fn actor_health_by_unique(&self, unique_id: i64) -> Option<(f32, f32)> {
        self.actors.health_by_unique(unique_id)
    }
    /// Position and view angles `(position, yaw, pitch)` of the actor with this unique id.
    pub fn actor_pose_by_unique(&self, unique_id: i64) -> Option<([f32; 3], f32, f32)> {
        self.actors.pose_by_unique(unique_id)
    }
    /// Whether this actor carries a named attribute (capability gate).
    pub fn actor_has_attribute_by_unique(&self, unique_id: i64, name: &str) -> bool {
        self.actors.actor_has_attribute_by_unique(unique_id, name)
    }
    /// Resolves one wire item stack against the retained item registry and
    /// compiled item visual routes.
    pub fn canonical_item_stack(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<crate::item::CanonicalItemStack> {
        self.actors.canonical_item_stack(stack)
    }
    /// Reads intrinsic shine admitted for the current numeric item identity.
    pub fn item_glint(&self, network_id: i32) -> assets::item::glint::ItemGlint {
        self.actors.item_glint(network_id)
    }
    /// The item identifier registered for a network id.
    pub fn item_identifier(&self, network_id: i32) -> Option<std::sync::Arc<str>> {
        self.actors.item_identifier(network_id)
    }

    /// Reads protection resolved for this numeric item registry entry.
    pub fn item_armour(&self, network_id: i32) -> assets::item::armour::ItemArmour {
        self.actors.item_armour(network_id)
    }

    /// Reads durability resolved for this numeric item registry entry.
    pub fn item_durability(&self, network_id: i32) -> assets::item::durability::ItemDurability {
        self.actors.item_durability(network_id)
    }
    /// Installs the StartGame item registry so server-defined item ids resolve
    /// before any play-time registry arrives. False when it is refused.
    pub fn seed_item_registry(&mut self, registry: protocol::ItemRegistryEvent) -> bool {
        self.actors.seed_item_registry(registry)
    }

    /// Identifies the registry and compiled facts used by canonical item resolution.
    pub fn item_presentation_generation(&self) -> &std::sync::Arc<()> {
        self.actors.item_presentation_generation()
    }
    /// Advances simulation ticks with one visual evaluation per tick.
    pub fn advance_actor_interpolation_ticks(&mut self, ticks: u32) {
        self.actors.advance_interpolation_ticks(ticks);
        self.publish_actor_particles();
        self.publish_actor_audio();
    }
    /// Predicts remote actors `ticks` ahead for [`Self::pick_hit_boxes`], leaving the live
    /// actors to [`Self::advance_actor_interpolation_frame`].
    pub fn predict_remote_actor_motion(&mut self, ticks: u32) {
        self.actors.predict_remote_motion(ticks);
    }
    /// Advances elapsed tick state, evaluating animation once for this rendered frame.
    pub fn advance_actor_interpolation_frame(&mut self, ticks: u32) {
        self.actors.advance_interpolation_frame(ticks);
        self.publish_actor_particles();
        self.publish_actor_audio();
    }

    fn publish_actor_particles(&mut self) {
        for event in self.actors.take_particle_effects() {
            self.push_committed_particle(event);
        }
    }
    fn publish_actor_audio(&mut self) {
        for mut event in self.actors.take_synchronized_audio() {
            if event.client_generated {
                event.dimension_epoch = self.form_dimension_epoch;
            }
            self.push_committed_audio(event);
        }
    }
    /// Drains decoded actor status events (hurt, death, taming, totem, ...) for particle and sound consumers.
    pub fn take_actor_status_notices(&mut self) -> Vec<crate::ActorStatusNotice> {
        self.actors.take_status_notices()
    }
    /// The newest sequence the actor store applied; it rejects any older sequence, so every
    /// event that advances it must keep wire order with the others.
    #[must_use]
    pub const fn actor_sequence_frontier(&self) -> u64 {
        self.actors.latest_sequence()
    }
    /// Drains where MobEquipment and MobArmorEquipment events landed, for diagnostics.
    pub fn take_equipment_notices(&mut self) -> Vec<crate::EquipmentNotice> {
        self.actors.take_equipment_notices()
    }
    /// Body boxes for the native liquid probes backing [`Self::set_actor_fluids`].
    pub fn actor_fluid_probes(&self) -> impl Iterator<Item = crate::ActorFluidProbe> + '_ {
        self.actors.fluid_probes()
    }

    /// Bed block under every sleeping actor, for [`Self::set_actor_bed_rotations`] sampling.
    #[must_use]
    pub fn actor_bed_sample_points(&self) -> Vec<(u64, [i32; 3])> {
        self.actors.bed_sample_points()
    }
    /// Records the `(runtime_id, degrees)` bed orientation that backs `query.sleep_rotation`.
    pub fn set_actor_bed_rotations(&mut self, samples: &[(u64, f32)]) {
        self.actors.set_bed_rotations(samples);
    }
    /// Records `(runtime_id, in_water, in_lava)` samples that back the fluid animation queries.
    pub fn set_actor_fluids(&mut self, samples: &[(u64, bool, bool)]) {
        self.actors.set_fluids(samples);
    }
    /// Records `(runtime_id, submerged)` breathing-point samples that hide entity shadows.
    pub fn set_actor_breathing_liquids(&mut self, samples: &[(u64, bool)]) {
        self.actors.set_breathing_liquids(samples);
    }
    /// Every actor's entity-shadow caster at `partial_tick`, local player included.
    pub fn actor_shadow_casters(
        &self,
        partial_tick: f32,
    ) -> impl Iterator<Item = crate::ActorShadowCaster> + '_ {
        self.actors.shadow_casters(partial_tick)
    }
    /// Samples one actor's shadow volume source at the render frame's interpolation fraction.
    pub fn actor_shadow_caster(
        &self,
        runtime_id: u64,
        partial_tick: f32,
    ) -> Option<crate::ActorShadowCaster> {
        self.actors
            .shadow_caster(self.actors.get(runtime_id)?, partial_tick)
    }

    /// Sets the view `[pitch, yaw]` (degrees) that camera-facing billboard rigs sample per tick.
    pub fn set_actor_camera_rotation(&mut self, rotation: [f32; 2]) {
        self.actors.set_camera_rotation(rotation);
    }
    /// Sets the view outside which rigs hold their pose at each tick; `None` animates all.
    pub fn set_actor_animation_view(&mut self, view: Option<actor_animation::ActorAnimationView>) {
        self.actors.set_animation_view(view);
    }
    /// Sets the view's world position that camera-relative queries sample per tick.
    pub fn set_actor_camera_position(&mut self, position: [f32; 3]) {
        self.actors.set_camera_position(position);
    }
    /// Feeds this frame's client-authored local-player pose into the shared actor rig. Call
    /// before [`Self::advance_actor_interpolation_ticks`] and [`Self::actor_rigs`] so the
    /// third-person body and first-person hand read a driven rig instead of a static fallback.
    pub fn sync_local_player_pose(&mut self, feed: &LocalPlayerFeed) {
        self.actors.sync_local_player(
            self.local_player_runtime_id,
            self.local_player_unique_id,
            feed,
        );
    }

    /// Replaces the local appearance while preserving the server's roster identity.
    pub fn update_local_player_skin(&mut self, skin: protocol::PlayerSkin) -> bool {
        let Some(actor) = self.actors.get(self.local_player_runtime_id) else {
            return false;
        };
        let protocol::ActorKind::Player { uuid, .. } = &actor.kind else {
            return false;
        };
        let uuid = *uuid;
        self.actors.apply_skin_update(uuid, skin) == crate::actor_store::ActorApplyResult::Updated
    }
    /// Ordered sources of server swings for the local player since the last call.
    pub fn take_local_server_swings(&mut self) -> Vec<Option<std::sync::Arc<str>>> {
        self.actors.take_local_server_swings()
    }
    /// Starts the local arm swing lasting `ticks`, the duration its packet guard used.
    pub fn start_local_player_swing(&mut self, ticks: i32) {
        self.actors.start_swing(self.local_player_runtime_id, ticks);
    }
    /// Binds the local Java torso to the current simulation; `None` restores actor-clock motion.
    pub fn set_local_motion_authority(
        &mut self,
        authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    ) {
        self.actors
            .set_local_motion_authority(self.local_player_runtime_id, authority);
    }
    /// Applies completed local torso samples without advancing other actor motion or clocks.
    pub fn sync_local_swing_motion(
        &mut self,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        samples: impl IntoIterator<Item = actor_animation::LocalSwingMotionSample>,
    ) {
        self.actors
            .sync_local_swing_motion(self.local_player_runtime_id, authority, samples);
    }
    /// Uses committed local swing samples without re-admitting them on the remote actor clock.
    pub fn sync_local_swing(&mut self, progress: actor_animation::LocalSwingProgress) {
        self.actors
            .sync_local_swing(self.local_player_runtime_id, progress);
    }
    /// Drops the local player's Java equip progress to zero at its next tick.
    pub fn reset_local_java_equip(&mut self) {
        self.actors.reset_java_equip(self.local_player_runtime_id);
    }
    /// Borrows the current actor with this runtime ID.
    pub fn actor(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(runtime_id)
    }

    /// Whether the actor currently owns a live fishing hook entity.
    pub fn has_fishing_hook(&self, owner_unique_id: i64) -> bool {
        self.actors.has_fishing_hook(owner_unique_id)
    }

    /// Returns the session, dimension and spawn identity that owns the current actor.
    pub fn actor_lifetime(&self, runtime_id: u64) -> Option<actor_animation::ActorLifetimeId> {
        self.actors.lifetime(runtime_id)
    }

    /// Publishes the session's level mode before remote players are admitted.
    pub fn set_world_default_game_mode(&mut self, mode: protocol::GameModeUpdate) {
        self.actors.apply_world_game_mode(mode);
    }

    /// Uses the native class or the server-advertised constructor for custom actor targets.
    #[must_use]
    pub fn camera_aim_assist_eligible(&self, actor: &ActorSnapshot) -> Option<bool> {
        self.actors.camera_aim_assist_eligible(actor)
    }
    /// Unique id of the local player's actor.
    pub fn local_player_unique_id(&self) -> i64 {
        self.local_player_unique_id
    }
    /// Seat feet position and body yaw of the local player on its mount, when placed.
    pub fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)> {
        self.actors.rider_seat_pose(self.local_player_unique_id)
    }
    /// Borrows the current actor with this persistent unique ID.
    pub fn actor_by_unique_id(&self, unique_id: i64) -> Option<&ActorSnapshot> {
        self.actors.snapshot_by_unique(unique_id)
    }
    /// Borrows the player profile associated with this actor.
    pub fn actor_player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        self.actors.player_profile(runtime_id)
    }
    /// Borrows this actor's current roster XUID and UUID, even while its appearance is being prepared.
    pub fn actor_player_roster_identity(&self, runtime_id: u64) -> Option<(&str, &[u8; 16])> {
        self.actors.player_roster_identity(runtime_id)
    }
    /// Replaces `views` with dropped-item stacks at their interpolated pose, spin, and pickup
    /// flight at `partial_tick`.
    pub fn dropped_items_into(&self, partial_tick: f32, views: &mut Vec<crate::DroppedItemView>) {
        self.actors.dropped_items_into(partial_tick, views);
    }
    /// Live lightning-bolt actors, for the bolt renderer and sky flash.
    pub fn lightning_bolts(&self) -> Vec<crate::LightningBoltView> {
        self.actors.lightning_bolts()
    }
    /// Falling blocks and primed TNT with interpolated centres, swell and flash.
    pub fn block_entities(&self, partial_tick: f32) -> Vec<crate::BlockEntityView> {
        self.actors.block_entities(partial_tick)
    }
    /// Replaces `candidates` with pending block actors, retained for visibility decisions in
    /// the terrain's render frame.
    pub fn block_entity_candidates_into(
        &self,
        partial_tick: f32,
        candidates: &mut Vec<crate::BlockEntityCandidate>,
    ) {
        self.actors
            .block_entity_candidates_into(partial_tick, candidates);
    }
    /// End crystal beams with interpolated endpoints and actor animation age.
    pub fn crystal_beams(&self, partial_tick: f32) -> Vec<crate::CrystalBeamView> {
        self.actors.crystal_beams(partial_tick)
    }
    /// Dying dragon body centers and ray parameters at the supplied frame fraction.
    pub fn dragon_death_rays(&self, partial_tick: f32) -> Vec<crate::DragonDeathView> {
        self.actors.dragon_death_rays(partial_tick)
    }
    /// Replaces `ropes` with fishing lines and leads at their interpolated endpoints.
    pub fn ropes_into(&self, partial_tick: f32, ropes: &mut Vec<crate::RopeView>) {
        self.actors.ropes_into(partial_tick, ropes);
    }
    /// Borrows one actor rig for presentation.
    pub fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.actors.actor_rig(runtime_id)
    }
    /// Render-controller values at the frame fraction, retaining completed tick poses.
    pub fn actor_render_frame(&self, partial_tick: f32) -> actor_animation::ActorRenderFrame<'_> {
        self.actors.render_frame(partial_tick)
    }
    /// Full-body pose for HUD rendering, independent of the local first-person hand pose.
    pub fn actor_ui_pose(&self, runtime_id: u64) -> Option<&[actor_animation::BoneTransform]> {
        self.actors.actor_ui_pose(runtime_id)
    }
    /// Full-body controller layers paired with the actor's UI pose.
    pub fn actor_ui_layers(
        &self,
        runtime_id: u64,
    ) -> Option<actor_animation::ActorRenderLayers<'_>> {
        self.actors.actor_ui_layers(runtime_id)
    }
    /// The rig's pose at the frame fraction with `targets` replacing their joints in model space;
    /// other bones keep their animated offsets from their parents.
    pub fn actor_retargeted_pose(
        &self,
        runtime_id: u64,
        partial_tick: f32,
        targets: &[Option<actor_animation::BoneTransform>],
    ) -> Option<Vec<actor_animation::BoneTransform>> {
        self.actors
            .actor_retargeted_pose(runtime_id, partial_tick, targets)
    }
    /// [`Self::actor_retargeted_pose`] borrowed from `cache`, which keeps each rig's
    /// tick-constant transforms so frames between ticks only blend and compose.
    pub fn actor_retargeted_pose_cached<'c>(
        &self,
        runtime_id: u64,
        partial_tick: f32,
        targets: &[Option<actor_animation::BoneTransform>],
        cache: &'c mut actor_animation::JavaRetargetCache,
    ) -> Option<&'c [actor_animation::BoneTransform]> {
        self.actors
            .actor_retargeted_pose_cached(runtime_id, partial_tick, targets, cache)
    }
    /// The animated skin layers at the frame fraction, each retargeted by the model-space
    /// targets `targets` writes from its skeleton's bone names and rest pose.
    pub fn actor_retargeted_layers(
        &self,
        runtime_id: u64,
        partial_tick: f32,
        targets: impl FnMut(
            &[Box<str>],
            &[actor_animation::BoneTransform],
            &mut Vec<Option<actor_animation::BoneTransform>>,
        ) -> Option<()>,
        cache: &mut actor_animation::JavaRetargetCache,
    ) -> Option<Vec<actor_animation::SkinRenderLayer>> {
        self.actors
            .actor_retargeted_layers(runtime_id, partial_tick, targets, cache)
    }
    /// Iterates the retained actor rigs for presentation.
    pub fn actor_rigs(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.actors.actor_rigs()
    }
    /// Borrows controller states that completed authored actor animation evaluation.
    pub fn actor_particle_controllers(
        &self,
    ) -> impl Iterator<Item = actor_animation::ActorParticleController<'_>> {
        self.actors.actor_particle_controllers()
    }
    /// Returns counters from the authoritative actor animation runtime.
    pub const fn actor_animation_stats(&self) -> ActorAnimationStats {
        self.actors.animation_stats()
    }
    /// Borrows the equipment resolved for this actor.
    pub fn actor_equipment(&self, runtime_id: u64) -> Option<&ActorEquipmentSnapshot> {
        self.actors.equipment(runtime_id)
    }
    /// Borrows the equipment selected for the requested hand.
    pub fn actor_equipment_in_hand(
        &self,
        runtime_id: u64,
        hand: ActorHandedness,
    ) -> Option<&ActorEquipmentSnapshot> {
        self.actors.equipment_in_hand(runtime_id, hand)
    }
    /// Installs the compiler receiving each packet stop expression and its declared version.
    pub fn set_server_animation_compiler(
        &mut self,
        compiler: actor_animation::ServerAnimationCompiler,
    ) {
        self.actors.set_server_animation_compiler(compiler);
    }

    /// Layers the session's server-pack entity catalog over the vanilla one; its entities
    /// win by identifier for actors spawned afterwards.
    pub fn set_pack_entities(
        &mut self,
        assets: Option<(std::sync::Arc<assets::RuntimeEntityAssets>, Vec<u32>)>,
    ) {
        self.actors.set_pack_entities(assets);
    }

    /// Seeds `query.property` definitions from pack behavior defaults for entity types the
    /// server has not synced.
    pub fn seed_property_defaults(
        &mut self,
        types: &[(std::sync::Arc<str>, Vec<crate::PropertyDefault>)],
    ) {
        self.actors.seed_property_defaults(types);
    }

    /// Installs compiled item-use overlays before numerical admission.
    pub fn set_item_use_overlays(
        &mut self,
        overlays: std::sync::Arc<
            std::collections::BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>,
        >,
    ) {
        self.actors.set_item_use_overlays(overlays)
    }

    /// Installs resource-defined item use durations for actor animation.
    pub fn set_item_use_durations(
        &mut self,
        durations: std::sync::Arc<std::collections::BTreeMap<Box<str>, u32>>,
    ) {
        self.actors.set_item_use_durations(durations);
    }
    /// Returns numerical use admission without resolving names or component data.
    pub fn item_use_profile(&self, network_id: i32) -> crate::EffectiveItemUse {
        self.actors.item_use_profile(network_id)
    }
    /// Installs effective item-directed attack and kinetic presentation facts.
    pub fn set_item_attack_timings(
        &mut self,
        timings: std::sync::Arc<std::collections::BTreeMap<Box<str>, protocol::ItemAttackTiming>>,
    ) {
        self.actors.set_item_attack_timings(timings);
    }
    /// Borrows the same item facts used by native animation queries.
    pub fn item_attack_timing(&self, identifier: &str) -> Option<&protocol::ItemAttackTiming> {
        self.actors.item_attack_timing(identifier)
    }
    /// Borrows the armor resolved for this actor.
    pub fn actor_armor(&self, runtime_id: u64) -> Option<&ActorArmorSnapshot> {
        self.actors.armor(runtime_id)
    }
    /// Borrows this actor’s current remote action.
    pub fn actor_action(&self, runtime_id: u64) -> Option<&RemoteActionSnapshot> {
        self.actors.action(runtime_id)
    }
    /// Borrows the bounded remote action history for this actor.
    pub fn actor_action_history(&self, runtime_id: u64) -> &[RemoteActionSnapshot] {
        self.actors.action_history(runtime_id)
    }
    /// Returns counters from remote action processing.
    pub const fn actor_action_stats(&self) -> RemoteActionStats {
        self.actors.action_stats()
    }
    /// Counts item stacks still waiting for their registry identity.
    pub fn pending_item_resolution_count(&self) -> usize {
        self.actors.pending_item_resolution_count()
    }
    /// Retained hanging pictures in runtime-ID order, excluding the local player.
    pub fn painting_actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        self.actors
            .painting_actors()
            .filter(|actor| actor.runtime_id != self.local_player_runtime_id)
    }

    /// Shares solid bounds for prediction, excluding the local actor and its current mount.
    pub fn movement_actor_colliders(&self) -> std::sync::Arc<[crate::ActorCollisionBounds]> {
        self.actors
            .movement_collider_snapshot(self.local_player_runtime_id, self.local_player_unique_id)
    }

    /// Every tracked actor except the local player, in no particular order.
    pub fn remote_actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        let local = self.local_player_runtime_id;
        self.actors
            .actors()
            .filter(move |actor| actor.runtime_id != local)
    }
    /// The actor's interaction boxes where this frame's due ticks put it, for picks.
    pub fn pick_hit_boxes<'a>(&self, actor: &'a ActorSnapshot) -> crate::ActorHitBoxes<'a> {
        match self.actors.pick_pose(actor.runtime_id) {
            Some(pose) => actor.hit_boxes_at_pose(pose),
            None => actor.hit_boxes(),
        }
    }
    /// Counts retained actors in this session.
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }
}
