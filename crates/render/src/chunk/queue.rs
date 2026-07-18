use crate::chunk::*;

pub(in crate::chunk) const DEFAULT_RENDER_QUEUE_ITEMS: usize = 512;
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

    #[must_use]
    pub fn upload_byte_len(mesh: &ChunkMesh, biome: &PackedBiomeRecord) -> u64 {
        chunk_publication_byte_len(mesh, biome)
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

    // The rejection path deliberately returns every owned input, including the
    // linear permit, so callers can retry without losing either work or authority.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn try_update_tracked_with_biome_identity_permitted(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
        publication_permit: PublicationPermit,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord, PublicationPermit)> {
        let expected_bytes = chunk_publication_byte_len(&mesh, &biome);
        let permit_matches = publication_permit.bytes() == Some(expected_bytes)
            && publication_permit.is_zero_byte() == (expected_bytes == 0);
        if !permit_matches {
            return Err((mesh, biome, publication_permit));
        }
        let old_bytes = self.pending.get(&key).map_or(0, pending_upload_byte_len);
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        let next_items = self.retained_len() + usize::from(!replaces_existing);
        let next_bytes = self
            .pending_bytes
            .saturating_sub(old_bytes)
            .saturating_add(expected_bytes);
        if next_items > self.limits.max_items || next_bytes > self.limits.max_bytes {
            return Err((mesh, biome, publication_permit));
        }
        let publication_permit = match publication_permit.into_handoff() {
            Ok(permit) => permit,
            Err(permit) => return Err((mesh, biome, permit)),
        };
        self.try_enqueue_inner(
            key,
            mesh,
            biome,
            tint_identity,
            priority,
            Some(token),
            Some(publication_permit),
        )
        .map_err(|(mesh, biome)| {
            unreachable!(
                "capacity was checked before linear permit transfer: {} {}",
                mesh.is_empty(),
                biome.byte_len()
            )
        })
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

    pub fn try_remove_tracked_permitted(
        &mut self,
        key: SubChunkKey,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
        publication_permit: PublicationPermit,
    ) -> Result<(), (SubChunkKey, PublicationPermit)> {
        if !publication_permit.is_zero_byte() || publication_permit.bytes() != Some(0) {
            return Err((key, publication_permit));
        }
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        if !replaces_existing && self.retained_len() >= self.limits.max_items {
            return Err((key, publication_permit));
        }
        let publication_permit = match publication_permit.into_handoff() {
            Ok(permit) => permit,
            Err(permit) => return Err((key, permit)),
        };
        self.try_remove_inner_permitted(key, priority, Some(token), Some(publication_permit))
            .map_err(|key| unreachable!("capacity was checked before permit transfer: {key:?}"))
    }

    pub(in crate::chunk) fn try_remove_inner(
        &mut self,
        key: SubChunkKey,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
    ) -> Result<(), SubChunkKey> {
        self.try_remove_inner_permitted(key, priority, token, None)
    }

    fn try_remove_inner_permitted(
        &mut self,
        key: SubChunkKey,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
        publication_permit: Option<PublicationPermit>,
    ) -> Result<(), SubChunkKey> {
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        if !replaces_existing && self.retained_len() >= self.limits.max_items {
            return Err(key);
        }
        if let Some(pending) = self.pending.remove(&key) {
            self.pending_bytes = self
                .pending_bytes
                .saturating_sub(pending_upload_byte_len(&pending));
            if let Some(permit) = pending.publication_permit {
                let _ = permit.retire();
            }
        }
        if let Some(pending) = self.removals.remove(&key)
            && let Some(permit) = pending.publication_permit
        {
            let _ = permit.retire();
        }
        self.removals.insert(
            key,
            PendingRemoval {
                priority,
                token,
                publication_permit,
            },
        );
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
            target_columns: None,
            target_keys: None,
            manifest: Arc::from(manifest),
            view_generation,
            render_ready_at,
        }
    }

    /// Freezes render generations only for actually announced required columns.
    #[must_use]
    pub fn freeze_target_expectation_for_columns(
        &self,
        cohort: RenderViewCohort,
        source_cohort: Option<RenderViewCohort>,
        columns: impl IntoIterator<Item = world::ChunkKey>,
        view_generation: u64,
        render_ready_at: Instant,
    ) -> Option<TargetRenderExpectation> {
        let columns = columns.into_iter().collect::<BTreeSet<_>>();
        if columns.is_empty()
            || columns
                .iter()
                .any(|column| !cohort.contains(SubChunkKey::from_chunk(*column, 0)))
        {
            return None;
        }
        let manifest = self
            .render_manifest
            .iter()
            .filter_map(|(&key, &generation)| {
                columns.contains(&key.chunk()).then_some((key, generation))
            })
            .collect::<Vec<_>>();
        Some(TargetRenderExpectation {
            cohort,
            source_cohort,
            target_columns: Some(Arc::from(columns.into_iter().collect::<Vec<_>>())),
            target_keys: None,
            manifest: Arc::from(manifest),
            view_generation,
            render_ready_at,
        })
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
            .iter()
            .copied()
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
            target_columns: None,
            target_keys: Some(Arc::from(keys.iter().copied().collect::<Vec<_>>())),
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
        self.try_enqueue_inner(key, mesh, biome, tint_identity, priority, token, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_enqueue_inner(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
        publication_permit: Option<PublicationPermit>,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        let old_bytes = self.pending.get(&key).map_or(0, pending_upload_byte_len);
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        let next_items = self
            .retained_len()
            .saturating_add(usize::from(!replaces_existing));
        let next_bytes = self
            .pending_bytes
            .saturating_sub(old_bytes)
            .saturating_add(chunk_publication_byte_len(&mesh, &biome));
        if next_items > self.limits.max_items || next_bytes > self.limits.max_bytes {
            return Err((mesh, biome));
        }
        if let Some(pending) = self.removals.remove(&key)
            && let Some(permit) = pending.publication_permit
        {
            let _ = permit.retire();
        }
        if let Some(previous) = self.pending.remove(&key)
            && let Some(permit) = previous.publication_permit
        {
            let _ = permit.retire();
        }
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
                publication_permit,
            },
        );
        Ok(())
    }
}

