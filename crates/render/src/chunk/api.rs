use crate::chunk::*;

pub(in crate::chunk) const DEFAULT_ACKNOWLEDGEMENT_CAPACITY: usize = 256;
pub(in crate::chunk) const DEFAULT_PRESENTED_FRAME_ACK_CAPACITY: usize = 8;

/// Maximum number of non-empty new or changed sub-chunks transferred to the
/// render world in one main-world update.
#[derive(Resource, ExtractResource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkUploadBudget {
    pub max_per_frame: usize,
    pub max_bytes_per_frame: u64,
}

impl Default for ChunkUploadBudget {
    fn default() -> Self {
        Self::new(32, 8 * 1024 * 1024)
    }
}

impl ChunkUploadBudget {
    #[must_use]
    pub const fn new(max_per_frame: usize, max_bytes_per_frame: u64) -> Self {
        Self {
            max_per_frame,
            max_bytes_per_frame,
        }
    }

    #[must_use]
    pub const fn can_fit(
        self,
        used_items: usize,
        used_bytes: u64,
        additional_items: usize,
        additional_bytes: u64,
    ) -> bool {
        used_items.saturating_add(additional_items) <= self.max_per_frame
            && used_bytes.saturating_add(additional_bytes) <= self.max_bytes_per_frame
    }
}

/// Sort key used by [`ChunkRenderQueue`] when an upload budget is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkUploadPriority(pub(in crate::chunk) f32);

impl ChunkUploadPriority {
    #[must_use]
    pub fn new(distance_squared: f32) -> Self {
        Self(if distance_squared.is_finite() {
            distance_squared.max(0.0)
        } else {
            f32::INFINITY
        })
    }

    /// Computes a nearest-first priority from a camera position and a
    /// sub-chunk's world-space center.
    #[must_use]
    pub fn from_camera(key: SubChunkKey, camera_position: Vec3) -> Self {
        let [x, y, z] = chunk_origin(key);
        let center = Vec3::new(x as f32 + 8.0, y as f32 + 8.0, z as f32 + 8.0);
        Self::new(center.distance_squared(camera_position))
    }

    #[must_use]
    pub const fn distance_squared(self) -> f32 {
        self.0
    }
}

impl PartialOrd for ChunkUploadPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.0.total_cmp(&other.0))
    }
}

pub(in crate::chunk) struct PendingUpload {
    pub(in crate::chunk) mesh: ChunkMesh,
    pub(in crate::chunk) biome: PackedBiomeRecord,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) priority: ChunkUploadPriority,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) token: Option<ChunkUploadToken>,
}

pub(in crate::chunk) struct PendingRemoval {
    pub(in crate::chunk) priority: ChunkUploadPriority,
    pub(in crate::chunk) token: Option<ChunkUploadToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkUploadToken {
    pub generation: u64,
    pub dirty_since: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkUploadAcknowledgement {
    pub key: SubChunkKey,
    pub token: ChunkUploadToken,
    pub applied_at: Instant,
    pub uploaded_bytes: u64,
}

/// Horizontal view identity attached to render-frame evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderViewCohort {
    pub dimension: i32,
    pub center: [i32; 2],
    pub radius: i32,
}

impl RenderViewCohort {
    #[must_use]
    pub const fn new(dimension: i32, center: [i32; 2], radius: i32) -> Self {
        Self {
            dimension,
            center,
            radius,
        }
    }

