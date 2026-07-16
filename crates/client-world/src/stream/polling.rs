use super::*;

impl WorldStream {
    pub fn poll(&mut self, camera_position: [f32; 3], max_mesh_jobs: usize) -> WorldStreamPoll {
        let mut report = WorldStreamPoll::default();
        while let Ok(completion) = self.decode_rx.try_recv() {
            report.decoded_results += 1;
            self.accept_decode_completion(completion);
        }
        self.apply_ready();
        self.expire_sub_chunk_deadlines(Instant::now());
        self.pump_deferred_retries();
        self.dispatch_decode_jobs();

        while let Ok(completion) = self.light_rx.try_recv() {
            report.light_results += 1;
            self.accept_light_completion(completion);
        }
        report.light_jobs_dispatched =
            self.dispatch_light_jobs(camera_position, LIGHT_DISPATCH_BUDGET_PER_POLL);

        while self.mesh_changes.len() < MAX_PENDING_MESH_CHANGES {
            let Ok(completion) = self.mesh_rx.try_recv() else {
                break;
            };
            report.mesh_results += 1;
            self.accept_mesh_completion(completion);
        }
        report.mesh_jobs_dispatched = self.dispatch_mesh_jobs(
            camera_position,
            max_mesh_jobs.min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len())),
        );
        report
    }
    pub fn camera_medium(&self, position: [f32; 3]) -> CameraMedium {
        if !position.iter().all(|value| value.is_finite()) {
            return CameraMedium::Air;
        }
        let block = position.map(floor_to_i32);
        let key = SubChunkKey::new(
            self.current_dimension,
            block[0].div_euclid(16),
            block[1].div_euclid(16),
            block[2].div_euclid(16),
        );
        let Some(center) = self.store.sub_chunk(key) else {
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
            if let Some(sub_chunk) = self.store.sub_chunk(neighbour_key) {
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
            &self.runtime_assets,
            self.network_id_mode,
            &neighbourhood,
            local_position,
        )
    }
    #[must_use]
    pub fn camera_biome_id(&self, position: [f32; 3]) -> Option<u32> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let block = position.map(floor_to_i32);
        let key = SubChunkKey::new(
            self.current_dimension,
            block[0].div_euclid(16),
            block[1].div_euclid(16),
            block[2].div_euclid(16),
        );
        self.store.biome_id(
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
        self.active_radius_chunks().max(0).saturating_mul(16) as f32
    }
    pub fn biome_definitions_snapshot(&self) -> Arc<[BiomeDefinitionEvent]> {
        Arc::clone(&self.biome_definitions)
    }
    pub fn resolved_biome_tints_snapshot(&self) -> Arc<ResolvedBiomeTints> {
        Arc::clone(&self.resolved_biome_tints)
    }
    pub fn connectivity(&self, key: SubChunkKey) -> Option<FaceConnectivity> {
        self.connectivity.get(&key).copied()
    }
    pub fn surface_eye_position(&self, block_x: i32, block_z: i32) -> Option<[f32; 3]> {
        let range = vanilla_dimension_range(self.current_dimension)?;
        let chunk = ChunkKey::new(
            self.current_dimension,
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
            let Some(sub_chunk) = self.store.sub_chunk(key) else {
                continue;
            };
            for local_y in (0_u8..16).rev() {
                let solid = (0..sub_chunk.storages().len()).any(|layer| {
                    sub_chunk
                        .runtime_id(layer, local_x, local_y, local_z)
                        .is_some_and(|runtime_id| !self.classifier.is_air(runtime_id))
                });
                if solid {
                    let block_y = key.y.saturating_mul(16) + i32::from(local_y);
                    return Some([
                        block_x as f32 + 0.5,
                        block_y as f32 + 2.62,
                        block_z as f32 + 0.5,
                    ]);
                }
            }
        }
        None
    }
    pub fn cave_visible_sub_chunks(&self, camera: SubChunkKey) -> BTreeSet<SubChunkKey> {
        crate::culling::cave_visible_sub_chunks(camera, &self.connectivity)
            .into_iter()
            .collect()
    }
}

impl WorldStream {
    #[must_use]
    pub const fn current_dimension(&self) -> i32 {
        self.current_dimension
    }
}

impl WorldStream {
    #[must_use]
    pub const fn biome_tint_revision(&self) -> u64 {
        self.biome_tint_revision
    }
}

impl WorldStream {
    #[must_use]
    pub const fn biome_tint_identity(&self) -> ChunkBiomeTintIdentity {
        ChunkBiomeTintIdentity::new(self.biome_tint_stream_id, self.biome_tint_revision)
    }
}

impl WorldStream {
    #[must_use]
    pub const fn committed_view_cohort(&self) -> Option<ViewCohort> {
        self.committed_view_cohort
    }
}

impl WorldStream {
    #[must_use]
    pub const fn local_player_runtime_id(&self) -> u64 {
        self.local_player_runtime_id
    }
}

impl WorldStream {
    #[must_use]
    pub const fn resolved_server_position(&self) -> ResolvedServerPosition {
        self.resolved_server_position
    }
}

impl WorldStream {
    #[must_use]
    pub const fn connectivity_generation(&self) -> u64 {
        self.connectivity_generation
    }
}
