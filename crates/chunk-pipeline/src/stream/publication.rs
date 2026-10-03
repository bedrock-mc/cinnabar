use super::*;

impl WorldStream {
    pub fn set_publication_allowance(&mut self, allowance: PublicationAllowance) {
        self.publication_allowance = Some(allowance);
    }

    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        let changes = self.mesh_changes.drain(..).collect::<Vec<_>>();
        self.stats.phase2_stages.mesh_changes_dequeued = self
            .stats
            .phase2_stages
            .mesh_changes_dequeued
            .saturating_add(changes.len() as u64);
        changes
    }
    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        let change = self.mesh_changes.pop_front();
        if change.is_some() {
            self.stats.phase2_stages.mesh_changes_dequeued = self
                .stats
                .phase2_stages
                .mesh_changes_dequeued
                .saturating_add(1);
        }
        change
    }
    pub fn pending_mesh_change_count(&self) -> usize {
        self.mesh_changes.len()
    }
    pub fn unacknowledged_mesh_count(&self) -> usize {
        self.revisions.entries.len()
    }
    pub fn is_mesh_clean(&self, key: SubChunkKey) -> bool {
        self.resident.contains(&key) && self.revisions.dirty(key).is_none()
    }
    // A rejected change stays intact so the caller can retry without cloning
    // packed streams or adding an allocation to this hot ownership path.
    #[allow(clippy::result_large_err)]
    pub fn retry_mesh_change_front(
        &mut self,
        change: WorldMeshChange,
    ) -> Result<(), WorldMeshChange> {
        if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
            return Err(change);
        }
        self.mesh_changes.push_front(change);
        self.stats.phase2_stages.mesh_changes_queued = self
            .stats
            .phase2_stages
            .mesh_changes_queued
            .saturating_add(1);
        Ok(())
    }
    pub fn acknowledge_mesh_upload(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
    ) {
        let Some(dirty) = self.revisions.dirty(key) else {
            return;
        };
        if dirty.revision != generation || dirty.since != dirty_since {
            return;
        }
        self.stats.max_remesh_latency = self
            .stats
            .max_remesh_latency
            .max(applied_at.saturating_duration_since(dirty_since));
        self.stats.last_mesh_ack_at = Some(
            self.stats
                .last_mesh_ack_at
                .map_or(applied_at, |latest| latest.max(applied_at)),
        );
        // An evicted key's removal ack must not re-enter the map its eviction just pruned.
        if self.resident.contains(&key) || self.known_air.contains(&key) {
            self.applied_mesh_generations.insert(key, generation);
        } else {
            self.applied_mesh_generations.remove(&key);
        }
        self.revisions.clear_if_current(key, generation);
        self.stats.phase2_stages.mesh_uploads_acknowledged = self
            .stats
            .phase2_stages
            .mesh_uploads_acknowledged
            .saturating_add(1);
    }
    /// Returns undelivered controls to the front without changing their order.
    pub fn restore_committed_controls(
        &mut self,
        controls: impl DoubleEndedIterator<Item = CommittedControlEvent>,
    ) {
        self.authority.restore_committed_controls(controls)
    }

    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.authority.take_committed_controls()
    }
    pub fn take_committed_ui(&mut self) -> Vec<CommittedUiEvent> {
        self.authority.take_committed_ui()
    }
    pub fn take_committed_audio(&mut self) -> Vec<CommittedAudioEvent> {
        self.authority.take_committed_audio()
    }
    pub fn take_committed_particles(&mut self) -> Vec<CommittedParticleEvent> {
        self.authority.take_committed_particles()
    }
    pub fn take_committed_camera(&mut self) -> Vec<CommittedCameraEvent> {
        self.authority.take_committed_camera()
    }
    pub fn take_fatal_error(&mut self) -> Option<WorldStreamFatalError> {
        self.fatal_error.take()
    }
    pub fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        self.authority.render_players()
    }
    pub fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.authority.actor_display_name(unique_id)
    }
    /// Synced actor name tag, distinct from the player's scoreboard username.
    pub fn actor_name_tag(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.authority.actor_name_tag(unique_id)
    }
    /// Every username on the retained authoritative player list, sorted.
    pub fn player_list_usernames(&self) -> Vec<std::sync::Arc<str>> {
        self.authority.player_list_usernames()
    }
    /// The authoritative `(current, maximum)` health of the actor with this
    /// unique id, if it is known and well-formed.
    pub fn actor_health_by_unique(&self, unique_id: i64) -> Option<(f32, f32)> {
        self.authority.actor_health_by_unique(unique_id)
    }
    /// Position and view angles `(position, yaw, pitch)` of the actor with this unique id.
    pub fn actor_pose_by_unique(&self, unique_id: i64) -> Option<([f32; 3], f32, f32)> {
        self.authority.actor_pose_by_unique(unique_id)
    }
    /// Whether this actor carries a named attribute (capability gate).
    pub fn actor_has_attribute_by_unique(&self, unique_id: i64, name: &str) -> bool {
        self.authority
            .actor_has_attribute_by_unique(unique_id, name)
    }
    /// Resolves one wire item stack against the retained item registry and
    /// compiled item visual routes.
    pub fn canonical_item_stack(
        &self,
        stack: &client_world::ingestion::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack> {
        self.authority.canonical_item_stack(stack)
    }
    /// The item identifier registered for a network id.
    pub fn item_identifier(&self, network_id: i32) -> Option<std::sync::Arc<str>> {
        self.authority.item_identifier(network_id)
    }
    /// Installs the StartGame item registry so server-defined item ids resolve
    /// before any play-time registry arrives. False when it is refused.
    pub fn seed_item_registry(
        &mut self,
        registry: client_world::ingestion::ItemRegistryEvent,
    ) -> bool {
        self.authority.seed_item_registry(registry)
    }
    /// Advances simulation ticks with one visual evaluation per tick.
    pub fn advance_actor_interpolation_ticks(&mut self, ticks: u32) {
        self.authority.advance_actor_interpolation_ticks(ticks)
    }
    /// Advances elapsed tick state, evaluating animation once for this rendered frame.
    /// Bedrock 1.26.50.26: AnimationComponent RVAs 0x1e019a0 and 0x1e13940.
    pub fn advance_actor_interpolation_frame(&mut self, ticks: u32) {
        self.authority.advance_actor_interpolation_frame(ticks)
    }
    /// Drains decoded actor status events (hurt, death, taming, totem, ...) for particle and sound consumers.
    pub fn take_actor_status_notices(&mut self) -> Vec<crate::ActorStatusNotice> {
        self.authority.take_actor_status_notices()
    }
    /// Drains where MobEquipment and MobArmorEquipment events landed, for diagnostics.
    pub fn take_equipment_notices(&mut self) -> Vec<crate::EquipmentNotice> {
        self.authority.take_equipment_notices()
    }
    /// Feet position of every tracked actor, for [`Self::set_actor_fluids`] sampling.
    #[must_use]
    pub fn actor_fluid_sample_points(&self) -> Vec<(u64, [f32; 3])> {
        self.authority.actor_fluid_sample_points()
    }
    /// Installs the per-mount seat layouts riders fall back to when the server streams no offset.
    pub fn set_actor_seat_defaults(&mut self, defaults: std::sync::Arc<crate::SeatDefaults>) {
        self.authority.set_actor_seat_defaults(defaults)
    }
    /// Bed block under every sleeping actor, for [`Self::set_actor_bed_rotations`] sampling.
    #[must_use]
    pub fn actor_bed_sample_points(&self) -> Vec<(u64, [i32; 3])> {
        self.authority.actor_bed_sample_points()
    }
    /// Records the `(runtime_id, degrees)` bed orientation that backs `query.sleep_rotation`.
    pub fn set_actor_bed_rotations(&mut self, samples: &[(u64, f32)]) {
        self.authority.set_actor_bed_rotations(samples)
    }
    /// Records `(runtime_id, in_water, in_lava)` samples that back the fluid animation queries.
    pub fn set_actor_fluids(&mut self, samples: &[(u64, bool, bool)]) {
        self.authority.set_actor_fluids(samples)
    }
    /// Sets the view `[pitch, yaw]` (degrees) that camera-facing billboard rigs sample per tick.
    pub fn set_actor_camera_rotation(&mut self, rotation: [f32; 2]) {
        self.authority.set_actor_camera_rotation(rotation)
    }
    /// Sets the view outside which rigs hold their pose at each tick; `None` animates all.
    pub fn set_actor_animation_view(&mut self, view: Option<crate::ActorAnimationView>) {
        self.authority.set_actor_animation_view(view)
    }
    /// Sets the view's world position that camera-relative queries sample per tick.
    pub fn set_actor_camera_position(&mut self, position: [f32; 3]) {
        self.authority.set_actor_camera_position(position)
    }
    /// Feeds this frame's client-authored local-player pose into the shared actor rig. Call
    /// before [`Self::advance_actor_interpolation_ticks`] and [`Self::actor_rigs`] so the
    /// third-person body and first-person hand read a driven rig instead of a static fallback.
    pub fn sync_local_player_pose(&mut self, feed: &LocalPlayerFeed) {
        self.authority.sync_local_player_pose(feed)
    }
    /// Starts the local player's arm swing, which the server never echoes back to its owner.
    /// Starts the local arm swing lasting `ticks`, the duration its packet guard used.
    pub fn start_local_player_swing(&mut self, ticks: i32) {
        self.authority.start_local_player_swing(ticks)
    }
    pub fn actor(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.authority.actor(runtime_id)
    }
    /// Unique id of the local player's actor.
    pub fn local_player_unique_id(&self) -> i64 {
        self.authority.local_player_unique_id()
    }
    /// Seat feet position and body yaw of the local player on its mount, when placed.
    pub fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)> {
        self.authority.local_rider_seat_pose()
    }
    pub fn actor_by_unique_id(&self, unique_id: i64) -> Option<&ActorSnapshot> {
        self.authority.actor_by_unique_id(unique_id)
    }
    pub fn actor_player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        self.authority.actor_player_profile(runtime_id)
    }
    /// Dropped-item stacks with interpolated pose, spin, and pickup flight at `partial_tick`.
    pub fn dropped_items(&self, partial_tick: f32) -> Vec<crate::DroppedItemView> {
        self.authority.dropped_items(partial_tick)
    }
    /// Live lightning-bolt actors, for the bolt renderer and sky flash.
    pub fn lightning_bolts(&self) -> Vec<crate::LightningBoltView> {
        self.authority.lightning_bolts()
    }
    /// Falling blocks and primed TNT with interpolated centres, swell and flash.
    pub fn block_entities(&self, partial_tick: f32) -> Vec<crate::BlockEntityView> {
        self.authority.block_entities(partial_tick)
    }
    /// Fishing lines and leads with interpolated endpoints.
    pub fn ropes(&self, partial_tick: f32) -> Vec<crate::RopeView> {
        self.authority.ropes(partial_tick)
    }
    pub fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.authority.actor_rig(runtime_id)
    }
    /// Full-body pose for HUD rendering, independent of the local first-person hand pose.
    pub fn actor_ui_pose(&self, runtime_id: u64) -> Option<&[crate::BoneTransform]> {
        self.authority.actor_ui_pose(runtime_id)
    }
    pub fn actor_rigs(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.authority.actor_rigs()
    }
    pub const fn actor_animation_stats(&self) -> ActorAnimationStats {
        self.authority.actor_animation_stats()
    }
    pub fn actor_equipment(&self, runtime_id: u64) -> Option<&ActorEquipmentSnapshot> {
        self.authority.actor_equipment(runtime_id)
    }
    pub fn actor_equipment_in_hand(
        &self,
        runtime_id: u64,
        hand: ActorHandedness,
    ) -> Option<&ActorEquipmentSnapshot> {
        self.authority.actor_equipment_in_hand(runtime_id, hand)
    }
    /// Item use durations (ticks by identifier) that drive `query.main_hand_item_max_duration`.
    /// Layers the session's server-pack entity catalog over the vanilla one; its entities
    /// win by identifier for actors spawned afterwards.
    pub fn set_pack_entities(
        &mut self,
        assets: Option<(std::sync::Arc<assets::RuntimeEntityAssets>, Vec<u32>)>,
    ) {
        self.authority.set_pack_entities(assets)
    }

    /// Seeds `query.property` definitions from pack behavior defaults for entity types the
    /// server has not synced.
    pub fn seed_property_defaults(
        &mut self,
        types: &[(std::sync::Arc<str>, Vec<crate::PropertyDefault>)],
    ) {
        self.authority.seed_property_defaults(types)
    }

    pub fn set_item_use_durations(
        &mut self,
        durations: std::sync::Arc<std::collections::BTreeMap<Box<str>, u32>>,
    ) {
        self.authority.set_item_use_durations(durations)
    }
    /// Ticks the pack lets `identifier` be used for before its use completes.
    pub fn item_max_use_ticks(&self, identifier: &str) -> Option<u32> {
        self.authority.item_max_use_ticks(identifier)
    }
    pub fn actor_armor(&self, runtime_id: u64) -> Option<&ActorArmorSnapshot> {
        self.authority.actor_armor(runtime_id)
    }
    pub fn actor_action(&self, runtime_id: u64) -> Option<&RemoteActionSnapshot> {
        self.authority.actor_action(runtime_id)
    }
    pub fn actor_action_history(&self, runtime_id: u64) -> &[RemoteActionSnapshot] {
        self.authority.actor_action_history(runtime_id)
    }
    pub const fn actor_action_stats(&self) -> RemoteActionStats {
        self.authority.actor_action_stats()
    }
    pub fn pending_item_resolution_count(&self) -> usize {
        self.authority.pending_item_resolution_count()
    }
    /// Every tracked actor except the local player, in no particular order.
    pub fn remote_actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        self.authority.remote_actors()
    }
    pub fn actor_count(&self) -> usize {
        self.authority.actor_count()
    }
    pub fn stats(&self) -> WorldStreamStats {
        let completed_decode_results = self
            .order
            .heavy_count()
            .saturating_sub(self.pending_decode.len())
            .saturating_sub(self.in_flight_decode_jobs);
        let [
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
        ] = self.block_entity_visuals.counts();
        WorldStreamStats {
            audio_nondefault_camera_observed: self.authority.audio_nondefault_camera_observed(),
            received_radius_chunks: self.chunk_radius,
            publisher_radius_chunks: self.publisher_radius_chunks,
            resident_sub_chunks: self.resident.len(),
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
            pending_mesh_jobs: self.pending_mesh.len(),
            in_flight_mesh_jobs: self.in_flight.len(),
            pending_light_jobs: self.pending_light.len(),
            in_flight_light_jobs: self.in_flight_light.len(),
            terminal_light_failures: self.light_failures.len(),
            admitted_world_events: self.order.admitted_count(),
            admitted_heavy_events: self.order.heavy_count(),
            committed_audio_events: self.authority.committed_audio_count(),
            committed_camera_events: self.authority.committed_camera_count(),
            queued_decode_jobs: self.pending_decode.len(),
            in_flight_decode_jobs: self.in_flight_decode_jobs,
            completed_decode_results,
            pending_retry_requests: self.queued_retry_request_count(),
            awaiting_sub_chunk_responses: self.sub_chunk_deadlines.len(),
            ..self.stats
        }
    }
    pub fn begin_timed_session(&mut self) {
        self.stats.max_decode_queue_wait = Duration::ZERO;
        self.stats.max_light_queue_wait = Duration::ZERO;
        self.stats.max_mesh_queue_wait = Duration::ZERO;
        self.stats.max_decode_duration = Duration::ZERO;
        self.stats.max_mesh_duration = Duration::ZERO;
        self.stats.max_light_duration = Duration::ZERO;
        self.stats.max_remesh_latency = Duration::ZERO;
        self.stats.last_chunk_commit_at = None;
        self.stats.last_mesh_dispatch_at = None;
        self.stats.last_mesh_completion_at = None;
        self.stats.last_mesh_ack_at = None;
    }
}

impl WorldStream {
    #[must_use]
    pub const fn actor_session_id(&self) -> u64 {
        self.authority.actor_session_id()
    }
}