pub(in crate::chunk) fn biome_record_is_fallback(record: &PackedBiomeRecord) -> bool {
    record.is_fallback()
}

pub(in crate::chunk) fn biome_record_byte_len(record: &PackedBiomeRecord) -> u64 {
    if biome_record_is_fallback(record) {
        0
    } else {
        record.byte_len()
    }
}

pub(in crate::chunk) fn pending_upload_byte_len(pending: &PendingUpload) -> u64 {
    chunk_publication_byte_len(&pending.mesh, &pending.biome)
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
    existing_instances: Query<&ChunkRenderInstance>,
    gpu_removals: Res<ChunkGpuRemovalQueue>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
) {
    let maximum_zero_byte_operations = budget
        .max_zero_byte_operations_per_frame
        .min(PublicationServiceConfig::PHASE2_GATE.maximum_zero_byte_operations_per_frame);
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

    let mut payload_applications = 0;
    let mut total_bytes = 0;
    let mut zero_byte_applications = 0;
    for (key, _, removal) in ready {
        if removal {
            let permitted = queue
                .removals
                .get(&key)
                .is_some_and(|pending| pending.publication_permit.is_some());
            if zero_byte_applications >= maximum_zero_byte_operations
                || (permitted && !gpu_removals.has_capacity_for(key))
            {
                continue;
            }
            let token = queue.removals.get(&key).and_then(|pending| pending.token);
            if !permitted && token.is_some_and(|token| !acknowledgements.try_reserve(key, token)) {
                continue;
            }
            let Some(pending) = queue.removals.remove(&key) else {
                continue;
            };
            let render_permit = pending
                .publication_permit
                .map(PublicationPermit::into_render_entity)
                .transpose();
            let render_permit = match render_permit {
                Ok(permit) => permit,
                Err(permit) => {
                    drop(permit);
                    continue;
                }
            };
            if let Some(entity) = entities.0.remove(&key) {
                if let Ok(instance) = existing_instances.get(entity)
                    && let Some(slot) = &instance.publication_permit
                {
                    drop(slot.take());
                }
                commands.entity(entity).despawn();
            }
            if let Some(permit) = render_permit {
                gpu_removals
                    .push(PendingGpuRemoval { key, token, permit })
                    .unwrap_or_else(|_| unreachable!("removal mailbox capacity was checked"));
            } else if let Some(token) = token {
                acknowledgements.complete(key, token, Instant::now());
            }
            zero_byte_applications = zero_byte_applications.saturating_add(1);
            continue;
        }
        let Some(pending) = queue.pending.get(&key) else {
            continue;
        };
        let pending_bytes = if pending.mesh.is_empty() {
            0
        } else {
            pending_upload_byte_len(pending)
        };
        if pending.publication_permit.is_none()
            && pending_bytes != 0
            && !budget.can_fit(payload_applications, total_bytes, 1, pending_bytes)
        {
            continue;
        }
        if pending.mesh.is_empty()
            && ((zero_byte_applications >= maximum_zero_byte_operations
                || (pending.publication_permit.is_some() && !gpu_removals.has_capacity_for(key)))
                || pending.token.is_some_and(|token| {
                    pending.publication_permit.is_none()
                        && !acknowledgements.try_reserve(key, token)
                }))
        {
            continue;
        }
        let Some(pending) = queue.pending.remove(&key) else {
            continue;
        };
        queue.pending_bytes = queue
            .pending_bytes
            .saturating_sub(pending_upload_byte_len(&pending));
        let render_permit = pending
            .publication_permit
            .map(PublicationPermit::into_render_entity)
            .transpose();
        let render_permit = match render_permit {
            Ok(permit) => permit,
            Err(permit) => {
                drop(permit);
                continue;
            }
        };
        gpu_removals.cancel(key);
        if pending.mesh.is_empty() {
            if let Some(entity) = entities.0.remove(&key) {
                if let Ok(instance) = existing_instances.get(entity)
                    && let Some(slot) = &instance.publication_permit
                {
                    drop(slot.take());
                }
                commands.entity(entity).despawn();
            }
            if let Some(permit) = render_permit {
                gpu_removals
                    .push(PendingGpuRemoval {
                        key,
                        token: pending.token,
                        permit,
                    })
                    .unwrap_or_else(|_| unreachable!("removal mailbox capacity was checked"));
            } else if let Some(token) = pending.token {
                acknowledgements.complete(key, token, Instant::now());
            }
            zero_byte_applications = zero_byte_applications.saturating_add(1);
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
            publication_permit: render_permit.map(PublicationPermitSlot::new),
            origin,
        };
        if let Some(&entity) = entities.0.get(&key) {
            if let Ok(existing) = existing_instances.get(entity)
                && let Some(slot) = &existing.publication_permit
            {
                drop(slot.take());
            }
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
        if pending_bytes != 0 {
            payload_applications = payload_applications.saturating_add(1);
            total_bytes = total_bytes.saturating_add(pending_bytes);
        }
    }
}

pub(in crate::chunk) const fn chunk_origin(key: SubChunkKey) -> [i32; 3] {
    [
        key.x.saturating_mul(16),
        key.y.saturating_mul(16),
        key.z.saturating_mul(16),
    ]
}
