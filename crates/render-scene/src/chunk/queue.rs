use crate::chunk::*;
use crate::{ChunkUploadPriority, ChunkUploadToken, RenderViewCohort, TargetRenderExpectation};
pub const DEFAULT_RENDER_QUEUE_ITEMS: usize = 512;

pub const DEFAULT_RENDER_QUEUE_BYTES: u64 = 64 * 1024 * 1024;

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
    pub pending: HashMap<SubChunkKey, PendingUpload>,
    pub removals: HashMap<SubChunkKey, PendingRemoval>,
    pub render_manifest: BTreeMap<SubChunkKey, u64>,
    /// Newest tracked generation per queued or presented key; pruned once its removal applies.
    pub tracked_generations: HashMap<SubChunkKey, u64>,
    pub next_generation: u64,
    pub pending_bytes: u64,
    pub limits: ChunkRenderQueueLimits,
    pub gpu_upload_bytes: u64,
    pub ready_scratch: Vec<(SubChunkKey, ChunkUploadPriority, bool)>,
    pub session_reset_pending: bool,
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
            tracked_generations: HashMap::new(),
            next_generation: 0,
            pending_bytes: 0,
            limits,
            ready_scratch: Vec::new(),
            gpu_upload_bytes: 0,
            session_reset_pending: false,
        }
    }

    /// Discards the previous world's queued work and retires its render entities
    /// at the next queue application, before applying any new-world uploads.
    /// New uploads receive fresh entities even at reused coordinates. The
    /// untracked generation counter is preserved. This is teardown, not a
    /// block update; old GPU allocations follow the existing retirement fence.
    pub fn reset_session(&mut self) {
        self.pending.clear();
        self.removals.clear();
        self.render_manifest.clear();
        self.tracked_generations.clear();
        self.pending_bytes = 0;
        self.ready_scratch.clear();
        self.session_reset_pending = true;
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
        if self.is_stale(key, Some(token)) {
            let _ = publication_permit.retire();
            return Ok(());
        }
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
        if self.is_stale(key, Some(token)) {
            let _ = publication_permit.retire();
            return Ok(());
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

    pub fn try_remove_inner(
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
        if self.is_stale(key, token) {
            if let Some(permit) = publication_permit {
                let _ = permit.retire();
            }
            return Ok(());
        }
        let priority = if priority.is_urgent()
            || self
                .pending
                .get(&key)
                .is_some_and(|pending| pending.priority.is_urgent())
            || self
                .removals
                .get(&key)
                .is_some_and(|pending| pending.priority.is_urgent())
        {
            ChunkUploadPriority::urgent()
        } else {
            priority
        };
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        if !replaces_existing && self.retained_len() >= self.limits.max_items {
            return Err(key);
        }
        let previous_generation = self.previous_manifest_generation(key);
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
                previous_generation,
                priority,
                token,
                publication_permit,
            },
        );
        self.render_manifest.remove(&key);
        self.record_tracked(key, token);
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

    /// Freezes render generations only for the caller's bounded required columns.
    ///
    /// Explicit columns define membership independently from the raw publisher
    /// geometry retained in `cohort`; they must be nonempty and belong to that
    /// cohort's dimension.
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
                .any(|column| column.dimension != cohort.dimension)
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

    pub fn try_enqueue(
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

    /// Retains the resident generation across coalesced uploads and removals.
    fn previous_manifest_generation(&self, key: SubChunkKey) -> Option<u64> {
        if let Some(pending) = self.pending.get(&key) {
            pending.previous_generation
        } else if let Some(removal) = self.removals.get(&key) {
            removal.previous_generation
        } else {
            self.render_manifest.get(&key).copied()
        }
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
        if self.is_stale(key, token) {
            if let Some(permit) = publication_permit {
                let _ = permit.retire();
            }
            return Ok(());
        }
        let priority = if priority.is_urgent()
            || self
                .pending
                .get(&key)
                .is_some_and(|pending| pending.priority.is_urgent())
            || self
                .removals
                .get(&key)
                .is_some_and(|pending| pending.priority.is_urgent())
        {
            ChunkUploadPriority::urgent()
        } else {
            priority
        };
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
        let previous_generation = self.previous_manifest_generation(key);
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
                previous_generation,
                mesh,
                biome,
                tint_identity,
                priority,
                generation,
                token,
                publication_permit,
            },
        );
        self.record_tracked(key, token);
        Ok(())
    }

    /// Stream generations only grow within a session, so an older tracked change was
    /// reordered behind a newer one; applying it would resurrect stale geometry or a hole.
    fn is_stale(&self, key: SubChunkKey, token: Option<ChunkUploadToken>) -> bool {
        token.is_some_and(|token| {
            self.tracked_generations
                .get(&key)
                .is_some_and(|&newest| newest > token.generation)
        })
    }

    fn record_tracked(&mut self, key: SubChunkKey, token: Option<ChunkUploadToken>) {
        if let Some(token) = token {
            self.tracked_generations.insert(key, token.generation);
        }
    }
}

pub fn biome_record_is_fallback(record: &PackedBiomeRecord) -> bool {
    record.is_fallback()
}

pub fn biome_record_byte_len(record: &PackedBiomeRecord) -> u64 {
    if biome_record_is_fallback(record) {
        0
    } else {
        record.byte_len()
    }
}

pub fn pending_upload_byte_len(pending: &PendingUpload) -> u64 {
    chunk_publication_byte_len(&pending.mesh, &pending.biome)
}

pub const fn chunk_origin(key: SubChunkKey) -> [i32; 3] {
    [
        key.x.saturating_mul(16),
        key.y.saturating_mul(16),
        key.z.saturating_mul(16),
    ]
}