    #[must_use]
    pub fn contains(self, key: SubChunkKey) -> bool {
        key.dimension == self.dimension
            && i64::from(key.x).abs_diff(i64::from(self.center[0])) <= self.radius.max(0) as u64
            && i64::from(key.z).abs_diff(i64::from(self.center[1])) <= self.radius.max(0) as u64
    }
}

/// Independently frozen main-world target for one render view generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRenderExpectation {
    pub cohort: RenderViewCohort,
    pub source_cohort: Option<RenderViewCohort>,
    /// Optional exact target scope. Keys outside it are retained presentation,
    /// never required, source, or foreign proof for this expectation.
    pub target_keys: Option<Arc<[SubChunkKey]>>,
    pub manifest: Arc<[(SubChunkKey, u64)]>,
    pub view_generation: u64,
    pub render_ready_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct ModelWitnessFrameEvaluation {
    pub(in crate::chunk) revision: u64,
    pub(in crate::chunk) request_hash: [u8; 32],
    pub(in crate::chunk) total_model_ref_count: usize,
    pub(in crate::chunk) manifest: Arc<[ModelWitnessManifestRecord]>,
    pub(in crate::chunk) missing_key_count: usize,
    pub(in crate::chunk) stale_generation_count: usize,
    pub(in crate::chunk) wrong_stream_count: usize,
    pub(in crate::chunk) zero_model_ref_count: usize,
    pub(in crate::chunk) draw_mismatch_count: usize,
}

impl ModelWitnessFrameEvaluation {
    #[cfg(test)]
    pub(in crate::chunk) fn is_exact(&self) -> bool {
        !self.manifest.is_empty()
            && self.total_model_ref_count != 0
            && self.missing_key_count == 0
            && self.stale_generation_count == 0
            && self.wrong_stream_count == 0
            && self.zero_model_ref_count == 0
            && self.draw_mismatch_count == 0
    }
}

pub(in crate::chunk) fn evaluate_model_witness_frame(
    request: &ModelWitnessRequest,
    _frame_sequence: u64,
    _view_generation: u64,
    expected: &[(SubChunkKey, u64)],
    allocations: &[(SubChunkKey, u64, ChunkStreamMask, usize)],
    drawn: &[(SubChunkKey, u64, ChunkStreamMask)],
) -> ModelWitnessFrameEvaluation {
    let expected = expected.iter().copied().collect::<BTreeMap<_, _>>();
    let allocations = allocations
        .iter()
        .copied()
        .map(|(key, generation, streams, model_ref_count)| {
            ((key, generation), (streams, model_ref_count))
        })
        .collect::<BTreeMap<_, _>>();
    let drawn = drawn
        .iter()
        .copied()
        .map(|(key, generation, streams)| ((key, generation), streams))
        .collect::<BTreeMap<_, _>>();
    let mut manifest = Vec::with_capacity(request.keys().len());
    let mut missing_key_count = 0;
    let mut stale_generation_count = 0;
    let mut wrong_stream_count = 0;
    let mut zero_model_ref_count = 0;
    let mut draw_mismatch_count = 0;

    for &key in request.keys() {
        let Some(&generation) = expected.get(&key) else {
            missing_key_count += 1;
            continue;
        };
        let Some(&(streams, model_ref_count)) = allocations.get(&(key, generation)) else {
            if allocations
                .keys()
                .any(|(allocation_key, allocation_generation)| {
                    *allocation_key == key && *allocation_generation != generation
                })
            {
                stale_generation_count += 1;
            } else {
                missing_key_count += 1;
            }
            continue;
        };
        if !streams.contains(ChunkStreamMask::MODEL) {
            wrong_stream_count += 1;
            continue;
        }
        if model_ref_count == 0 {
            zero_model_ref_count += 1;
            continue;
        }
        if !drawn
            .get(&(key, generation))
            .is_some_and(|streams| streams.contains(ChunkStreamMask::MODEL))
        {
            draw_mismatch_count += 1;
            continue;
        }
        manifest.push(ModelWitnessManifestRecord {
            key,
            generation,
            model_ref_count,
        });
    }
    let total_model_ref_count = manifest.iter().map(|record| record.model_ref_count).sum();
    ModelWitnessFrameEvaluation {
        revision: request.revision,
        request_hash: request.request_hash,
        total_model_ref_count,
        manifest: Arc::from(manifest),
        missing_key_count,
        stale_generation_count,
        wrong_stream_count,
        zero_model_ref_count,
        draw_mismatch_count,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct CompletedFrameProbe {
    pub(in crate::chunk) expectation: TargetRenderExpectation,
    pub(in crate::chunk) frame_sequence: u64,
    pub(in crate::chunk) allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    pub(in crate::chunk) visible_allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    pub(in crate::chunk) drawn_manifest: Arc<[(SubChunkKey, u64)]>,
    pub(in crate::chunk) missing_target_instances: usize,
    pub(in crate::chunk) unexpected_target_instances: usize,
    pub(in crate::chunk) source_instances: usize,
    pub(in crate::chunk) foreign_instances: usize,
    pub(in crate::chunk) stale_generation_instances: usize,
    pub(in crate::chunk) orphan_allocations: usize,
    pub(in crate::chunk) transparent_sort_generation: u64,
    pub(in crate::chunk) model_witness: Option<ModelWitnessFrameEvaluation>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct FrameCompletionEvidence {
    pub(in crate::chunk) present_returned_at: Option<Instant>,
    pub(in crate::chunk) submitted_work_done_at: Option<Instant>,
}

/// Exact frame evidence published only after present returns and the sentinel
/// submission's GPU-completion callback runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedFrameAck {
    pub cohort: RenderViewCohort,
    pub frame_sequence: u64,
    /// Every target GPU allocation observed before `PrepareResources`.
    pub allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    /// Eligible target allocations extracted as visible for at least one queued view.
    pub visible_allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    /// Target allocations actually emitted by the direct or MDI draw path.
    pub drawn_manifest: Arc<[(SubChunkKey, u64)]>,
    pub view_generation: u64,
    pub render_ready_at: Instant,
    pub present_returned_at: Instant,
    pub gpu_completed_at: Instant,
    pub missing_target_instances: usize,
    pub unexpected_target_instances: usize,
    pub source_instances: usize,
    pub foreign_instances: usize,
    pub stale_generation_instances: usize,
    pub orphan_allocations: usize,
    pub transparent_sort_generation: u64,
    pub model_witness: Option<ModelWitnessFrameAck>,
}

impl PresentedFrameAck {
    #[must_use]
    pub(in crate::chunk) fn is_model_witness_compatible(&self) -> bool {
        self.visible_allocation_manifest == self.drawn_manifest
            && self.missing_target_instances == 0
            && self.unexpected_target_instances == 0
            && self.source_instances == 0
            && self.foreign_instances == 0
            && self.stale_generation_instances == 0
            && self.orphan_allocations == 0
    }

    #[must_use]
    pub fn is_exact(&self) -> bool {
        !self.allocation_manifest.is_empty()
            && self.drawn_manifest == self.visible_allocation_manifest
            && self.missing_target_instances == 0
            && self.unexpected_target_instances == 0
            && self.source_instances == 0
            && self.foreign_instances == 0
            && self.stale_generation_instances == 0
            && self.orphan_allocations == 0
    }

    #[must_use]
    pub fn forms_stable_exact_pair_with(&self, next: &Self) -> bool {
        self.is_exact()
            && next.is_exact()
            && self.cohort == next.cohort
            && self.allocation_manifest == next.allocation_manifest
            && self.visible_allocation_manifest == next.visible_allocation_manifest
            && self.view_generation == next.view_generation
            && self.render_ready_at == next.render_ready_at
            && self.transparent_sort_generation == next.transparent_sort_generation
            && self.frame_sequence.checked_add(1) == Some(next.frame_sequence)
            && self.gpu_completed_at <= next.gpu_completed_at
    }
}

#[derive(Default)]
pub(in crate::chunk) struct PresentedFrameGateState {
    pub(in crate::chunk) expectation: Option<TargetRenderExpectation>,
    pub(in crate::chunk) acknowledgements: VecDeque<PresentedFrameAck>,
    pub(in crate::chunk) in_flight_callbacks: usize,
}

/// Shared main/render-world target and bounded GPU-completed frame evidence.
#[derive(Resource, Clone)]
pub struct PresentedFrameGate {
    pub(in crate::chunk) inner: Arc<Mutex<PresentedFrameGateState>>,
}

impl Default for PresentedFrameGate {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PresentedFrameGateState::default())),
        }
    }
}

