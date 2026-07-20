use crate::chunk::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct TransparentModelAllocationIdentity {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) model_range: Range<u32>,
    pub(in crate::chunk) draw_range: Range<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct TransparentModelAddressIdentity {
    pub(in crate::chunk) asset_identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) allocations: Arc<[TransparentModelAllocationIdentity]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct TransparentModelSortKey {
    pub(in crate::chunk) view_entity: Entity,
    pub(in crate::chunk) rotation_bits: [u32; 4],
    pub(in crate::chunk) address: TransparentModelAddressIdentity,
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentModelSortCandidate {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) draw_range: Range<u32>,
    pub(in crate::chunk) stable_index: u32,
    pub(in crate::chunk) centroid: Vec3,
    pub(in crate::chunk) words: [u32; 2],
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentModelCandidateCache {
    pub(in crate::chunk) address: TransparentModelAddressIdentity,
    pub(in crate::chunk) candidates: Arc<[TransparentModelSortCandidate]>,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentModelSortWork {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: TransparentModelSortKey,
    pub(in crate::chunk) view_from_world: Mat4,
    pub(in crate::chunk) candidates: Arc<[TransparentModelSortCandidate]>,
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentModelSortBatch {
    pub(in crate::chunk) draw_range: Range<u32>,
    pub(in crate::chunk) words: Box<[[u32; 2]]>,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentModelWorkerResult {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: TransparentModelSortKey,
    pub(in crate::chunk) batches: Vec<TransparentModelSortBatch>,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentModelStagedSort {
    pub(in crate::chunk) key: TransparentModelSortKey,
    pub(in crate::chunk) batches: VecDeque<TransparentModelSortBatch>,
}

#[derive(Resource)]
pub(in crate::chunk) struct TransparentModelSortRuntime {
    pub(in crate::chunk) next_generation: u64,
    pub(in crate::chunk) requested: Option<(ViewSortGeneration, TransparentModelSortKey)>,
    pub(in crate::chunk) committed: Option<TransparentModelSortKey>,
    pub(in crate::chunk) staged: Option<TransparentModelStagedSort>,
    pub(in crate::chunk) gate: TransparentSortJobGate<TransparentModelSortWork>,
    pub(in crate::chunk) result_sender: SyncSender<TransparentModelWorkerResult>,
    pub(in crate::chunk) result_receiver: Mutex<Receiver<TransparentModelWorkerResult>>,
    pub(in crate::chunk) candidate_cache: Option<TransparentModelCandidateCache>,
}

#[derive(Debug, Resource)]
pub(in crate::chunk) struct TransparentUploadBudget {
    pub(in crate::chunk) remaining_refs: usize,
}

impl Default for TransparentUploadBudget {
    fn default() -> Self {
        Self {
            remaining_refs: DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
        }
    }
}

impl TransparentUploadBudget {
    pub(in crate::chunk) fn reset(&mut self) {
        self.remaining_refs = DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME;
    }

    pub(in crate::chunk) const fn remaining(&self) -> usize {
        self.remaining_refs
    }

    pub(in crate::chunk) fn consume(&mut self, refs: usize) -> bool {
        let Some(remaining) = self.remaining_refs.checked_sub(refs) else {
            return false;
        };
        self.remaining_refs = remaining;
        true
    }
}

impl Default for TransparentModelSortRuntime {
    fn default() -> Self {
        let (result_sender, result_receiver) = sync_channel(1);
        Self {
            next_generation: 0,
            requested: None,
            committed: None,
            staged: None,
            gate: TransparentSortJobGate::default(),
            result_sender,
            result_receiver: Mutex::new(result_receiver),
            candidate_cache: None,
        }
    }
}

impl Default for TransparentSortRuntime {
    fn default() -> Self {
        let (result_sender, result_receiver) = sync_channel(1);
        Self {
            view_entity: None,
            state: TransparentSortState::with_upload_cap(DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME),
            gate: TransparentSortJobGate::default(),
            result_sender,
            result_receiver: Mutex::new(result_receiver),
            requested_at: HashMap::new(),
            staged_distinct_tint_counts: HashMap::new(),
            committed_distinct_tint_count: 0,
            last_indirect_identity: None,
            candidate_cache: None,
        }
    }
}

impl TransparentSortRuntime {
    pub(in crate::chunk) fn reset_for_view(&mut self, view_entity: Option<Entity>) {
        let next_generation = self.state.next_generation;
        *self = Self::default();
        self.state.next_generation = next_generation;
        self.view_entity = view_entity;
    }

    pub(in crate::chunk) fn fail_closed_conflicting_manifest(
        &mut self,
        metrics: &TransparentSortMetrics,
    ) {
        let view_entity = self.view_entity;
        self.reset_for_view(view_entity);
        clear_active_transparent_metrics(metrics);
    }

    pub(in crate::chunk) fn prune_request_metadata(&mut self) {
        let in_flight = self.gate.in_flight_generation();
        let pending = self.gate.pending_generation();
        let staged = self.state.staged_generation();
        self.requested_at.retain(|generation, _| {
            Some(*generation) == in_flight
                || Some(*generation) == pending
                || Some(*generation) == staged
        });
        self.staged_distinct_tint_counts
            .retain(|generation, _| Some(*generation) == staged);
    }

    pub(in crate::chunk) fn generation_needs_sort_job(
        &self,
        generation: ViewSortGeneration,
    ) -> bool {
        self.state.staged_generation() != Some(generation)
            && !self.gate.contains_generation(generation)
    }

    pub(in crate::chunk) fn resolve_candidate_cache(
        &mut self,
        key: &ViewSortKey,
        build: impl FnOnce() -> Result<(Vec<TransparentSortCandidate>, usize), TransparentSortError>,
    ) -> Result<(Arc<[TransparentSortCandidate]>, usize), TransparentSortError> {
        let address_identity = key.address_identity();
        if let Some(cache) = self
            .candidate_cache
            .as_ref()
            .filter(|cache| cache.address_identity == address_identity)
        {
            return Ok((Arc::clone(&cache.candidates), cache.distinct_tint_count));
        }
        self.candidate_cache = None;
        let (candidates, distinct_tint_count) = build()?;
        validate_transparent_sort_ref_count(candidates.len())?;
        let candidates = Arc::<[TransparentSortCandidate]>::from(candidates);
        self.candidate_cache = Some(TransparentCandidateCache {
            address_identity,
            candidates: Arc::clone(&candidates),
            distinct_tint_count,
        });
        Ok((candidates, distinct_tint_count))
    }
}

pub(in crate::chunk) fn transparent_request_to_commit_latency(
    requested_at: Instant,
    committed_at: Instant,
) -> Duration {
    committed_at.saturating_duration_since(requested_at)
}

pub(in crate::chunk) fn clear_active_transparent_metrics(metrics: &TransparentSortMetrics) {
    metrics.update(|snapshot| {
        snapshot.request_generation = 0;
        snapshot.result_generation = 0;
        snapshot.committed_generation = 0;
        snapshot.encoded_generation = 0;
        snapshot.presented_generation = 0;
        snapshot.ref_count = 0;
        snapshot.active_slot_age_frames = 0;
        snapshot.transparent_water_distinct_tint_count = 0;
    });
}

pub(in crate::chunk) fn fail_closed_transparent_sort_key_error(
    runtime: &mut TransparentSortRuntime,
    metrics: &TransparentSortMetrics,
    error: TransparentSortError,
) {
    match error {
        TransparentSortError::ConflictingAllocation { .. }
        | TransparentSortError::InvalidCameraTransform => {
            runtime.fail_closed_conflicting_manifest(metrics);
        }
        TransparentSortError::ReferenceCeiling { .. } => {}
    }
}

pub(in crate::chunk) fn spawn_transparent_sort(
    sender: SyncSender<TransparentWorkerResult>,
    work: TransparentSortWork,
) {
    rayon::spawn(move || {
        let started = Instant::now();
        let refs = sort_transparent_candidates(work.view_from_world, work.candidates);
        let _ = sender.try_send(TransparentWorkerResult {
            generation: work.generation,
            requested_at: work.requested_at,
            key: work.key,
            refs: Ok(refs),
            cpu_duration: started.elapsed(),
            distinct_tint_count: work.distinct_tint_count,
        });
    });
}

pub(in crate::chunk) fn transparent_model_subchunk_center(key: SubChunkKey) -> Vec3 {
    let origin = chunk_origin(key);
    Vec3::new(
        origin[0] as f32 + 8.0,
        origin[1] as f32 + 8.0,
        origin[2] as f32 + 8.0,
    )
}

pub(in crate::chunk) fn transparent_model_phase_distance(
    rangefinder: &ViewRangefinder3d,
    key: SubChunkKey,
) -> f32 {
    rangefinder.distance(&transparent_model_subchunk_center(key))
}

pub(in crate::chunk) fn transparent_model_draw_candidate(
    key: SubChunkKey,
    model_refs: &[PackedModelRef],
    draw_ref: PackedModelDrawRef,
    model_templates: &[ModelTemplate],
    model_quads: &[assets::ModelQuad],
    model_record_base: u32,
) -> Option<(Vec3, [u32; 2])> {
    let [chunk_x, chunk_y, chunk_z] = chunk_origin(key).map(|coordinate| coordinate as f32);
    let [local_model_ref, quad_index] = draw_ref.words();
    let model_ref = model_refs.get(local_model_ref as usize)?.words();
    let template = model_templates.get(model_ref[1] as usize)?;
    if quad_index >= template.quad_count || model_ref[3] & (1_u32 << quad_index) == 0 {
        return None;
    }
    let quad = model_quads.get(template.quad_start.checked_add(quad_index)? as usize)?;
    let mut centroid = quad.positions.iter().fold(Vec3::ZERO, |total, position| {
        total
            + Vec3::new(
                f32::from(position[0]),
                f32::from(position[1]),
                f32::from(position[2]),
            )
    }) / (4.0 * 256.0);
    let centered = centroid - Vec3::new(0.5, 0.0, 0.5);
    centroid = match (model_ref[0] >> 12) & 3 {
        1 => Vec3::new(-centered.z, centered.y, centered.x),
        2 => Vec3::new(-centered.x, centered.y, -centered.z),
        3 => Vec3::new(centered.z, centered.y, -centered.x),
        _ => centered,
    } + Vec3::new(0.5, 0.0, 0.5);
    let block = Vec3::new(
        (model_ref[0] & 15) as f32,
        ((model_ref[0] >> 4) & 15) as f32,
        ((model_ref[0] >> 8) & 15) as f32,
    );
    Some((
        Vec3::new(chunk_x, chunk_y, chunk_z) + block + centroid,
        [local_model_ref.checked_add(model_record_base)?, quad_index],
    ))
}

#[cfg(test)]
pub(in crate::chunk) fn sorted_transparent_model_draw_words(
    rangefinder: &ViewRangefinder3d,
    key: SubChunkKey,
    model_refs: &[PackedModelRef],
    draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
    model_quads: &[assets::ModelQuad],
    model_record_base: u32,
) -> Option<Vec<[u32; 2]>> {
    let mut sorted = Vec::with_capacity(draw_refs.len());
    for (stable_index, draw_ref) in draw_refs.iter().copied().enumerate() {
        let (world_centroid, words) = transparent_model_draw_candidate(
            key,
            model_refs,
            draw_ref,
            model_templates,
            model_quads,
            model_record_base,
        )?;
        sorted.push((rangefinder.distance(&world_centroid), stable_index, words));
    }
    sorted.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    Some(sorted.into_iter().map(|(_, _, words)| words).collect())
}

pub(in crate::chunk) fn canonical_transparent_rotation_bits(
    mut rotation: Quat,
) -> Option<[u32; 4]> {
    let norm_squared = rotation.length_squared();
    if !norm_squared.is_finite() || norm_squared == 0.0 {
        return None;
    }
    rotation *= norm_squared.sqrt().recip();
    let mut values = rotation.to_array();
    let anchor = [values[3], values[2], values[1], values[0]]
        .into_iter()
        .find(|value| *value != 0.0)
        .unwrap_or(1.0);
    if anchor.is_sign_negative() {
        values = values.map(|value| -value);
    }
    Some(values.map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}

pub(in crate::chunk) fn sort_transparent_model_candidates(
    view_from_world: Mat4,
    candidates: Arc<[TransparentModelSortCandidate]>,
) -> Vec<TransparentModelSortBatch> {
    let mut groups =
        HashMap::<Entity, (SubChunkKey, Range<u32>, Vec<TransparentModelSortCandidate>)>::new();
    for candidate in candidates.iter().cloned() {
        let entry = groups
            .entry(candidate.entity)
            .or_insert_with(|| (candidate.key, candidate.draw_range.clone(), Vec::new()));
        entry.2.push(candidate);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by_key(|(key, range, _)| (*key, range.start));
    groups
        .into_iter()
        .map(|(_, draw_range, mut candidates)| {
            candidates.sort_by(|left, right| {
                view_from_world
                    .transform_point3(left.centroid)
                    .z
                    .total_cmp(&view_from_world.transform_point3(right.centroid).z)
                    .then_with(|| left.stable_index.cmp(&right.stable_index))
            });
            TransparentModelSortBatch {
                draw_range,
                words: candidates
                    .into_iter()
                    .map(|candidate| candidate.words)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }
        })
        .collect()
}

pub(in crate::chunk) fn take_transparent_model_upload_batches(
    batches: &mut VecDeque<TransparentModelSortBatch>,
    upload_cap: usize,
) -> Vec<TransparentModelSortBatch> {
    let mut upload_remaining = upload_cap;
    let mut selected = Vec::new();
    while let Some(batch) = batches.front() {
        if batch.words.len() > upload_remaining {
            break;
        }
        upload_remaining -= batch.words.len();
        selected.push(
            batches
                .pop_front()
                .expect("front batch remains available while selecting uploads"),
        );
    }
    selected
}

pub(in crate::chunk) fn spawn_transparent_model_sort(
    sender: SyncSender<TransparentModelWorkerResult>,
    work: TransparentModelSortWork,
) {
    rayon::spawn(move || {
        let batches = sort_transparent_model_candidates(work.view_from_world, work.candidates);
        let _ = sender.try_send(TransparentModelWorkerResult {
            generation: work.generation,
            key: work.key,
            batches,
        });
    });
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_transparent_model_sorts(
    views: Query<(Entity, &ExtractedView, &RenderVisibleEntities), With<ExtractedCamera>>,
    instances: Query<&ChunkRenderInstance>,
    allocations: Query<&GpuChunkAllocation>,
    texture_assets: Res<ChunkTextureAssets>,
    arena: Res<ChunkGpuArena>,
    render_queue: Res<RenderQueue>,
    transparent_runtime: Res<TransparentSortRuntime>,
    mut upload_budget: ResMut<TransparentUploadBudget>,
    mut runtime: ResMut<TransparentModelSortRuntime>,
) {
    let Some((view_entity, view, visible_entities)) = views
        .iter()
        .find(|(entity, _, _)| transparent_runtime.view_entity == Some(*entity))
    else {
        runtime.committed = None;
        runtime.staged = None;
        runtime.candidate_cache = None;
        return;
    };
    let (_, rotation, _) = view.world_from_view.to_scale_rotation_translation();
    let Some(rotation_bits) = canonical_transparent_rotation_bits(rotation) else {
        return;
    };
    let mut identities = Vec::new();
    let mut total_refs = 0_usize;
    for &(entity, _) in visible_entities.get::<ChunkRenderInstance>() {
        let Ok(allocation) = allocations.get(entity) else {
            continue;
        };
        let Ok(instance) = instances.get(entity) else {
            continue;
        };
        if !transparent_model_allocation_matches(instance, allocation) {
            continue;
        }
        let (Some(model_range), Some(draw_range)) = (
            allocation.model_range.clone(),
            allocation.transparent_model_draw_range.clone(),
        ) else {
            continue;
        };
        if !model_range.start.is_multiple_of(4)
            || !draw_range.start.is_multiple_of(2)
            || !draw_range.end.is_multiple_of(2)
        {
            bevy::log::error!(key = ?allocation.key, "transparent model sort ranges are misaligned");
            return;
        }
        let ref_count = draw_range.end.saturating_sub(draw_range.start) as usize / 2;
        if ref_count > DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME {
            bevy::log::error!(key = ?allocation.key, "one transparent model sub-chunk exceeds the per-frame sort upload cap");
            return;
        }
        total_refs = total_refs.saturating_add(ref_count);
        if total_refs > MAX_TRANSPARENT_DRAW_REFS {
            bevy::log::error!("visible transparent model refs exceed the hard sort ceiling");
            return;
        }
        identities.push(TransparentModelAllocationIdentity {
            entity,
            key: allocation.key,
            generation: allocation.generation,
            model_range,
            draw_range,
        });
    }
    identities.sort_by_key(|identity| (identity.key, identity.draw_range.start));
    let address = TransparentModelAddressIdentity {
        asset_identity: texture_assets.identity(),
        allocations: Arc::from(identities),
    };
    let key = TransparentModelSortKey {
        view_entity,
        rotation_bits,
        address: address.clone(),
    };

    let completed = runtime
        .result_receiver
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .try_recv()
        .ok();
    if let Some(result) = completed {
        let next = runtime.gate.complete(result.generation);
        if runtime.requested.as_ref() == Some(&(result.generation, result.key.clone()))
            && result.key == key
        {
            runtime.staged = Some(TransparentModelStagedSort {
                key: result.key,
                batches: result.batches.into(),
            });
        }
        if let Some((_generation, work)) = next {
            spawn_transparent_model_sort(runtime.result_sender.clone(), work);
        }
    }
    if runtime
        .staged
        .as_ref()
        .is_some_and(|staged| staged.key != key)
    {
        runtime.staged = None;
    }
    if let Some(staged) = runtime.staged.as_mut() {
        let batches =
            take_transparent_model_upload_batches(&mut staged.batches, upload_budget.remaining());
        let uploaded_refs = batches.iter().map(|batch| batch.words.len()).sum();
        if !upload_budget.consume(uploaded_refs) {
            bevy::log::error!(
                "transparent model sort batches exceed the shared per-frame reference upload budget"
            );
            return;
        }
        for batch in batches {
            render_queue.write_buffer(
                &arena.geometry_stream_buffer,
                u64::from(batch.draw_range.start) * GEOMETRY_STREAM_WORD_BYTES,
                bytemuck::cast_slice(&batch.words),
            );
        }
        if staged.batches.is_empty() {
            runtime.committed = Some(staged.key.clone());
            runtime.staged = None;
        }
    }
    if runtime.committed.as_ref() == Some(&key)
        || runtime
            .staged
            .as_ref()
            .is_some_and(|staged| staged.key == key)
        || runtime
            .requested
            .as_ref()
            .is_some_and(|(_, requested)| requested == &key)
    {
        return;
    }

    if total_refs == 0 {
        runtime.requested = None;
        runtime.staged = None;
        runtime.committed = Some(key);
        runtime.candidate_cache = Some(TransparentModelCandidateCache {
            address,
            candidates: Arc::from([]),
        });
        return;
    }

    let candidates = if runtime
        .candidate_cache
        .as_ref()
        .is_some_and(|cache| cache.address == address)
    {
        Arc::clone(&runtime.candidate_cache.as_ref().unwrap().candidates)
    } else {
        let mut candidates = Vec::with_capacity(total_refs);
        for identity in address.allocations.iter() {
            let Ok(instance) = instances.get(identity.entity) else {
                return;
            };
            if instance.transparent_model_draw_refs.len().checked_mul(2)
                != Some(
                    identity
                        .draw_range
                        .end
                        .saturating_sub(identity.draw_range.start) as usize,
                )
            {
                return;
            }
            for (stable_index, draw_ref) in instance
                .transparent_model_draw_refs
                .iter()
                .copied()
                .enumerate()
            {
                let Some((centroid, words)) = transparent_model_draw_candidate(
                    identity.key,
                    &instance.model_refs,
                    draw_ref,
                    texture_assets.assets().model_templates(),
                    texture_assets.assets().model_quads(),
                    identity.model_range.start / 4,
                ) else {
                    return;
                };
                let Ok(stable_index) = u32::try_from(stable_index) else {
                    return;
                };
                candidates.push(TransparentModelSortCandidate {
                    entity: identity.entity,
                    key: identity.key,
                    draw_range: identity.draw_range.clone(),
                    stable_index,
                    centroid,
                    words,
                });
            }
        }
        let candidates = Arc::from(candidates);
        runtime.candidate_cache = Some(TransparentModelCandidateCache {
            address: address.clone(),
            candidates: Arc::clone(&candidates),
        });
        candidates
    };
    runtime.next_generation = runtime.next_generation.wrapping_add(1).max(1);
    let generation = ViewSortGeneration(runtime.next_generation);
    runtime.requested = Some((generation, key.clone()));
    runtime.committed = None;
    let work = TransparentModelSortWork {
        generation,
        key,
        view_from_world: Mat4::from(view.world_from_view.affine().inverse()),
        candidates,
    };
    let (start, _) = runtime.gate.submit_with_replacement(generation, work);
    if let Some((_generation, work)) = start {
        spawn_transparent_model_sort(runtime.result_sender.clone(), work);
    }
}
