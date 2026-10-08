use super::*;
use client_world::ingestion::PLAYER_NETWORK_OFFSET;

impl WorldStream {
    /// Prioritizes the complete spawn columns and their light halo while entry is pending.
    pub fn set_startup_priority(&mut self, enabled: bool) {
        self.startup_priority = enabled;
    }

    /// Ends spawn priority once local terrain is ready; servers that stream only after
    /// initialization keep it until terrain arrives. Returns whether priority is off.
    pub fn finish_startup_priority(&mut self) -> bool {
        if self.startup_priority && !self.local_terrain_ready() {
            return false;
        }
        self.startup_priority = false;
        true
    }

    pub(super) fn scheduler_view(&self, position: [f32; 3]) -> SchedulerView {
        let player = self.authority.resolved_server_position().position;
        SchedulerView {
            position,
            forward: self.view_forward,
            startup_center: self.startup_priority.then(|| {
                ChunkKey::new(
                    self.authority.current_dimension(),
                    floor_to_i32(player[0]).div_euclid(16),
                    floor_to_i32(player[2]).div_euclid(16),
                )
            }),
        }
    }

    pub(super) fn is_startup_dependency(&self, key: SubChunkKey) -> bool {
        self.startup_priority && self.scheduler_view([0.0; 3]).startup_class(key) < 2
    }

    /// Mutation-through frontier for inactive inventory projections after polling. It passes
    /// pending chunk decodes but not block mutations, which apply only once decoded.
    #[must_use]
    pub fn inventory_committed_through(&self) -> Option<u64> {
        if self.lighting.fatal_failure {
            return None;
        }
        Some(self.order.committed_past_chunk_data())
    }

    const INITIAL_MESH_DISPATCH_BUDGET_PER_POLL: usize = 32;

    /// Breaks the publication-token deadlock: when the allowance is exhausted
    /// and nothing is in flight, a small floor keeps meshing alive so
    /// completions can resume once permits retire instead of starving a live
    /// join forever.
    const STARVED_MESH_DISPATCH_FLOOR_PER_POLL: usize = 4;

    /// Sets the view direction light and mesh work favour; a zero or non-finite one clears it.
    pub fn set_view_forward(&mut self, forward: [f32; 3]) {
        let length = forward
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        self.view_forward = (length.is_finite() && length > f32::EPSILON)
            .then(|| forward.map(|value| value / length));
    }

