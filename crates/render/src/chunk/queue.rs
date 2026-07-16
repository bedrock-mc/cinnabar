use crate::chunk::*;

pub(in crate::chunk) const DEFAULT_RENDER_QUEUE_ITEMS: usize = 256;
pub(in crate::chunk) const DEFAULT_RENDER_QUEUE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkRenderQueueLimits {
    pub max_items: usize,
    pub max_bytes: u64,
}

impl Default for ChunkRenderQueueLimits {
    fn default() -> Self {
        Self {
            max_items: DEFAULT_RENDER_QUEUE_ITEMS,
            max_bytes: DEFAULT_RENDER_QUEUE_BYTES,
        }
    }
}

/// Main-world insertion/update/removal API for packed sub-chunk meshes.
///
/// Re-enqueuing a key replaces its pending value, so rapid block updates are
/// deduplicated before they consume the per-frame GPU upload budget.
#[derive(Resource)]
pub struct ChunkRenderQueue {
    pub(in crate::chunk) pending: HashMap<SubChunkKey, PendingUpload>,
    pub(in crate::chunk) removals: HashMap<SubChunkKey, PendingRemoval>,
    pub(in crate::chunk) render_manifest: BTreeMap<SubChunkKey, u64>,
    pub(in crate::chunk) next_generation: u64,
    pub(in crate::chunk) pending_bytes: u64,
    pub(in crate::chunk) limits: ChunkRenderQueueLimits,
    pub(in crate::chunk) gpu_upload_bytes: u64,
}

impl Default for ChunkRenderQueue {
    fn default() -> Self {
        Self::with_limits(ChunkRenderQueueLimits::default())
    }
}

impl ChunkRenderQueue {
    #[must_use]
    pub fn with_limits(limits: ChunkRenderQueueLimits) -> Self {
        Self {
            pending: HashMap::new(),
            removals: HashMap::new(),
            render_manifest: BTreeMap::new(),
            next_generation: 0,
            pending_bytes: 0,
            limits,
            gpu_upload_bytes: 0,
        }
    }

    pub fn try_insert(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(
            key,
            mesh,
            PackedBiomeRecord::fallback(),
            ChunkBiomeTintIdentity::default(),
            priority,
            None,
        )
        .map_err(|(mesh, _)| mesh)
    }