impl PresentedFrameGate {
    pub fn set_expectation(&self, expectation: TargetRenderExpectation) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.expectation.as_ref() != Some(&expectation) {
            state.acknowledgements.clear();
        }
        state.expectation = Some(expectation);
    }

    pub fn clear(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.expectation = None;
        state.acknowledgements.clear();
    }

    #[must_use]
    pub fn expectation(&self) -> Option<TargetRenderExpectation> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .expectation
            .clone()
    }

    #[must_use]
    pub fn drain(&self) -> Vec<PresentedFrameAck> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .acknowledgements
            .drain(..)
            .collect()
    }

    pub(in crate::chunk) fn try_reserve_callback(
        &self,
        expectation: &TargetRenderExpectation,
    ) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.expectation.as_ref() != Some(expectation)
            || state.in_flight_callbacks >= DEFAULT_PRESENTED_FRAME_ACK_CAPACITY
        {
            return false;
        }
        state.in_flight_callbacks += 1;
        true
    }

    pub(in crate::chunk) fn publish_reserved_probe(
        &self,
        probe: CompletedFrameProbe,
        present_returned_at: Instant,
        gpu_completed_at: Instant,
    ) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight_callbacks == 0 {
            return false;
        }
        state.in_flight_callbacks -= 1;
        if state.expectation.as_ref() != Some(&probe.expectation) {
            return false;
        }
        let Some(acknowledgement) = build_presented_frame_ack(
            probe,
            FrameCompletionEvidence {
                present_returned_at: Some(present_returned_at),
                submitted_work_done_at: Some(gpu_completed_at),
            },
        ) else {
            return false;
        };
        if state.acknowledgements.len() >= DEFAULT_PRESENTED_FRAME_ACK_CAPACITY {
            state.acknowledgements.pop_front();
        }
        state.acknowledgements.push_back(acknowledgement);
        true
    }
}