    pub fn poll(&mut self, camera_position: [f32; 3], max_mesh_jobs: usize) -> WorldStreamPoll {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.poll").entered();
        if camera_position.iter().all(|value| value.is_finite()) {
            self.last_camera_position = camera_position;
            self.requests.last_player_chunk = Some(ChunkKey::new(
                self.authority.current_dimension(),
                floor_to_i32(camera_position[0]).div_euclid(16),
                floor_to_i32(camera_position[2]).div_euclid(16),
            ));
        }
        let now = Instant::now();
        let frame_deadline = self
            .frame_deadline
            .take()
            .unwrap_or_else(|| self.poll_deadline.unwrap_or(now + self.poll_budget));
        let remaining = frame_deadline.saturating_duration_since(now);
        self.poll_deadline
            .get_or_insert(now + remaining - remaining / commit_budget::WORLD_SCHEDULING_SHARE);
        self.polling = true;
        self.poll_heavy_guarantee = true;
        let mut report = WorldStreamPoll::default();
        while report.decoded_results == 0 || !self.poll_budget_exhausted() {
            let Ok(completion) = self.decode_rx.try_recv() else {
                break;
            };
            report.decoded_results += 1;
            self.accept_decode_completion(completion);
        }
        self.apply_ready();
        self.promote_deferred_ingress();
        self.expire_sub_chunk_deadlines(Instant::now());
        self.pump_deferred_retries();
        self.dispatch_decode_jobs();

        let now = Instant::now();
        let remaining = frame_deadline.saturating_duration_since(now);
        self.poll_deadline = Some(now + remaining - remaining / commit_budget::WORLD_MESH_SHARE);
        while report.light_results == 0 || !self.poll_budget_exhausted() {
            let Ok(completion) = self.lighting.rx.try_recv() else {
                break;
            };
            report.light_results += 1;
            self.accept_light_completion(completion);
        }
        report.light_jobs_dispatched =
            self.dispatch_light_jobs(camera_position, LIGHT_DISPATCH_BUDGET_PER_POLL);

        self.poll_deadline = Some(frame_deadline);
        self.retry_staged_mesh_completions();
        while self.mesh_changes.len() < MAX_PENDING_MESH_CHANGES
            && (report.mesh_results == 0 || !self.poll_budget_exhausted())
        {
            let Ok(completion) = self.mesh_rx.try_recv() else {
                break;
            };
            report.mesh_results += 1;
            self.accept_mesh_completion(completion);
        }
        let live_publication_items = self
            .publication_allowance
            .as_ref()
            .map_or(max_mesh_jobs, PublicationAllowance::frame_remaining_items);
        // Do not flood the render queue while the first view is still being
        // lit. A large Bedrock view can contain thousands of resident
        // sub-chunks; admitting all of their meshes at once makes the GPU
        // preparation/present path stutter even though only the nearest
        // handful can be visible. Once the initial backlog drains, restore
        // the normal publication budget.
        let mesh_budget = if self.lighting.jobs.pending.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
            || self.lighting.jobs.in_flight.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
        {
            max_mesh_jobs.min(Self::INITIAL_MESH_DISPATCH_BUDGET_PER_POLL)
        } else {
            max_mesh_jobs
        };
        let mut dispatch_budget = mesh_budget
            .min(live_publication_items)
            .min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len()));
        if dispatch_budget == 0
            && mesh_budget != 0
            && self.mesh_jobs.in_flight.is_empty()
            && self.staged_mesh_completions.is_empty()
            && !self.mesh_jobs.pending.is_empty()
        {
            dispatch_budget = Self::STARVED_MESH_DISPATCH_FLOOR_PER_POLL.min(mesh_budget);
        }
        let removal_budget = max_mesh_jobs
            .min(live_publication_items)
            .min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len()))
            .max(dispatch_budget);
        report.mesh_jobs_dispatched =
            self.dispatch_mesh_jobs_with_limits(camera_position, dispatch_budget, removal_budget);
        self.poll_deadline = None;
        self.polling = false;
        report
    }
    /// Dispatches the light and mesh work a live block change made urgent, without
    /// waiting for the next poll.
    pub(super) fn dispatch_urgent_work(&mut self) {
        if !std::mem::take(&mut self.urgent_work_due) || self.lighting.fatal_failure {
            return;
        }
        self.with_urgent_deadline(|stream| {
            let camera = stream.last_camera_position;
            stream.dispatch_light_jobs(camera, URGENT_DISPATCH_BUDGET);
            let budget = stream
                .publication_allowance
                .as_ref()
                .map_or(
                    URGENT_DISPATCH_BUDGET,
                    PublicationAllowance::frame_remaining_items,
                )
                .min(URGENT_DISPATCH_BUDGET)
                .min(MAX_PENDING_MESH_CHANGES.saturating_sub(stream.mesh_changes.len()));
            stream.dispatch_mesh_jobs_with_limits(camera, budget, budget);
        });
    }

    /// Runs urgent work under its own cooperative deadline unless a poll's already applies.
    fn with_urgent_deadline<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        if self.poll_deadline.is_some() {
            return work(self);
        }
        self.poll_deadline = Some(Instant::now() + URGENT_PASS_BUDGET);
        let result = work(self);
        self.poll_deadline = None;
        result
    }

    /// Accepts urgent light and mesh results finished since the poll and dispatches the meshes
    /// they unblock, so a block change publishes in the frame its workers finish. Returns the
    /// mesh results accepted.
    pub fn service_urgent_work(&mut self) -> usize {
        if !self.urgent_work_due
            && self.urgent_mesh_in_flight.is_empty()
            && !self
                .lighting
                .jobs
                .in_flight
                .values()
                .any(|identity| identity.urgent)
        {
            return 0;
        }
        self.with_urgent_deadline(|stream| {
            for _ in 0..URGENT_RESULTS_PER_PASS {
                let Ok(completion) = stream.lighting.rx.try_recv() else {
                    break;
                };
                stream.accept_light_completion(completion);
                stream.urgent_work_due = true;
            }
            stream.dispatch_urgent_work();
            stream.retry_urgent_staged_mesh_completions(URGENT_RESULTS_PER_PASS);
            let mut accepted = 0;
            while accepted < URGENT_RESULTS_PER_PASS
                && stream.mesh_changes.len() < MAX_PENDING_MESH_CHANGES
            {
                let Ok(completion) = stream.mesh_rx.try_recv() else {
                    break;
                };
                stream.accept_mesh_completion(completion);
                accepted += 1;
            }
            accepted
        })
    }

    pub fn camera_medium(&self, position: [f32; 3]) -> CameraMedium {
        if !position.iter().all(|value| value.is_finite()) {
            return CameraMedium::Air;
        }
        let block = position.map(floor_to_i32);
        let key = SubChunkKey::new(
            self.authority.current_dimension(),
            block[0].div_euclid(16),
            block[1].div_euclid(16),
            block[2].div_euclid(16),
        );
        let Some(center) = self.authority.terrain().sub_chunk(key) else {
            return CameraMedium::Air;
        };
        let mut adjacent: [Option<Arc<SubChunk>>; 27] = std::array::from_fn(|_| None);
        for offset @ [dx, dy, dz] in MeshNeighbourhood::liquid_sample_offsets() {
            if offset == [0, 0, 0] {
                continue;
            }
            let Some(neighbour_key) = key
                .x
                .checked_add(i32::from(dx))
                .zip(key.y.checked_add(i32::from(dy)))
                .zip(key.z.checked_add(i32::from(dz)))
                .map(|((x, y), z)| SubChunkKey::new(key.dimension, x, y, z))
            else {
                continue;
            };
            if let Some(sub_chunk) = self.authority.terrain().sub_chunk(neighbour_key) {
                adjacent[mesh_offset_index(offset)] = Some(sub_chunk);
            }
        }
        let mut neighbourhood = MeshNeighbourhood::new(&center);
        for offset in MeshNeighbourhood::liquid_sample_offsets() {
            if let Some(sub_chunk) = adjacent[mesh_offset_index(offset)].as_deref() {
                let inserted = neighbourhood.insert(offset, sub_chunk);
                debug_assert!(inserted);
            }
        }
        let local_position = std::array::from_fn(|axis| {
            block[axis].rem_euclid(16) as f32 + position[axis].rem_euclid(1.0)
        });
        sample_camera_medium(
            self.classifier,
            self.authority.runtime_assets(),
            self.authority.network_id_mode(),
            &neighbourhood,
            local_position,
        )
    }
    /// Retained block and sky light (0..=15) at `position`'s block cell in the current
    /// dimension, for lighting the first-person hand to match the player's standing block. A
    /// non-finite position or a sub-chunk whose light is not resident reads dark `(0, 0)`.
    #[must_use]
    pub fn light_level_at(&self, position: [f32; 3]) -> (u8, u8) {
        self.solved_light_at(position).unwrap_or((0, 0))
    }
    /// `(block, sky)` light at `position`; `None` where no light has been solved yet.
    #[must_use]
    pub fn solved_light_at(&self, position: [f32; 3]) -> Option<(u8, u8)> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let block = position.map(floor_to_i32);
        let key = SubChunkKey::new(
            self.authority.current_dimension(),
            block[0].div_euclid(16),
            block[1].div_euclid(16),
            block[2].div_euclid(16),
        );
        let light = self.lighting.store.light(key)?;
        let local = |axis: usize| block[axis].rem_euclid(16) as u8;
        let (x, y, z) = (local(0), local(1), local(2));
        Some((
            light.get(LightChannel::Block, x, y, z).unwrap_or(0),
            light.get(LightChannel::Sky, x, y, z).unwrap_or(0),
        ))
    }
    #[must_use]
    pub fn camera_biome_id(&self, position: [f32; 3]) -> Option<u32> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let block = position.map(floor_to_i32);
        let key = SubChunkKey::new(
            self.authority.current_dimension(),
            block[0].div_euclid(16),
            block[1].div_euclid(16),
            block[2].div_euclid(16),
        );
        self.authority.terrain().biome_id(
            key,
            block[0].rem_euclid(16) as u8,
            block[1].rem_euclid(16) as u8,
            block[2].rem_euclid(16) as u8,
        )
    }
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "the committed stream radius is bounded to sixteen chunks"
    )]
    pub fn render_distance_blocks(&self) -> f32 {
        self.chunk_radius
            .unwrap_or_else(|| self.active_radius_chunks())
            .clamp(0, PHASE0_MAX_VIEW_RADIUS_CHUNKS)
            .saturating_mul(16) as f32
    }
    pub fn biome_definitions_snapshot(&self) -> Arc<[BiomeDefinitionEvent]> {
        Arc::clone(self.authority.biome_definitions())
    }
    pub fn resolved_biome_tints_snapshot(&self) -> Arc<ResolvedBiomeTints> {
        Arc::clone(self.authority.resolved_biome_tints())
    }
    pub fn connectivity(&self, key: SubChunkKey) -> Option<FaceConnectivity> {
        self.connectivity.get(&key)
    }
    pub fn surface_eye_position(&self, block_x: i32, block_z: i32) -> Option<[f32; 3]> {
        let block_y = self.top_non_air_block_y(block_x, block_z)?;
        // Rest the anchor exactly on the surface: movement feet
        // are recovered as network Y minus PLAYER_NETWORK_OFFSET,
        // so the former eye-height guess (`+ 2.62`) left feet
        // 1e-5 blocks inside the surface block and every surface
        // spawn started embedded in terrain.
        Some([
            block_x as f32 + 0.5,
            block_y as f32 + 1.0 + PLAYER_NETWORK_OFFSET,
            block_z as f32 + 0.5,
        ])
    }
    /// Y of the highest non-air block in a loaded column, or `None` when it is unloaded or empty.
    #[must_use]
    pub fn top_non_air_block_y(&self, block_x: i32, block_z: i32) -> Option<i32> {
        let range = self
            .authority
            .dimension_range(self.authority.current_dimension())?;
        let chunk = ChunkKey::new(
            self.authority.current_dimension(),
            block_x.div_euclid(16),
            block_z.div_euclid(16),
        );
        if !self.loaded_columns.contains(&chunk) {
            return None;
        }
        let keys = (0..range.sub_chunk_count)
            .map(|offset| SubChunkKey::from_chunk(chunk, range.base_sub_chunk_y + offset as i32));

        let local_x = block_x.rem_euclid(16) as u8;
        let local_z = block_z.rem_euclid(16) as u8;
        for key in keys.rev() {
            if self.known_air.contains(&key) {
                continue;
            }
            let Some(sub_chunk) = self.authority.terrain().sub_chunk(key) else {
                continue;
            };
            for local_y in (0_u8..16).rev() {
                let solid = (0..sub_chunk.storages().len()).any(|layer| {
                    sub_chunk
                        .runtime_id(layer, local_x, local_y, local_z)
                        .is_some_and(|runtime_id| !self.classifier.is_air(runtime_id))
                });
                if solid {
                    return Some(key.y.saturating_mul(16) + i32::from(local_y));
                }
            }
        }
        None
    }
    pub fn cave_visible_sub_chunks(&self, camera: SubChunkKey) -> HashSet<SubChunkKey> {
        crate::culling::cave_visible_sub_chunks(camera, &self.connectivity)
    }
    /// Whether the face-connectivity graph covers `key`; the cave culler can only hide those.
    #[must_use]
    pub fn has_sub_chunk_connectivity(&self, key: SubChunkKey) -> bool {
        self.connectivity.contains_key(&key)
    }
}

