use super::*;

impl WorldAuthority {
    /// Changes only the actor visual after the corresponding terrain mesh is published.
    pub fn apply_actor_block_sync(&mut self, sync: protocol::ActorBlockSyncMessage) -> bool {
        self.actors.apply_terrain_sync(sync)
    }
    /// Borrows remote players and their retained profile records.
    pub fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        self.actors
            .render_players(Some(self.local_player_runtime_id))
    }
    /// Returns the retained display name for an actor identity.
    pub fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_display_name(unique_id)
    }
    /// Synced actor name tag, distinct from the player's scoreboard username.
    pub fn actor_name_tag(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_name_tag(unique_id)
    }
    /// Every username on the retained authoritative player list, sorted.
    pub fn player_list_usernames(&self) -> Vec<std::sync::Arc<str>> {
        self.actors.player_list_usernames()
    }
    /// How many players are on the player list, without allocating.
    pub fn player_count(&self) -> usize {
        self.actors.player_list_count()
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
    /// The item identifier registered for a network id.
    pub fn item_identifier(&self, network_id: i32) -> Option<std::sync::Arc<str>> {
        self.actors.item_identifier(network_id)
    }
    /// Installs the StartGame item registry so server-defined item ids resolve
    /// before any play-time registry arrives. False when it is refused.
    pub fn seed_item_registry(&mut self, registry: protocol::ItemRegistryEvent) -> bool {
        self.actors.seed_item_registry(registry)
    }
    /// Advances simulation ticks with one visual evaluation per tick.
    pub fn advance_actor_interpolation_ticks(&mut self, ticks: u32) {
        self.actors.advance_interpolation_ticks(ticks);
        self.publish_actor_particles();
        self.publish_actor_audio();
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
        for event in self.actors.take_synchronized_audio() {
            self.push_committed_audio(event);
        }
    }
    /// Drains decoded actor status events (hurt, death, taming, totem, ...) for particle and sound consumers.
    pub fn take_actor_status_notices(&mut self) -> Vec<crate::ActorStatusNotice> {
        self.actors.take_status_notices()
    }
    /// Drains where MobEquipment and MobArmorEquipment events landed, for diagnostics.
    pub fn take_equipment_notices(&mut self) -> Vec<crate::EquipmentNotice> {
        self.actors.take_equipment_notices()
    }
    /// Body boxes for the native liquid probes backing [`Self::set_actor_fluids`].
    #[must_use]
    pub fn actor_fluid_probes(&self) -> Vec<crate::ActorFluidProbe> {
        self.actors.fluid_probes()
    }
    /// Installs the per-mount seat layouts riders fall back to when the server streams no offset.
    pub fn set_actor_seat_defaults(&mut self, defaults: std::sync::Arc<crate::SeatDefaults>) {
        self.actors.set_seat_defaults(defaults);
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
    /// Sets the view `[pitch, yaw]` (degrees) that camera-facing billboard rigs sample per tick.
    pub fn set_actor_camera_rotation(&mut self, rotation: [f32; 2]) {
        self.actors.set_camera_rotation(rotation);
    }
    /// Sets the view outside which rigs hold their pose at each tick; `None` animates all.
    pub fn set_actor_animation_view(&mut self, view: Option<crate::ActorAnimationView>) {
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
    /// Starts the local player's arm swing, which the server never echoes back to its owner.
    /// Starts the local arm swing lasting `ticks`, the duration its packet guard used.
    pub fn start_local_player_swing(&mut self, ticks: i32) {
        self.actors.start_swing(self.local_player_runtime_id, ticks);
    }
    /// Binds the local Java torso to the current simulation; `None` restores actor-clock motion.
    pub fn set_local_motion_authority(&mut self, authority: Option<(u64, u64)>) {
        self.actors
            .set_local_motion_authority(self.local_player_runtime_id, authority);
    }
    /// Applies completed local torso samples without advancing other actor motion or clocks.
    pub fn sync_local_swing_motion(
        &mut self,
        authority: (u64, u64),
        samples: impl IntoIterator<Item = crate::LocalSwingMotionSample>,
    ) {
        self.actors
            .sync_local_swing_motion(self.local_player_runtime_id, authority, samples);
    }
    /// Uses committed local swing samples without re-admitting them on the remote actor clock.
    pub fn sync_local_swing(&mut self, progress: crate::LocalSwingProgress) {
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
    /// Dropped-item stacks with interpolated pose, spin, and pickup flight at `partial_tick`.
    pub fn dropped_items(&self, partial_tick: f32) -> Vec<crate::DroppedItemView> {
        self.actors.dropped_items(partial_tick)
    }
    /// Live lightning-bolt actors, for the bolt renderer and sky flash.
    pub fn lightning_bolts(&self) -> Vec<crate::LightningBoltView> {
        self.actors.lightning_bolts()
    }
    /// Falling blocks and primed TNT with interpolated centres, swell and flash.
    pub fn block_entities(&self, partial_tick: f32) -> Vec<crate::BlockEntityView> {
        self.actors.block_entities(partial_tick)
    }
    /// Retains pending block actors for visibility decisions in the terrain's render frame.
    pub fn block_entity_candidates(&self, partial_tick: f32) -> Vec<crate::BlockEntityCandidate> {
        self.actors.block_entity_candidates(partial_tick)
    }
    /// End crystal beams with interpolated endpoints and actor animation age.
    pub fn crystal_beams(&self, partial_tick: f32) -> Vec<crate::CrystalBeamView> {
        self.actors.crystal_beams(partial_tick)
    }
    /// Dying dragon body centers and ray parameters at the supplied frame fraction.
    pub fn dragon_death_rays(&self, partial_tick: f32) -> Vec<crate::DragonDeathView> {
        self.actors.dragon_death_rays(partial_tick)
    }
    /// Fishing lines and leads with interpolated endpoints.
    pub fn ropes(&self, partial_tick: f32) -> Vec<crate::RopeView> {
        self.actors.ropes(partial_tick)
    }
    /// Borrows one actor rig for presentation.
    pub fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.actors.actor_rig(runtime_id)
    }
    /// Render-controller values at the frame fraction, retaining completed tick poses.
    pub fn actor_render_frame(&self, partial_tick: f32) -> crate::ActorRenderFrame<'_> {
        self.actors.render_frame(partial_tick)
    }
    /// Full-body pose for HUD rendering, independent of the local first-person hand pose.
    pub fn actor_ui_pose(&self, runtime_id: u64) -> Option<&[crate::BoneTransform]> {
        self.actors.actor_ui_pose(runtime_id)
    }
    /// The rig's pose at the frame fraction with `targets` replacing their joints in model space;
    /// other bones keep their animated offsets from their parents.
    pub fn actor_retargeted_pose(
        &self,
        runtime_id: u64,
        partial_tick: f32,
        targets: &[Option<crate::BoneTransform>],
    ) -> Option<Vec<crate::BoneTransform>> {
        self.actors
            .actor_retargeted_pose(runtime_id, partial_tick, targets)
    }
    /// The animated skin layers at the frame fraction, each retargeted by the model-space
    /// targets `targets` builds from its skeleton's bone names and rest pose.
    pub fn actor_retargeted_layers(
        &self,
        runtime_id: u64,
        partial_tick: f32,
        targets: impl Fn(
            &[Box<str>],
            &[crate::BoneTransform],
        ) -> Option<Vec<Option<crate::BoneTransform>>>,
    ) -> Option<Vec<crate::SkinRenderLayer>> {
        self.actors
            .actor_retargeted_layers(runtime_id, partial_tick, targets)
    }
    /// Iterates the retained actor rigs for presentation.
    pub fn actor_rigs(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.actors.actor_rigs()
    }
    /// Borrows controller states that completed authored actor animation evaluation.
    pub fn actor_particle_controllers(
        &self,
    ) -> impl Iterator<Item = crate::ActorParticleController<'_>> {
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
    /// Item use durations (ticks by identifier) that drive `query.main_hand_item_max_duration`.
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

    /// Installs resource-defined item use durations for actor animation.
    pub fn set_item_use_durations(
        &mut self,
        durations: std::sync::Arc<std::collections::BTreeMap<Box<str>, u32>>,
    ) {
        self.actors.set_item_use_durations(durations);
    }
    /// Ticks the pack lets `identifier` be used for before its use completes.
    pub fn item_max_use_ticks(&self, identifier: &str) -> Option<u32> {
        self.actors.item_max_use_ticks(identifier)
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
    /// Every tracked actor except the local player, in no particular order.
    pub fn remote_actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        let local = self.local_player_runtime_id;
        self.actors
            .actors()
            .filter(move |actor| actor.runtime_id != local)
    }
    /// Counts retained actors in this session.
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }
}