pub(in crate::chunk) enum AcknowledgementSlot {
    Reserved {
        token: ChunkUploadToken,
        prior_ready: Option<ChunkUploadAcknowledgement>,
    },
    Ready(ChunkUploadAcknowledgement),
}

pub(in crate::chunk) struct AcknowledgementState {
    pub(in crate::chunk) capacity: usize,
    pub(in crate::chunk) slots: HashMap<SubChunkKey, AcknowledgementSlot>,
}

#[derive(Resource, Clone)]
pub struct ChunkUploadAcknowledgements {
    pub(in crate::chunk) inner: Arc<Mutex<AcknowledgementState>>,
}

impl Default for ChunkUploadAcknowledgements {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_ACKNOWLEDGEMENT_CAPACITY)
    }
}

impl ChunkUploadAcknowledgements {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AcknowledgementState {
                capacity,
                slots: HashMap::with_capacity(capacity),
            })),
        }
    }

    #[must_use]
    pub fn drain(&self) -> Vec<ChunkUploadAcknowledgement> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let ready = state
            .slots
            .iter()
            .filter_map(|(&key, slot)| matches!(slot, AcknowledgementSlot::Ready(_)).then_some(key))
            .collect::<Vec<_>>();
        ready
            .into_iter()
            .filter_map(|key| match state.slots.remove(&key) {
                Some(AcknowledgementSlot::Ready(acknowledgement)) => Some(acknowledgement),
                Some(AcknowledgementSlot::Reserved { .. }) | None => None,
            })
            .collect()
    }

    pub fn clear(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.slots.clear();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.slots.is_empty()
    }

    pub(in crate::chunk) fn try_reserve(&self, key: SubChunkKey, token: ChunkUploadToken) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let at_capacity = state.slots.len() >= state.capacity;
        match state.slots.entry(key) {
            Entry::Occupied(mut entry) => {
                let prior_ready = match entry.get() {
                    AcknowledgementSlot::Reserved { prior_ready, .. } => *prior_ready,
                    AcknowledgementSlot::Ready(acknowledgement) => Some(*acknowledgement),
                };
                entry.insert(AcknowledgementSlot::Reserved { token, prior_ready });
                true
            }
            Entry::Vacant(entry) if !at_capacity => {
                entry.insert(AcknowledgementSlot::Reserved {
                    token,
                    prior_ready: None,
                });
                true
            }
            Entry::Vacant(_) => false,
        }
    }

    pub(in crate::chunk) fn cancel(&self, key: SubChunkKey, token: ChunkUploadToken) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Entry::Occupied(mut entry) = state.slots.entry(key) else {
            return false;
        };
        let AcknowledgementSlot::Reserved {
            token: reserved,
            prior_ready,
        } = entry.get()
        else {
            return false;
        };
        if *reserved != token {
            return false;
        }
        if let Some(acknowledgement) = *prior_ready {
            entry.insert(AcknowledgementSlot::Ready(acknowledgement));
        } else {
            entry.remove();
        }
        true
    }

    pub(in crate::chunk) fn complete(
        &self,
        key: SubChunkKey,
        token: ChunkUploadToken,
        applied_at: Instant,
    ) -> bool {
        self.complete_with_bytes(key, token, applied_at, 0)
    }

    pub(in crate::chunk) fn complete_with_bytes(
        &self,
        key: SubChunkKey,
        token: ChunkUploadToken,
        applied_at: Instant,
        uploaded_bytes: u64,
    ) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(AcknowledgementSlot::Reserved {
            token: reserved,
            prior_ready,
        }) = state.slots.get(&key)
        else {
            return false;
        };
        if *reserved != token {
            return false;
        }
        let uploaded_bytes = prior_ready
            .map_or(0, |acknowledgement| acknowledgement.uploaded_bytes)
            .saturating_add(uploaded_bytes);
        state.slots.insert(
            key,
            AcknowledgementSlot::Ready(ChunkUploadAcknowledgement {
                key,
                token,
                applied_at,
                uploaded_bytes,
            }),
        );
        true
    }

    #[cfg(test)]
    pub(in crate::chunk) fn record(&self, acknowledgement: ChunkUploadAcknowledgement) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !state.slots.contains_key(&acknowledgement.key) && state.slots.len() >= state.capacity {
            return false;
        }
        state.slots.insert(
            acknowledgement.key,
            AcknowledgementSlot::Ready(acknowledgement),
        );
        true
    }
}