impl WorldStream {
    #[must_use]
    pub const fn current_dimension(&self) -> i32 {
        self.authority.current_dimension()
    }

    #[must_use]
    pub fn dimension_range(&self, dimension: i32) -> Option<DimensionRange> {
        self.authority.dimension_range(dimension)
    }

    pub fn dimension_transfer_area_ready(&self, position: [f32; 3]) -> bool {
        self.authority.dimension_transfer_area_ready(position)
    }

    /// The validated sequence of the last committed dimension transition.
    #[must_use]
    pub const fn form_dimension_epoch(&self) -> u64 {
        self.authority.form_dimension_epoch()
    }

    #[must_use]
    pub const fn local_movement_speed(&self) -> Option<f64> {
        self.authority.local_movement_speed()
    }

    /// Packed palette store used by read-only local collision queries.
    #[must_use]
    pub const fn collision_store(&self) -> &world::ChunkStore {
        self.authority.terrain()
    }

    /// Runtime identity mode carried by block palettes in this session.
    #[must_use]
    pub const fn network_id_mode(&self) -> assets::NetworkIdMode {
        self.authority.network_id_mode()
    }

    /// The block assets this stream meshes with, including any session overlay.
    #[must_use]
    pub fn runtime_assets(&self) -> &std::sync::Arc<assets::RuntimeAssets> {
        self.authority.runtime_assets()
    }
}

impl WorldStream {
    #[must_use]
    pub const fn biome_tint_revision(&self) -> u64 {
        self.authority.biome_tint_revision()
    }
}

impl WorldStream {
    #[must_use]
    pub const fn biome_tint_identity(&self) -> ChunkBiomeTintIdentity {
        ChunkBiomeTintIdentity::new(
            self.authority.biome_tint_stream_id(),
            self.authority.biome_tint_revision(),
        )
    }
}

impl WorldStream {
    #[must_use]
    pub const fn committed_view_cohort(&self) -> Option<ViewCohort> {
        self.publisher.cohort
    }
}

impl WorldStream {
    #[must_use]
    pub const fn local_player_runtime_id(&self) -> u64 {
        self.authority.local_player_runtime_id()
    }
}

impl WorldStream {
    #[must_use]
    pub const fn resolved_server_position(&self) -> ResolvedServerPosition {
        self.authority.resolved_server_position()
    }
}

impl WorldStream {
    #[must_use]
    pub const fn connectivity_generation(&self) -> u64 {
        self.connectivity_generation
    }
}