    pub fn try_insert_with_biome(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            priority,
            None,
        )
    }

    pub fn try_insert_with_biome_revision(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_revision: u64,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_insert_with_biome_identity(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::new(0, tint_revision),
            priority,
        )
    }

    pub fn try_insert_with_biome_identity(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(key, mesh, biome, tint_identity, priority, None)
    }

    pub fn try_update(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(
            key,
            mesh,
            PackedBiomeRecord::fallback(),
            ChunkBiomeTintIdentity::default(),
            priority,
            None,
        )
        .map_err(|(mesh, _)| mesh)
    }

    pub fn try_update_with_biome(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            priority,
            None,
        )
    }

    pub fn try_update_tracked(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(
            key,
            mesh,
            PackedBiomeRecord::fallback(),
            ChunkBiomeTintIdentity::default(),
            priority,
            Some(token),
        )
        .map_err(|(mesh, _)| mesh)
    }

    pub fn try_update_tracked_with_biome(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            priority,
            Some(token),
        )
    }

    pub fn try_update_tracked_with_biome_revision(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_revision: u64,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_update_tracked_with_biome_identity(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::new(0, tint_revision),
            priority,
            token,
        )
    }

    pub fn try_update_tracked_with_biome_identity(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(key, mesh, biome, tint_identity, priority, Some(token))
    }

    pub fn try_remove(&mut self, key: SubChunkKey) -> Result<(), SubChunkKey> {
        self.try_remove_inner(key, ChunkUploadPriority::new(0.0), None)
    }

    pub fn try_remove_tracked(
        &mut self,
        key: SubChunkKey,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), SubChunkKey> {
        self.try_remove_inner(key, priority, Some(token))
    }

    pub(in crate::chunk) fn try_remove_inner(
        &mut self,
        key: SubChunkKey,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
    ) -> Result<(), SubChunkKey> {
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        if !replaces_existing && self.retained_len() >= self.limits.max_items {
            return Err(key);
        }
        if let Some(pending) = self.pending.remove(&key) {
            self.pending_bytes = self
                .pending_bytes
                .saturating_sub(pending_upload_byte_len(&pending));
        }
        self.removals
            .insert(key, PendingRemoval { priority, token });
        self.render_manifest.remove(&key);
        Ok(())
    }

    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn retained_len(&self) -> usize {
        self.pending.len().saturating_add(self.removals.len())
    }

    #[must_use]
    pub const fn pending_bytes(&self) -> u64 {
        self.pending_bytes
    }

    #[must_use]
    pub const fn gpu_upload_bytes(&self) -> u64 {
        self.gpu_upload_bytes
    }

    pub fn record_gpu_upload_bytes(&mut self, bytes: u64) {
        self.gpu_upload_bytes = self.gpu_upload_bytes.saturating_add(bytes);
    }

    #[must_use]
    pub fn freeze_target_expectation(
        &self,
        cohort: RenderViewCohort,
        source_cohort: Option<RenderViewCohort>,
        view_generation: u64,
        render_ready_at: Instant,
    ) -> TargetRenderExpectation {
        let manifest = self
            .render_manifest
            .iter()
            .filter_map(|(&key, &generation)| cohort.contains(key).then_some((key, generation)))
            .collect::<Vec<_>>();
        TargetRenderExpectation {
            cohort,
            source_cohort,
            manifest: Arc::from(manifest),
            view_generation,
            render_ready_at,
        }
    }

    /// Freezes only the requested target keys for model-witness evidence.
    ///
    /// Returns `None` for an empty request or when any requested key is outside
    /// the active view cohort so callers cannot accidentally certify a stale or
    /// unrelated view.
    #[must_use]
    pub fn freeze_target_expectation_for_keys(
        &self,
        cohort: RenderViewCohort,
        source_cohort: Option<RenderViewCohort>,
        keys: impl IntoIterator<Item = SubChunkKey>,
        view_generation: u64,
        render_ready_at: Instant,
    ) -> Option<TargetRenderExpectation> {
        let keys = keys.into_iter().collect::<BTreeSet<_>>();
        if keys.is_empty() || keys.iter().any(|&key| !cohort.contains(key)) {
            return None;
        }
        let manifest = keys
            .into_iter()
            .filter_map(|key| {
                self.render_manifest
                    .get(&key)
                    .copied()
                    .map(|generation| (key, generation))
            })
            .collect::<Vec<_>>();
        Some(TargetRenderExpectation {
            cohort,
            source_cohort,
            manifest: Arc::from(manifest),
            view_generation,
            render_ready_at,
        })
    }

    pub(in crate::chunk) fn try_enqueue(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        let old_bytes = self.pending.get(&key).map_or(0, pending_upload_byte_len);
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        let next_items = self
            .retained_len()
            .saturating_add(usize::from(!replaces_existing));
        let next_bytes = self
            .pending_bytes
            .saturating_sub(old_bytes)
            .saturating_add(mesh_byte_len(&mesh))
            .saturating_add(biome_record_byte_len(&biome));
        if next_items > self.limits.max_items || next_bytes > self.limits.max_bytes {
            return Err((mesh, biome));
        }
        self.removals.remove(&key);
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = token.map_or(self.next_generation, |token| token.generation);
        if mesh.is_empty() {
            self.render_manifest.remove(&key);
        } else {
            self.render_manifest.insert(key, generation);
        }
        self.pending_bytes = next_bytes;
        self.pending.insert(
            key,
            PendingUpload {
                mesh,
                biome,
                tint_identity,
                priority,
                generation,
                token,
            },
        );
        Ok(())
    }
}