/// Extracted packed geometry for one visible, frustum-cullable sub-chunk.
#[derive(Component, Clone, ExtractComponent)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<ChunkRenderInstance>)]
pub struct ChunkRenderInstance {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) cube_quads: Arc<[PackedQuad]>,
    pub(in crate::chunk) cube_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) model_refs: Arc<[PackedModelRef]>,
    pub(in crate::chunk) model_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub(in crate::chunk) transparent_model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub(in crate::chunk) liquid_quads: Arc<[PackedLiquidQuad]>,
    pub(in crate::chunk) liquid_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) has_depth_liquid: bool,
    pub(in crate::chunk) has_transparent_liquid: bool,
    pub(in crate::chunk) depth_liquid_start: Option<u32>,
    pub(in crate::chunk) biome: PackedBiomeRecord,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) token: Option<ChunkUploadToken>,
    pub(in crate::chunk) origin: [i32; 3],
}

impl ChunkRenderInstance {
    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        self.key
    }

    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.cube_quads.len()
    }

    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        &self.cube_quads
    }

    /// CPU-retained cube lighting sidecars consumed through the shared GPU
    /// geometry arena without changing this extraction contract.
    #[must_use]
    pub fn cube_lighting(&self) -> &[PackedQuadLighting] {
        &self.cube_lighting
    }

    #[must_use]
    pub fn model_refs(&self) -> &[PackedModelRef] {
        &self.model_refs
    }

    #[must_use]
    pub fn model_lighting(&self) -> &[PackedQuadLighting] {
        &self.model_lighting
    }

    #[must_use]
    pub fn model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.model_draw_refs
    }

    #[must_use]
    pub fn transparent_model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.transparent_model_draw_refs
    }

    #[must_use]
    pub fn liquid_quads(&self) -> &[PackedLiquidQuad] {
        &self.liquid_quads
    }

    #[must_use]
    pub fn liquid_lighting(&self) -> &[PackedQuadLighting] {
        &self.liquid_lighting
    }

    #[must_use]
    pub const fn biome_record(&self) -> &PackedBiomeRecord {
        &self.biome
    }

    #[must_use]
    pub const fn tint_revision(&self) -> u64 {
        self.tint_identity.revision()
    }

    #[must_use]
    pub const fn tint_identity(&self) -> ChunkBiomeTintIdentity {
        self.tint_identity
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}