pub(in crate::chunk) fn mesh_byte_len(mesh: &ChunkMesh) -> u64 {
    buffer_byte_len(mesh.cube_quads().len(), PACKED_QUAD_BYTES)
        .saturating_add(buffer_byte_len(
            mesh.cube_lighting().len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.model_refs().len(),
            PACKED_MODEL_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.model_lighting().len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.model_draw_refs().len(),
            PACKED_MODEL_DRAW_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.transparent_model_draw_refs().len(),
            PACKED_MODEL_DRAW_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.liquid_quads().len(),
            PACKED_LIQUID_QUAD_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            mesh.liquid_lighting().len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
}

pub(in crate::chunk) fn biome_record_is_fallback(record: &PackedBiomeRecord) -> bool {
    record.words() == FALLBACK_BIOME_RECORD
}

pub(in crate::chunk) fn biome_record_byte_len(record: &PackedBiomeRecord) -> u64 {
    if biome_record_is_fallback(record) {
        0
    } else {
        record.byte_len()
    }
}

pub(in crate::chunk) fn pending_upload_byte_len(pending: &PendingUpload) -> u64 {
    mesh_byte_len(&pending.mesh).saturating_add(biome_record_byte_len(&pending.biome))
}

pub(in crate::chunk) fn update_chunk_animation_clock(
    time: Option<Res<Time>>,
    mut clock: ResMut<ChunkAnimationClock>,
) {
    let elapsed_seconds = time.map_or(0.0, |time| time.elapsed_secs_f64());
    *clock = ChunkAnimationClock::from_elapsed_seconds(elapsed_seconds);
}

pub(in crate::chunk) fn apply_chunk_render_queue(
    mut commands: Commands,
    mut queue: ResMut<ChunkRenderQueue>,
    budget: Res<ChunkUploadBudget>,
    mut entities: ResMut<ChunkEntities>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
) {
    let mut ready = queue
        .pending
        .iter()
        .map(|(&key, pending)| (key, pending.priority, false))
        .chain(
            queue
                .removals
                .iter()
                .map(|(&key, pending)| (key, pending.priority, true)),
        )
        .collect::<Vec<_>>();
    ready.sort_by(|(left_key, left, _), (right_key, right, _)| {
        left.distance_squared()
            .total_cmp(&right.distance_squared())
            .then_with(|| left_key.cmp(right_key))
    });

    let mut total_applications = 0;
    let mut non_empty_uploads = 0;
    for (key, _, removal) in ready {
        if total_applications >= DEFAULT_RENDER_QUEUE_ITEMS {
            break;
        }
        if removal {
            let token = queue.removals.get(&key).and_then(|pending| pending.token);
            if token.is_some_and(|token| !acknowledgements.try_reserve(key, token)) {
                continue;
            }
            queue.removals.remove(&key);
            if let Some(entity) = entities.0.remove(&key) {
                commands.entity(entity).despawn();
            }
            if let Some(token) = token {
                acknowledgements.complete(key, token, Instant::now());
            }
            total_applications += 1;
            continue;
        }
        let Some(pending) = queue.pending.get(&key) else {
            continue;
        };
        if !pending.mesh.is_empty() && non_empty_uploads >= budget.max_per_frame {
            continue;
        }
        if pending.mesh.is_empty()
            && pending
                .token
                .is_some_and(|token| !acknowledgements.try_reserve(key, token))
        {
            continue;
        }
        let Some(pending) = queue.pending.remove(&key) else {
            continue;
        };
        queue.pending_bytes = queue
            .pending_bytes
            .saturating_sub(pending_upload_byte_len(&pending));
        if pending.mesh.is_empty() {
            if let Some(entity) = entities.0.remove(&key) {
                commands.entity(entity).despawn();
            }
            if let Some(token) = pending.token {
                acknowledgements.complete(key, token, Instant::now());
            }
            total_applications += 1;
            continue;
        }

        let origin = chunk_origin(key);
        let (
            cube_quads,
            cube_lighting,
            model_refs,
            model_lighting,
            model_draw_refs,
            transparent_model_draw_refs,
            liquid_quads,
            liquid_lighting,
        ) = pending.mesh.into_streams();
        debug_assert_eq!(cube_quads.len(), cube_lighting.len());
        let depth_liquid_start = liquid_quads
            .iter()
            .position(|quad| quad.is_depth_writing())
            .and_then(|index| u32::try_from(index).ok());
        debug_assert!(depth_liquid_start.is_none_or(|start| {
            liquid_quads[start as usize..]
                .iter()
                .all(|quad| quad.is_depth_writing())
        }));
        let has_depth_liquid = depth_liquid_start.is_some();
        let has_transparent_liquid = liquid_quads
            .first()
            .is_some_and(|quad| !quad.is_depth_writing());
        let instance = ChunkRenderInstance {
            key,
            cube_quads: Arc::from(cube_quads),
            cube_lighting: Arc::from(cube_lighting),
            model_refs: Arc::from(model_refs),
            model_lighting: Arc::from(model_lighting),
            model_draw_refs: Arc::from(model_draw_refs),
            transparent_model_draw_refs: Arc::from(transparent_model_draw_refs),
            liquid_quads: Arc::from(liquid_quads),
            liquid_lighting: Arc::from(liquid_lighting),
            has_depth_liquid,
            has_transparent_liquid,
            depth_liquid_start,
            biome: pending.biome,
            tint_identity: pending.tint_identity,
            generation: pending.generation,
            token: pending.token,
            origin,
        };
        if let Some(&entity) = entities.0.get(&key) {
            commands.entity(entity).insert(instance);
        } else {
            let entity = commands
                .spawn((
                    instance,
                    Visibility::default(),
                    Transform::from_xyz(origin[0] as f32, origin[1] as f32, origin[2] as f32),
                    Aabb {
                        center: Vec3A::splat(8.0),
                        half_extents: Vec3A::splat(8.0),
                    },
                ))
                .id();
            entities.0.insert(key, entity);
        }
        total_applications += 1;
        non_empty_uploads += 1;
    }
}

pub(in crate::chunk) const fn chunk_origin(key: SubChunkKey) -> [i32; 3] {
    [
        key.x.saturating_mul(16),
        key.y.saturating_mul(16),
        key.z.saturating_mul(16),
    ]
}
