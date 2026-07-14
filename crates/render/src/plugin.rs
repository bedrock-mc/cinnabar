use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque, hash_map::Entry},
    ops::Range,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};

use assets::{
    ANIMATION_FLAG_BLEND, Animation, Material, ModelTemplate, NO_ANIMATION, ResolvedBiomeTints,
    RuntimeAssets, TextureArray, TextureMip, TextureRef,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    camera::{
        primitives::Aabb,
        visibility::{self, VisibilityClass},
    },
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::Mesh,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
            TrackedRenderPass, ViewBinnedRenderPhases, ViewRangefinder3d, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferDescriptor, BufferId, BufferInitDescriptor, BufferUsages,
            Canonical, ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, DownlevelFlags, DrawIndexedIndirectArgs, Extent3d, Face as CullFace,
            FilterMode, FragmentState, IndexFormat, Origin3d, PipelineCache, PollType,
            PrimitiveState, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState, WgpuFeatures,
        },
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        settings::Backends,
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
};
use world::SubChunkKey;

use crate::{
    AtmosphereFrame, ChunkMesh, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting,
    atmosphere_render::{AtmosphereGpu, install_atmosphere},
};

const CHUNK_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b5664c91-763f-4e5c-9310-d12659f70cd4");
const MODEL_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2cd46297-17aa-4c18-bfb1-83373bf39475");
const LIQUID_SHADER_HANDLE: Handle<Shader> = uuid_handle!("52e731aa-0a4d-4b07-9d66-80eb7688398f");
const LIGHTING_SHADER_HANDLE: Handle<Shader> = uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
const STATIC_QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
const MODEL_INDEX_COUNT: u32 = 6;
const MODEL_TEMPLATE_BINDING_BUDGET: u32 = 8;
const MODEL_VERTEX_STORAGE_BINDINGS: u32 = 8;
const _: () = assert!(MODEL_VERTEX_STORAGE_BINDINGS <= MODEL_TEMPLATE_BINDING_BUDGET);
const PACKED_QUAD_BYTES: u64 = 8;
const PACKED_MODEL_REF_BYTES: u64 = 16;
const PACKED_MODEL_DRAW_REF_BYTES: u64 = 8;
const PACKED_QUAD_LIGHTING_BYTES: u64 = 8;
const PACKED_LIQUID_QUAD_BYTES: u64 = 16;
const GEOMETRY_STREAM_WORD_BYTES: u64 = 4;
const CHUNK_ORIGIN_BYTES: u64 = 32;
const BIOME_WORD_BYTES: u64 = 4;
const FALLBACK_BIOME_WORDS: usize = 2;
const FALLBACK_BIOME_RECORD: [u32; FALLBACK_BIOME_WORDS] = [1 << 8, 0];
const INDEXED_INDIRECT_BYTES: u64 = 20;
const DEFAULT_RENDER_QUEUE_ITEMS: usize = 256;
const DEFAULT_RENDER_QUEUE_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_ACKNOWLEDGEMENT_CAPACITY: usize = DEFAULT_RENDER_QUEUE_ITEMS;
const DEFAULT_PRESENTED_FRAME_ACK_CAPACITY: usize = 8;

/// Hard 16 MiB ceiling for one committed transparent indirection snapshot.
pub const MAX_TRANSPARENT_DRAW_REFS: usize = 2_097_152;
pub const MAX_TRANSPARENT_VIEWS: usize = 1;
pub const TRANSPARENT_REF_SLOT_BYTES: usize =
    MAX_TRANSPARENT_DRAW_REFS * std::mem::size_of::<PackedTransparentDrawRef>();
pub const TRANSPARENT_REF_BUFFER_BYTES: usize = TRANSPARENT_REF_SLOT_BYTES * 2;
pub const DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME: usize = 131_072;
pub const MAX_TRANSPARENT_WITNESS_KEYS: usize = 64;
pub const MAX_MODEL_WITNESS_KEYS: usize = 64;
const MAX_TRANSPARENT_RETIRED_ALLOCATIONS: usize = 16_384;
const MAX_TRANSPARENT_RETIRED_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentDrawArgs {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransparentSortCandidate {
    key: SubChunkKey,
    local_quad_index: u32,
    liquid_record_index: u32,
    metadata_index: u32,
    subchunk_center: [f32; 3],
    quad_centroid: [f32; 3],
}

impl TransparentSortCandidate {
    #[must_use]
    pub const fn new(
        key: SubChunkKey,
        local_quad_index: u32,
        liquid_record_index: u32,
        metadata_index: u32,
        subchunk_center: [f32; 3],
        quad_centroid: [f32; 3],
    ) -> Self {
        Self {
            key,
            local_quad_index,
            liquid_record_index,
            metadata_index,
            subchunk_center,
            quad_centroid,
        }
    }
}

fn sort_transparent_candidates(
    view_from_world: Mat4,
    candidates: Arc<[TransparentSortCandidate]>,
) -> Vec<PackedTransparentDrawRef> {
    let mut grouped = BTreeMap::<SubChunkKey, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        grouped.entry(candidate.key).or_default().push(index);
    }
    let mut groups = grouped.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(left_key, left), (right_key, right)| {
        let left_center = Vec3::from_array(candidates[left[0]].subchunk_center);
        let right_center = Vec3::from_array(candidates[right[0]].subchunk_center);
        view_from_world
            .transform_point3(left_center)
            .z
            .total_cmp(&view_from_world.transform_point3(right_center).z)
            .then_with(|| left_key.cmp(right_key))
    });
    let mut refs = Vec::new();
    for (_key, mut group) in groups {
        group.sort_by(|&left, &right| {
            let left = &candidates[left];
            let right = &candidates[right];
            view_from_world
                .transform_point3(Vec3::from_array(left.quad_centroid))
                .z
                .total_cmp(
                    &view_from_world
                        .transform_point3(Vec3::from_array(right.quad_centroid))
                        .z,
                )
                .then_with(|| left.key.cmp(&right.key))
                .then_with(|| left.local_quad_index.cmp(&right.local_quad_index))
        });
        refs.extend(group.into_iter().map(|index| {
            let candidate = &candidates[index];
            PackedTransparentDrawRef::new(candidate.liquid_record_index, candidate.metadata_index)
        }));
    }
    refs
}

#[doc(hidden)]
pub fn sort_transparent_candidates_for_test(
    view_from_world: Mat4,
    candidates: Vec<TransparentSortCandidate>,
) -> Vec<PackedTransparentDrawRef> {
    sort_transparent_candidates(view_from_world, Arc::from(candidates))
}

fn transparent_draw_args(buffer_slot: u8, ref_count: usize) -> Option<TransparentDrawArgs> {
    transparent_draw_range_args(buffer_slot, 0..u32::try_from(ref_count).ok()?)
}

fn transparent_draw_range_args(
    buffer_slot: u8,
    ref_range: Range<u32>,
) -> Option<TransparentDrawArgs> {
    if ref_range.start > ref_range.end
        || usize::try_from(ref_range.end).ok()? > MAX_TRANSPARENT_DRAW_REFS
    {
        return None;
    }
    let instance_count = ref_range.end - ref_range.start;
    let first_instance = u32::from(buffer_slot)
        .checked_mul(u32::try_from(MAX_TRANSPARENT_DRAW_REFS).ok()?)?
        .checked_add(ref_range.start)?;
    Some(TransparentDrawArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: 0,
        first_instance,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransparentLiquidPhaseGroup {
    key: SubChunkKey,
    ref_range: Range<u32>,
}

fn transparent_liquid_phase_groups(
    snapshot: &TransparentOrderedSnapshot,
) -> Option<Vec<TransparentLiquidPhaseGroup>> {
    let mut identities = BTreeMap::new();
    for identity in snapshot.key.visible_allocations.iter() {
        if !identity.liquid_range.start.is_multiple_of(4)
            || !identity.liquid_range.end.is_multiple_of(4)
            || identities
                .insert(identity.metadata_index, identity)
                .is_some()
        {
            return None;
        }
    }

    let mut groups = Vec::<TransparentLiquidPhaseGroup>::new();
    let mut closed_metadata = HashSet::new();
    for (index, draw_ref) in snapshot.refs().iter().copied().enumerate() {
        let identity = identities.get(&draw_ref.metadata_index())?;
        let record_range = identity.liquid_range.start / 4..identity.liquid_range.end / 4;
        if !record_range.contains(&draw_ref.liquid_record_index()) {
            return None;
        }
        let index = u32::try_from(index).ok()?;
        if let Some(group) = groups.last_mut()
            && group.key == identity.key
        {
            group.ref_range.end = index.checked_add(1)?;
            continue;
        }
        if !closed_metadata.insert(identity.metadata_index) {
            return None;
        }
        groups.push(TransparentLiquidPhaseGroup {
            key: identity.key,
            ref_range: index..index.checked_add(1)?,
        });
    }
    Some(groups)
}

fn transparent_indirect_args(
    snapshot: &TransparentOrderedSnapshot,
) -> Option<DrawIndexedIndirectArgs> {
    let args = transparent_draw_args(snapshot.buffer_slot(), snapshot.refs().len())?;
    Some(DrawIndexedIndirectArgs {
        index_count: args.index_count,
        instance_count: args.instance_count,
        first_index: args.first_index,
        base_vertex: args.base_vertex,
        first_instance: args.first_instance,
    })
}

#[doc(hidden)]
pub fn direct_transparent_draw_args_for_test(
    buffer_slot: u8,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_args(buffer_slot, ref_count)
}

#[doc(hidden)]
pub fn mdi_transparent_draw_args_for_test(
    buffer_slot: u8,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_args(buffer_slot, ref_count)
}

/// One absolute liquid-record/chunk-metadata pair in committed back-to-front order.
///
/// The liquid record carries its absolute lighting address in word 3. These
/// references belong to a committed per-view snapshot, never to `ChunkMesh`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedTransparentDrawRef {
    liquid_record_index: u32,
    metadata_index: u32,
}

impl PackedTransparentDrawRef {
    #[must_use]
    pub const fn new(liquid_record_index: u32, metadata_index: u32) -> Self {
        Self {
            liquid_record_index,
            metadata_index,
        }
    }

    #[must_use]
    pub const fn liquid_record_index(self) -> u32 {
        self.liquid_record_index
    }

    #[must_use]
    pub const fn metadata_index(self) -> u32 {
        self.metadata_index
    }
}

const _: () = assert!(std::mem::size_of::<PackedTransparentDrawRef>() == 8);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransparentSortMetricsSnapshot {
    pub request_generation: u64,
    pub result_generation: u64,
    pub committed_generation: u64,
    /// Generation whose draw command was encoded into a render pass.
    pub encoded_generation: u64,
    /// Generation proven by the submitted-work completion sentinel.
    pub presented_generation: u64,
    pub ref_count: usize,
    pub cpu_duration: std::time::Duration,
    pub request_to_commit_latency: std::time::Duration,
    pub staged_bytes: u64,
    /// Cumulative transparent ref bytes successfully written to the GPU.
    pub upload_bytes: u64,
    pub stale_reject_count: u64,
    pub ceiling_reject_count: u64,
    pub active_slot_age_frames: u64,
    pub transparent_water_distinct_tint_count: usize,
}

/// Cross-world metrics bridge shared by the main and render worlds.
#[derive(Resource, Debug, Clone, Default)]
pub struct TransparentSortMetrics(Arc<Mutex<TransparentSortMetricsSnapshot>>);

impl TransparentSortMetrics {
    #[must_use]
    pub fn snapshot(&self) -> TransparentSortMetricsSnapshot {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    fn update(&self, update: impl FnOnce(&mut TransparentSortMetricsSnapshot)) {
        update(&mut self.0.lock().unwrap_or_else(|poison| poison.into_inner()));
    }

    #[doc(hidden)]
    pub fn publish_for_test(&self, snapshot: TransparentSortMetricsSnapshot) {
        self.update(|current| *current = snapshot);
    }
}

/// Exact model workload for one allocation cohort.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadCount {
    pub model_ref_count: usize,
    pub model_draw_ref_count: usize,
    /// Quad vertex-shader invocations avoided relative to the former fixed
    /// 32-quad slot issued for every model ref.
    pub legacy_fixed_slot_quad_invocations_avoided: usize,
}

/// Current resident and frustum-visible model workload published by the
/// render world for acceptance telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadMetricsSnapshot {
    pub resident: ModelWorkloadCount,
    pub visible: ModelWorkloadCount,
}

/// Cross-world bridge for exact model workload telemetry.
#[derive(Resource, Debug, Clone, Default)]
pub struct ModelWorkloadMetrics(Arc<Mutex<ModelWorkloadMetricsSnapshot>>);

impl ModelWorkloadMetrics {
    #[must_use]
    pub fn snapshot(&self) -> ModelWorkloadMetricsSnapshot {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    fn begin_frame(&self, resident: ModelWorkloadCount) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) =
            ModelWorkloadMetricsSnapshot {
                resident,
                visible: ModelWorkloadCount::default(),
            };
    }

    fn record_visible(&self, visible: ModelWorkloadCount) {
        let mut snapshot = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        snapshot.visible.model_ref_count = snapshot
            .visible
            .model_ref_count
            .max(visible.model_ref_count);
        snapshot.visible.model_draw_ref_count = snapshot
            .visible
            .model_draw_ref_count
            .max(visible.model_draw_ref_count);
        snapshot.visible.legacy_fixed_slot_quad_invocations_avoided = snapshot
            .visible
            .legacy_fixed_slot_quad_invocations_avoided
            .max(visible.legacy_fixed_slot_quad_invocations_avoided);
    }

    #[doc(hidden)]
    pub fn publish_for_test(&self, snapshot: ModelWorkloadMetricsSnapshot) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) = snapshot;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransparentWitnessRequestError {
    InvalidRevision,
    Empty,
    TooMany,
    Duplicate,
}

#[derive(Resource, ExtractResource, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransparentWitnessRequest {
    revision: u64,
    keys: Arc<[SubChunkKey]>,
}

impl TransparentWitnessRequest {
    pub fn try_new(
        revision: u64,
        mut keys: Vec<SubChunkKey>,
    ) -> Result<Self, TransparentWitnessRequestError> {
        if revision == 0 {
            return Err(TransparentWitnessRequestError::InvalidRevision);
        }
        if keys.is_empty() {
            return Err(TransparentWitnessRequestError::Empty);
        }
        if keys.len() > MAX_TRANSPARENT_WITNESS_KEYS {
            return Err(TransparentWitnessRequestError::TooMany);
        }
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(TransparentWitnessRequestError::Duplicate);
        }
        Ok(Self {
            revision,
            keys: Arc::from(keys),
        })
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn keys(&self) -> &[SubChunkKey] {
        &self.keys
    }

    const fn enabled(&self) -> bool {
        self.revision != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelWitnessRequestError {
    InvalidRevision,
    InvalidHash,
    Empty,
    TooMany,
    Duplicate,
}

#[derive(Resource, ExtractResource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelWitnessRequest {
    revision: u64,
    request_hash: [u8; 32],
    keys: Arc<[SubChunkKey]>,
}

impl ModelWitnessRequest {
    pub fn try_new(
        revision: u64,
        request_hash: [u8; 32],
        mut keys: Vec<SubChunkKey>,
    ) -> Result<Self, ModelWitnessRequestError> {
        if revision == 0 {
            return Err(ModelWitnessRequestError::InvalidRevision);
        }
        if request_hash == [0; 32] {
            return Err(ModelWitnessRequestError::InvalidHash);
        }
        if keys.is_empty() {
            return Err(ModelWitnessRequestError::Empty);
        }
        if keys.len() > MAX_MODEL_WITNESS_KEYS {
            return Err(ModelWitnessRequestError::TooMany);
        }
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ModelWitnessRequestError::Duplicate);
        }
        Ok(Self {
            revision,
            request_hash,
            keys: Arc::from(keys),
        })
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn request_hash(&self) -> &[u8; 32] {
        &self.request_hash
    }

    #[must_use]
    pub fn keys(&self) -> &[SubChunkKey] {
        &self.keys
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.revision != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelWitnessManifestRecord {
    pub key: SubChunkKey,
    pub generation: u64,
    pub model_ref_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWitnessFrameAck {
    pub revision: u64,
    pub request_hash: [u8; 32],
    pub frame_sequence: u64,
    pub view_generation: u64,
    pub present_returned_at: Instant,
    pub gpu_completed_at: Instant,
    pub total_model_ref_count: usize,
    pub manifest: Arc<[ModelWitnessManifestRecord]>,
    pub missing_key_count: usize,
    pub stale_generation_count: usize,
    pub wrong_stream_count: usize,
    pub zero_model_ref_count: usize,
    pub draw_mismatch_count: usize,
}

impl ModelWitnessFrameAck {
    #[must_use]
    pub fn is_exact(&self) -> bool {
        !self.manifest.is_empty()
            && self.total_model_ref_count != 0
            && self.missing_key_count == 0
            && self.stale_generation_count == 0
            && self.wrong_stream_count == 0
            && self.zero_model_ref_count == 0
            && self.draw_mismatch_count == 0
    }

    #[must_use]
    pub fn forms_stable_exact_pair_with(&self, next: &Self) -> bool {
        self.is_exact()
            && next.is_exact()
            && self.revision == next.revision
            && self.request_hash == next.request_hash
            && self.view_generation == next.view_generation
            && self.total_model_ref_count == next.total_model_ref_count
            && self.manifest == next.manifest
            && self.frame_sequence.checked_add(1) == Some(next.frame_sequence)
            && self.present_returned_at <= next.present_returned_at
            && self.gpu_completed_at <= next.gpu_completed_at
            && self.present_returned_at <= self.gpu_completed_at
            && next.present_returned_at <= next.gpu_completed_at
    }

    #[cfg(test)]
    fn exact_for_test(
        revision: u64,
        request_hash: [u8; 32],
        frame_sequence: u64,
        view_generation: u64,
        manifest: Arc<[ModelWitnessManifestRecord]>,
        now: Instant,
    ) -> Self {
        let total_model_ref_count = manifest.iter().map(|record| record.model_ref_count).sum();
        Self {
            revision,
            request_hash,
            frame_sequence,
            view_generation,
            present_returned_at: now,
            gpu_completed_at: now,
            total_model_ref_count,
            manifest,
            missing_key_count: 0,
            stale_generation_count: 0,
            wrong_stream_count: 0,
            zero_model_ref_count: 0,
            draw_mismatch_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWitnessEvent {
    pub acknowledgement: ModelWitnessFrameAck,
    pub consecutive: u8,
}

#[derive(Debug, Default)]
struct ModelWitnessEvidenceState {
    active: ModelWitnessRequest,
    first: Option<ModelWitnessFrameAck>,
    complete: bool,
    events: VecDeque<ModelWitnessEvent>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct ModelWitnessEvidence(Arc<Mutex<ModelWitnessEvidenceState>>);

impl ModelWitnessEvidence {
    pub fn set_authoritative_request(&self, request: &ModelWitnessRequest) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active == *request {
            return;
        }
        state.active = request.clone();
        state.first = None;
        state.complete = false;
        state.events.clear();
    }

    pub fn observe_presented_frame(
        &self,
        request: &ModelWitnessRequest,
        acknowledgement: &PresentedFrameAck,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.complete || state.active != *request {
            return;
        }
        if !acknowledgement.is_model_witness_compatible() {
            state.first = None;
            return;
        }
        let Some(current) = acknowledgement.model_witness.as_ref().filter(|current| {
            current.revision == request.revision
                && current.request_hash == request.request_hash
                && current.manifest.len() == request.keys.len()
                && current.is_exact()
        }) else {
            state.first = None;
            return;
        };
        let Some(first) = state.first.take() else {
            state.first = Some(current.clone());
            return;
        };
        if !first.forms_stable_exact_pair_with(current) {
            state.first = Some(current.clone());
            return;
        }
        state.events.push_back(ModelWitnessEvent {
            acknowledgement: first,
            consecutive: 1,
        });
        state.events.push_back(ModelWitnessEvent {
            acknowledgement: current.clone(),
            consecutive: 2,
        });
        state.complete = true;
    }

    pub fn drain_events(&self) -> Vec<ModelWitnessEvent> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .events
            .drain(..)
            .collect()
    }

    #[must_use]
    pub fn is_complete_for(&self, request: &ModelWitnessRequest) -> bool {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.complete && state.active == *request
    }

    pub fn reset(&self) {
        self.set_authoritative_request(&ModelWitnessRequest::default());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentWitnessEvent {
    pub revision: u64,
    pub sequence: u64,
    pub generation: u64,
    pub key_count: usize,
    pub consecutive: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentWitnessIncompleteEvent {
    pub revision: u64,
    pub sequence: u64,
    pub generation: u64,
    pub missing_keys: Arc<[SubChunkKey]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentWitnessStageRecord {
    pub key: SubChunkKey,
    pub extracted_visible: bool,
    pub instance_present: bool,
    pub liquid_quad_count: usize,
    pub instance_generation: u64,
    pub allocation_present: bool,
    pub liquid_range_len: u32,
    pub lighting_range_len: u32,
    pub allocation_matches: bool,
    pub committed_member: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentWitnessStageEvent {
    pub revision: u64,
    pub committed_generation: u64,
    pub records: Arc<[TransparentWitnessStageRecord]>,
}

#[derive(Debug, Clone)]
struct TransparentWitnessToken {
    request: TransparentWitnessRequest,
    sequence: u64,
    generation: u64,
    missing_keys: Arc<[SubChunkKey]>,
}

#[derive(Debug, Default)]
struct TransparentWitnessEvidenceState {
    active: TransparentWitnessRequest,
    next_sequence: u64,
    in_flight: Option<u64>,
    consecutive: u8,
    events: VecDeque<TransparentWitnessEvent>,
    last_missing_keys: Arc<[SubChunkKey]>,
    incomplete_events: VecDeque<TransparentWitnessIncompleteEvent>,
    last_stage_records: Arc<[TransparentWitnessStageRecord]>,
    stage_event_count: u8,
    stage_events: VecDeque<TransparentWitnessStageEvent>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct TransparentWitnessEvidence(Arc<Mutex<TransparentWitnessEvidenceState>>);

impl TransparentWitnessEvidence {
    #[cfg(test)]
    fn try_reserve(
        &self,
        request: &TransparentWitnessRequest,
        generation: u64,
        complete: bool,
    ) -> Option<TransparentWitnessToken> {
        let missing = if complete {
            Vec::new()
        } else {
            request.keys().to_vec()
        };
        self.try_reserve_missing(request, generation, missing)
    }

    fn try_reserve_missing(
        &self,
        request: &TransparentWitnessRequest,
        generation: u64,
        missing_keys: Vec<SubChunkKey>,
    ) -> Option<TransparentWitnessToken> {
        if !request.enabled() {
            return None;
        }
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active != *request {
            return None;
        }
        if state.in_flight.is_some() || state.consecutive >= 2 {
            return None;
        }
        state.next_sequence = state.next_sequence.checked_add(1)?;
        let sequence = state.next_sequence;
        state.in_flight = Some(sequence);
        Some(TransparentWitnessToken {
            request: request.clone(),
            sequence,
            generation,
            missing_keys: missing_keys.into(),
        })
    }

    fn complete(&self, token: TransparentWitnessToken) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active != token.request || state.in_flight != Some(token.sequence) {
            return false;
        }
        state.in_flight = None;
        if !token.missing_keys.is_empty() {
            state.consecutive = 0;
            state.events.clear();
            if state.last_missing_keys != token.missing_keys {
                state.last_missing_keys = Arc::clone(&token.missing_keys);
                if state.incomplete_events.len() == 4 {
                    state.incomplete_events.pop_front();
                }
                state
                    .incomplete_events
                    .push_back(TransparentWitnessIncompleteEvent {
                        revision: token.request.revision,
                        sequence: token.sequence,
                        generation: token.generation,
                        missing_keys: token.missing_keys,
                    });
            }
            return true;
        }
        state.consecutive = state.consecutive.saturating_add(1).min(2);
        let event = TransparentWitnessEvent {
            revision: token.request.revision,
            sequence: token.sequence,
            generation: token.generation,
            key_count: token.request.keys.len(),
            consecutive: state.consecutive,
        };
        if state.events.len() == 4 {
            state.events.pop_front();
        }
        state.events.push_back(event);
        true
    }

    pub fn drain_events(&self) -> Vec<TransparentWitnessEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.events.drain(..).collect()
    }

    pub fn drain_incomplete_events(&self) -> Vec<TransparentWitnessIncompleteEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.incomplete_events.drain(..).collect()
    }

    fn record_stage_snapshot(
        &self,
        revision: u64,
        committed_generation: u64,
        mut records: Vec<TransparentWitnessStageRecord>,
    ) -> bool {
        records.sort_unstable_by_key(|record| record.key);
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if revision == 0
            || state.active.revision() != revision
            || records.len() != state.active.keys().len()
            || !records
                .iter()
                .zip(state.active.keys())
                .all(|(record, key)| record.key == *key)
            || state.last_stage_records.as_ref() == records.as_slice()
            || state.stage_event_count >= 8
        {
            return false;
        }
        state.last_stage_records = Arc::from(records);
        state.stage_event_count += 1;
        let records = Arc::clone(&state.last_stage_records);
        state.stage_events.push_back(TransparentWitnessStageEvent {
            revision,
            committed_generation,
            records,
        });
        true
    }

    pub fn drain_stage_events(&self) -> Vec<TransparentWitnessStageEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.stage_events.drain(..).collect()
    }

    pub fn set_authoritative_request(&self, request: &TransparentWitnessRequest) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active == *request {
            return;
        }
        state.active = request.clone();
        state.in_flight = None;
        state.consecutive = 0;
        state.events.clear();
        state.last_missing_keys = Arc::default();
        state.incomplete_events.clear();
        state.last_stage_records = Arc::default();
        state.stage_event_count = 0;
        state.stage_events.clear();
    }

    pub fn reset(&self) {
        self.set_authoritative_request(&TransparentWitnessRequest::default());
    }
}

fn record_encoded_transparent_generation(
    metrics: &TransparentSortMetrics,
    generation: ViewSortGeneration,
) {
    metrics.update(|snapshot| snapshot.encoded_generation = generation.get());
}

fn record_gpu_completed_transparent_generation(metrics: &TransparentSortMetrics, generation: u64) {
    metrics.update(|snapshot| {
        if generation != 0
            && snapshot.committed_generation == generation
            && snapshot.encoded_generation == generation
        {
            snapshot.presented_generation = snapshot.presented_generation.max(generation);
        }
    });
}

#[derive(Resource, Debug, Clone, Default)]
struct TransparentPresentationFence(Arc<Mutex<Option<u64>>>);

impl TransparentPresentationFence {
    fn try_reserve(&self, generation: u64) -> bool {
        let mut in_flight = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if generation == 0 || in_flight.is_some() {
            return false;
        }
        *in_flight = Some(generation);
        true
    }

    fn complete(&self, generation: u64) -> bool {
        let mut in_flight = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if *in_flight != Some(generation) {
            return false;
        }
        *in_flight = None;
        true
    }
}

#[derive(Debug, Default)]
struct TransparentRetirementFenceState {
    next_epoch: u64,
    in_flight: Option<u64>,
    completed_epoch: u64,
}

/// Independent queue-completion epoch for reclaiming retired arena addresses.
/// It deliberately does not use `ViewSortGeneration`: view resets and stale
/// sort callbacks must not make physical GPU memory reusable early.
#[derive(Resource, Debug, Clone, Default)]
struct TransparentRetirementFence(Arc<Mutex<TransparentRetirementFenceState>>);

impl TransparentRetirementFence {
    fn try_reserve(&self) -> Option<u64> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight.is_some() {
            return None;
        }
        state.next_epoch = state.next_epoch.checked_add(1)?;
        let epoch = state.next_epoch;
        state.in_flight = Some(epoch);
        Some(epoch)
    }

    fn complete(&self, epoch: u64) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight != Some(epoch) {
            return false;
        }
        state.in_flight = None;
        state.completed_epoch = state.completed_epoch.max(epoch);
        true
    }

    fn completed_epoch(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .completed_epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransparentRetirementBudget {
    max_items: usize,
    max_bytes: u64,
    items: usize,
    bytes: u64,
}

impl TransparentRetirementBudget {
    const fn with_limits(max_items: usize, max_bytes: u64) -> Self {
        Self {
            max_items,
            max_bytes,
            items: 0,
            bytes: 0,
        }
    }

    fn try_reserve(&mut self, items: usize, bytes: u64) -> bool {
        let Some(next_items) = self.items.checked_add(items) else {
            return false;
        };
        let Some(next_bytes) = self.bytes.checked_add(bytes) else {
            return false;
        };
        if next_items > self.max_items || next_bytes > self.max_bytes {
            return false;
        }
        self.items = next_items;
        self.bytes = next_bytes;
        true
    }

    fn can_reserve(self, items: usize, bytes: u64) -> bool {
        let mut next = self;
        next.try_reserve(items, bytes)
    }

    fn release(&mut self, items: usize, bytes: u64) {
        self.items = self.items.saturating_sub(items);
        self.bytes = self.bytes.saturating_sub(bytes);
    }

    #[cfg(test)]
    const fn items(self) -> usize {
        self.items
    }

    #[cfg(test)]
    const fn bytes(self) -> u64 {
        self.bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransparentSortError {
    ReferenceCeiling { requested: usize, ceiling: usize },
    ConflictingAllocation { key: SubChunkKey },
    InvalidCameraTransform,
}

pub const fn validate_transparent_sort_ref_count(
    requested: usize,
) -> Result<(), TransparentSortError> {
    if requested > MAX_TRANSPARENT_DRAW_REFS {
        Err(TransparentSortError::ReferenceCeiling {
            requested,
            ceiling: MAX_TRANSPARENT_DRAW_REFS,
        })
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewSortGeneration(u64);

impl ViewSortGeneration {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct TransparentSortJobGate<T> {
    in_flight: Option<ViewSortGeneration>,
    pending: Option<(ViewSortGeneration, T)>,
}

impl<T> Default for TransparentSortJobGate<T> {
    fn default() -> Self {
        Self {
            in_flight: None,
            pending: None,
        }
    }
}

impl<T> TransparentSortJobGate<T> {
    pub fn submit(
        &mut self,
        generation: ViewSortGeneration,
        payload: T,
    ) -> Option<(ViewSortGeneration, T)> {
        if self.in_flight.is_none() {
            self.in_flight = Some(generation);
            Some((generation, payload))
        } else {
            self.pending = Some((generation, payload));
            None
        }
    }

    fn submit_with_replacement(
        &mut self,
        generation: ViewSortGeneration,
        payload: T,
    ) -> (Option<(ViewSortGeneration, T)>, Option<ViewSortGeneration>) {
        if self.in_flight.is_none() {
            self.in_flight = Some(generation);
            (Some((generation, payload)), None)
        } else {
            let replaced = self
                .pending
                .replace((generation, payload))
                .map(|(generation, _)| generation);
            (None, replaced)
        }
    }

    pub fn complete(&mut self, generation: ViewSortGeneration) -> Option<(ViewSortGeneration, T)> {
        if self.in_flight != Some(generation) {
            return None;
        }
        self.in_flight = None;
        if let Some((next_generation, payload)) = self.pending.take() {
            self.in_flight = Some(next_generation);
            Some((next_generation, payload))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn in_flight_generation(&self) -> Option<ViewSortGeneration> {
        self.in_flight
    }

    #[must_use]
    pub fn pending_generation(&self) -> Option<ViewSortGeneration> {
        self.pending.as_ref().map(|(generation, _)| *generation)
    }

    fn contains_generation(&self, generation: ViewSortGeneration) -> bool {
        self.in_flight == Some(generation) || self.pending_generation() == Some(generation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransparentAllocationIdentity {
    key: SubChunkKey,
    mesh_generation: u64,
    liquid_range: Range<u32>,
    lighting_range: Range<u32>,
    metadata_index: u32,
}

impl TransparentAllocationIdentity {
    #[must_use]
    pub const fn new(
        key: SubChunkKey,
        mesh_generation: u64,
        liquid_range: Range<u32>,
        lighting_range: Range<u32>,
        metadata_index: u32,
    ) -> Self {
        Self {
            key,
            mesh_generation,
            liquid_range,
            lighting_range,
            metadata_index,
        }
    }

    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        self.key
    }

    fn canonical_tuple(&self) -> (SubChunkKey, u64, u32, u32, u32, u32, u32) {
        (
            self.key,
            self.mesh_generation,
            self.liquid_range.start,
            self.liquid_range.end,
            self.lighting_range.start,
            self.lighting_range.end,
            self.metadata_index,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSortKey {
    camera_position_bits: [u32; 3],
    camera_orientation_bits: [u32; 4],
    visible_allocations: Arc<[TransparentAllocationIdentity]>,
    asset_identity: ChunkTextureAssetIdentity,
    tint_identity: ChunkBiomeTintIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TransparentAddressIdentity {
    visible_allocations: Arc<[TransparentAllocationIdentity]>,
    asset_identity: ChunkTextureAssetIdentity,
    tint_identity: ChunkBiomeTintIdentity,
}

impl ViewSortKey {
    pub fn try_new(
        camera_position: [f32; 3],
        camera_orientation: [f32; 4],
        mut visible_allocations: Vec<TransparentAllocationIdentity>,
        asset_identity: ChunkTextureAssetIdentity,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Result<Self, TransparentSortError> {
        if !camera_position.into_iter().all(f32::is_finite)
            || !camera_orientation.into_iter().all(f32::is_finite)
        {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        let norm_squared = camera_orientation
            .into_iter()
            .map(|value| value * value)
            .sum::<f32>();
        if !norm_squared.is_finite() || norm_squared == 0.0 {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        let inverse_norm = norm_squared.sqrt().recip();
        let mut orientation = camera_orientation.map(|value| value * inverse_norm);
        let sign_anchor = [
            orientation[3],
            orientation[2],
            orientation[1],
            orientation[0],
        ]
        .into_iter()
        .find(|value| *value != 0.0)
        .unwrap_or(1.0);
        if sign_anchor.is_sign_negative() {
            orientation = orientation.map(|value| -value);
        }
        let canonical_bits = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
        visible_allocations.sort_by_key(TransparentAllocationIdentity::canonical_tuple);
        visible_allocations.dedup();
        for pair in visible_allocations.windows(2) {
            if pair[0].key == pair[1].key {
                return Err(TransparentSortError::ConflictingAllocation { key: pair[0].key });
            }
        }
        Ok(Self {
            camera_position_bits: camera_position.map(canonical_bits),
            camera_orientation_bits: orientation.map(canonical_bits),
            visible_allocations: Arc::from(visible_allocations),
            asset_identity,
            tint_identity,
        })
    }

    fn address_identity_eq(&self, other: &Self) -> bool {
        self.visible_allocations == other.visible_allocations
            && self.asset_identity == other.asset_identity
            && self.tint_identity == other.tint_identity
    }

    fn address_identity(&self) -> TransparentAddressIdentity {
        TransparentAddressIdentity {
            visible_allocations: Arc::clone(&self.visible_allocations),
            asset_identity: self.asset_identity,
            tint_identity: self.tint_identity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentSortResult {
    generation: ViewSortGeneration,
    key: ViewSortKey,
    refs: Box<[PackedTransparentDrawRef]>,
}

impl TransparentSortResult {
    pub fn new(
        generation: ViewSortGeneration,
        key: ViewSortKey,
        refs: Vec<PackedTransparentDrawRef>,
    ) -> Result<Self, TransparentSortError> {
        validate_transparent_sort_ref_count(refs.len())?;
        Ok(Self {
            generation,
            key,
            refs: refs.into_boxed_slice(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentOrderedSnapshot {
    generation: ViewSortGeneration,
    key: ViewSortKey,
    refs: Arc<[PackedTransparentDrawRef]>,
    buffer_slot: u8,
}

impl TransparentOrderedSnapshot {
    #[must_use]
    pub const fn generation(&self) -> ViewSortGeneration {
        self.generation
    }

    #[must_use]
    pub const fn key(&self) -> &ViewSortKey {
        &self.key
    }

    #[must_use]
    pub fn refs(&self) -> &[PackedTransparentDrawRef] {
        &self.refs
    }

    #[must_use]
    pub const fn buffer_slot(&self) -> u8 {
        self.buffer_slot
    }
}

#[derive(Debug)]
pub struct TransparentSortState {
    next_generation: u64,
    requested: Option<(ViewSortGeneration, ViewSortKey)>,
    committed: Option<TransparentOrderedSnapshot>,
    staged: Option<TransparentStagedSnapshot>,
    upload_cap: usize,
}

#[derive(Debug)]
struct TransparentStagedSnapshot {
    generation: ViewSortGeneration,
    key: ViewSortKey,
    refs: Arc<[PackedTransparentDrawRef]>,
    uploaded: usize,
    buffer_slot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentUploadBatch<'a> {
    buffer_slot: u8,
    ref_range: Range<usize>,
    refs: &'a [PackedTransparentDrawRef],
}

impl TransparentUploadBatch<'_> {
    #[must_use]
    pub const fn buffer_slot(&self) -> u8 {
        self.buffer_slot
    }

    #[must_use]
    pub fn ref_range(&self) -> Range<usize> {
        self.ref_range.clone()
    }

    #[must_use]
    pub const fn refs(&self) -> &[PackedTransparentDrawRef] {
        self.refs
    }
}

impl TransparentSortState {
    #[must_use]
    pub const fn with_upload_cap(upload_cap: usize) -> Self {
        Self {
            next_generation: 0,
            requested: None,
            committed: None,
            staged: None,
            upload_cap: if upload_cap == 0 { 1 } else { upload_cap },
        }
    }

    pub fn request(&mut self, key: &ViewSortKey) -> ViewSortGeneration {
        self.request_retaining_resident_snapshot(key, false)
    }

    fn request_retaining_resident_snapshot(
        &mut self,
        key: &ViewSortKey,
        committed_addresses_are_resident: bool,
    ) -> ViewSortGeneration {
        if let Some((generation, requested_key)) = &self.requested
            && requested_key == key
        {
            return *generation;
        }
        let address_identity_is_safe = self.committed.as_ref().is_none_or(|snapshot| {
            snapshot.key.address_identity_eq(key) || committed_addresses_are_resident
        });
        if !address_identity_is_safe {
            self.committed = None;
        }
        // Exact camera bits may change every frame. Finish a safe inactive-slot
        // upload before accepting another pose so bounded uploads cannot starve.
        if let Some(staged) = self
            .staged
            .as_ref()
            .filter(|snapshot| snapshot.key.address_identity_eq(key))
        {
            return staged.generation;
        }
        if self
            .staged
            .as_ref()
            .is_some_and(|snapshot| !snapshot.key.address_identity_eq(key))
        {
            self.staged = None;
        }
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = ViewSortGeneration(self.next_generation);
        self.requested = Some((generation, key.clone()));
        generation
    }

    pub fn complete(
        &mut self,
        result: TransparentSortResult,
    ) -> Result<bool, TransparentSortError> {
        if self
            .requested
            .as_ref()
            .is_none_or(|(generation, key)| *generation != result.generation || key != &result.key)
        {
            return Ok(false);
        }
        if let Some(committed) = self.committed.as_mut()
            && committed.key.address_identity_eq(&result.key)
            && committed.refs.as_ref() == result.refs.as_ref()
        {
            committed.generation = result.generation;
            committed.key = result.key;
            self.staged = None;
            return Ok(true);
        }
        let buffer_slot = self
            .committed
            .as_ref()
            .map_or(0, |snapshot| 1 - snapshot.buffer_slot);
        if result.refs.is_empty() {
            self.committed = Some(TransparentOrderedSnapshot {
                generation: result.generation,
                key: result.key,
                refs: Arc::from(result.refs),
                buffer_slot,
            });
            self.staged = None;
            return Ok(true);
        }
        self.staged = Some(TransparentStagedSnapshot {
            generation: result.generation,
            key: result.key,
            refs: Arc::from(result.refs),
            uploaded: 0,
            buffer_slot,
        });
        Ok(false)
    }

    #[must_use]
    pub fn next_upload_batch(&self) -> Option<TransparentUploadBatch<'_>> {
        let staged = self.staged.as_ref()?;
        let end = staged
            .uploaded
            .saturating_add(self.upload_cap)
            .min(staged.refs.len());
        (end > staged.uploaded).then(|| TransparentUploadBatch {
            buffer_slot: staged.buffer_slot,
            ref_range: staged.uploaded..end,
            refs: &staged.refs[staged.uploaded..end],
        })
    }

    /// Acknowledges that the batch returned by [`Self::next_upload_batch`] was
    /// written successfully. Returns true only when the inactive slot became
    /// complete and was atomically promoted to the committed snapshot.
    pub fn acknowledge_upload(&mut self) -> bool {
        let Some(staged) = self.staged.as_mut() else {
            return false;
        };
        let remaining = staged.refs.len().saturating_sub(staged.uploaded);
        let uploaded = remaining.min(self.upload_cap);
        if uploaded == 0 {
            return false;
        }
        staged.uploaded += uploaded;
        if staged.uploaded == staged.refs.len() {
            let staged = self.staged.take().expect("staged snapshot exists");
            self.committed = Some(TransparentOrderedSnapshot {
                generation: staged.generation,
                key: staged.key,
                refs: staged.refs,
                buffer_slot: staged.buffer_slot,
            });
            return true;
        }
        false
    }

    #[must_use]
    pub const fn committed(&self) -> Option<&TransparentOrderedSnapshot> {
        self.committed.as_ref()
    }

    #[must_use]
    pub fn staged_ref_count(&self) -> usize {
        self.staged
            .as_ref()
            .map_or(0, |snapshot| snapshot.refs.len())
    }

    fn staged_generation(&self) -> Option<ViewSortGeneration> {
        self.staged.as_ref().map(|snapshot| snapshot.generation)
    }

    pub fn reset_preserving_generation(&mut self) {
        self.requested = None;
        self.committed = None;
        self.staged = None;
    }
}

#[derive(Debug)]
struct TransparentSortRequest {
    generation: ViewSortGeneration,
    requested_at: Instant,
    key: ViewSortKey,
    view_from_world: Mat4,
}

#[derive(Debug)]
struct TransparentSortWork {
    generation: ViewSortGeneration,
    requested_at: Instant,
    key: ViewSortKey,
    view_from_world: Mat4,
    candidates: Arc<[TransparentSortCandidate]>,
    distinct_tint_count: usize,
}

#[derive(Debug, Clone)]
struct TransparentCandidateCache {
    address_identity: TransparentAddressIdentity,
    candidates: Arc<[TransparentSortCandidate]>,
    distinct_tint_count: usize,
}

#[derive(Debug)]
struct TransparentWorkerResult {
    generation: ViewSortGeneration,
    requested_at: Instant,
    key: ViewSortKey,
    refs: Result<Vec<PackedTransparentDrawRef>, TransparentSortError>,
    cpu_duration: Duration,
    distinct_tint_count: usize,
}

#[derive(Resource)]
struct TransparentSortRuntime {
    view_entity: Option<Entity>,
    state: TransparentSortState,
    gate: TransparentSortJobGate<TransparentSortWork>,
    result_sender: SyncSender<TransparentWorkerResult>,
    result_receiver: Mutex<Receiver<TransparentWorkerResult>>,
    requested_at: HashMap<ViewSortGeneration, Instant>,
    staged_distinct_tint_counts: HashMap<ViewSortGeneration, usize>,
    committed_distinct_tint_count: usize,
    last_indirect_identity: Option<(u8, usize)>,
    candidate_cache: Option<TransparentCandidateCache>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TransparentModelAllocationIdentity {
    entity: Entity,
    key: SubChunkKey,
    generation: u64,
    model_range: Range<u32>,
    draw_range: Range<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TransparentModelAddressIdentity {
    asset_identity: ChunkTextureAssetIdentity,
    allocations: Arc<[TransparentModelAllocationIdentity]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TransparentModelSortKey {
    view_entity: Entity,
    rotation_bits: [u32; 4],
    address: TransparentModelAddressIdentity,
}

#[derive(Debug, Clone)]
struct TransparentModelSortCandidate {
    entity: Entity,
    key: SubChunkKey,
    draw_range: Range<u32>,
    stable_index: u32,
    centroid: Vec3,
    words: [u32; 2],
}

#[derive(Debug, Clone)]
struct TransparentModelCandidateCache {
    address: TransparentModelAddressIdentity,
    candidates: Arc<[TransparentModelSortCandidate]>,
}

#[derive(Debug)]
struct TransparentModelSortWork {
    generation: ViewSortGeneration,
    key: TransparentModelSortKey,
    view_from_world: Mat4,
    candidates: Arc<[TransparentModelSortCandidate]>,
}

#[derive(Debug, Clone)]
struct TransparentModelSortBatch {
    draw_range: Range<u32>,
    words: Box<[[u32; 2]]>,
}

#[derive(Debug)]
struct TransparentModelWorkerResult {
    generation: ViewSortGeneration,
    key: TransparentModelSortKey,
    batches: Vec<TransparentModelSortBatch>,
}

#[derive(Debug)]
struct TransparentModelStagedSort {
    key: TransparentModelSortKey,
    batches: VecDeque<TransparentModelSortBatch>,
}

#[derive(Resource)]
struct TransparentModelSortRuntime {
    next_generation: u64,
    requested: Option<(ViewSortGeneration, TransparentModelSortKey)>,
    committed: Option<TransparentModelSortKey>,
    staged: Option<TransparentModelStagedSort>,
    gate: TransparentSortJobGate<TransparentModelSortWork>,
    result_sender: SyncSender<TransparentModelWorkerResult>,
    result_receiver: Mutex<Receiver<TransparentModelWorkerResult>>,
    candidate_cache: Option<TransparentModelCandidateCache>,
}

#[derive(Debug, Resource)]
struct TransparentUploadBudget {
    remaining_refs: usize,
}

impl Default for TransparentUploadBudget {
    fn default() -> Self {
        Self {
            remaining_refs: DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
        }
    }
}

impl TransparentUploadBudget {
    fn reset(&mut self) {
        self.remaining_refs = DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME;
    }

    const fn remaining(&self) -> usize {
        self.remaining_refs
    }

    fn consume(&mut self, refs: usize) -> bool {
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
    fn reset_for_view(&mut self, view_entity: Option<Entity>) {
        let next_generation = self.state.next_generation;
        *self = Self::default();
        self.state.next_generation = next_generation;
        self.view_entity = view_entity;
    }

    fn fail_closed_conflicting_manifest(&mut self, metrics: &TransparentSortMetrics) {
        let view_entity = self.view_entity;
        self.reset_for_view(view_entity);
        clear_active_transparent_metrics(metrics);
    }

    fn prune_request_metadata(&mut self) {
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

    fn generation_needs_sort_job(&self, generation: ViewSortGeneration) -> bool {
        self.state.staged_generation() != Some(generation)
            && !self.gate.contains_generation(generation)
    }

    fn resolve_candidate_cache(
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

fn transparent_request_to_commit_latency(requested_at: Instant, committed_at: Instant) -> Duration {
    committed_at.saturating_duration_since(requested_at)
}

fn clear_active_transparent_metrics(metrics: &TransparentSortMetrics) {
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

fn fail_closed_transparent_sort_key_error(
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

fn spawn_transparent_sort(sender: SyncSender<TransparentWorkerResult>, work: TransparentSortWork) {
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

pub const MATERIAL_UV_ROTATE_90: u32 = 1;
pub const MATERIAL_UV_ROTATE_180: u32 = 2;
pub const MATERIAL_UV_ROTATE_270: u32 = 3;
pub const MATERIAL_UV_REFLECT_U: u32 = 1 << 2;
pub const MATERIAL_UV_REFLECT_V: u32 = 1 << 3;
const MATERIAL_UV_ROTATION_MASK: u32 = 0b11;

/// Linear-space tint colours resolved from one live biome definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeTint {
    pub grass: [f32; 3],
    pub foliage: [f32; 3],
    pub birch: [f32; 3],
    pub evergreen: [f32; 3],
    pub dry_foliage: [f32; 3],
    pub water: [f32; 3],
    pub flags: u32,
}

impl Default for BiomeTint {
    fn default() -> Self {
        Self {
            grass: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            foliage: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            birch: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            evergreen: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            dry_foliage: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            water: [1.0; 3],
            flags: 0,
        }
    }
}

/// Immutable dense biome tint table. Palette-native chunk records reference
/// these entries by index; entry zero is always a deterministic fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ChunkBiomeTintIdentity {
    stream: u64,
    revision: u64,
}

impl ChunkBiomeTintIdentity {
    #[must_use]
    pub const fn new(stream: u64, revision: u64) -> Self {
        Self { stream, revision }
    }

    #[must_use]
    pub const fn stream(self) -> u64 {
        self.stream
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Resource, Clone)]
pub struct ChunkBiomeTints {
    entries: Arc<[BiomeTint]>,
    identity: ChunkBiomeTintIdentity,
}

impl Default for ChunkBiomeTints {
    fn default() -> Self {
        Self {
            entries: Arc::from([BiomeTint::default()]),
            identity: ChunkBiomeTintIdentity::default(),
        }
    }
}

impl ChunkBiomeTints {
    #[must_use]
    pub fn from_resolved(resolved: &ResolvedBiomeTints, revision: u64) -> Self {
        Self::from_resolved_with_identity(resolved, ChunkBiomeTintIdentity::new(0, revision))
    }

    #[must_use]
    pub fn from_resolved_with_identity(
        resolved: &ResolvedBiomeTints,
        identity: ChunkBiomeTintIdentity,
    ) -> Self {
        let entries = resolved
            .records
            .iter()
            .map(|record| BiomeTint {
                grass: record.grass[..3].try_into().expect("three grass channels"),
                foliage: record.foliage[..3]
                    .try_into()
                    .expect("three foliage channels"),
                birch: record.birch[..3].try_into().expect("three birch channels"),
                evergreen: record.evergreen[..3]
                    .try_into()
                    .expect("three evergreen channels"),
                dry_foliage: record.dry_foliage[..3]
                    .try_into()
                    .expect("three dry foliage channels"),
                water: record.water[..3].try_into().expect("three water channels"),
                flags: record.flags,
            })
            .collect::<Vec<_>>();
        Self::with_identity(Arc::from(entries), identity)
    }

    /// Replaces tint colours while retaining the dense index contract used by
    /// queued [`PackedBiomeRecord`] palettes. Callers that change index
    /// assignments must enqueue replacement records with the same revision.
    #[must_use]
    pub fn with_revision(entries: Arc<[BiomeTint]>, revision: u64) -> Self {
        Self::with_identity(entries, ChunkBiomeTintIdentity::new(0, revision))
    }

    #[must_use]
    pub fn with_identity(entries: Arc<[BiomeTint]>, identity: ChunkBiomeTintIdentity) -> Self {
        let entries = if entries.is_empty() {
            Arc::from([BiomeTint::default()])
        } else {
            entries
        };
        Self { entries, identity }
    }

    #[must_use]
    pub fn entries(&self) -> &[BiomeTint] {
        &self.entries
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.identity.revision()
    }

    #[must_use]
    pub const fn table_identity(&self) -> ChunkBiomeTintIdentity {
        self.identity
    }

    fn resource_identity(&self) -> ChunkBiomeTintResourceIdentity {
        ChunkBiomeTintResourceIdentity {
            pointer: Arc::as_ptr(&self.entries) as *const BiomeTint as usize,
            table: self.identity,
        }
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkBiomeTints {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkBiomeTintResourceIdentity {
    pointer: usize,
    table: ChunkBiomeTintIdentity,
}

/// Immutable assets selected for the single global chunk texture array.
#[derive(Resource, Clone)]
pub struct ChunkTextureAssets {
    assets: Arc<RuntimeAssets>,
    revision: u64,
}

impl Default for ChunkTextureAssets {
    fn default() -> Self {
        Self::new(Arc::new(RuntimeAssets::diagnostic()))
    }
}

impl ChunkTextureAssets {
    #[must_use]
    pub const fn new(assets: Arc<RuntimeAssets>) -> Self {
        Self {
            assets,
            revision: 0,
        }
    }

    #[must_use]
    pub const fn with_revision(assets: Arc<RuntimeAssets>, revision: u64) -> Self {
        Self { assets, revision }
    }

    #[must_use]
    pub fn assets(&self) -> &Arc<RuntimeAssets> {
        &self.assets
    }

    #[must_use]
    pub fn identity(&self) -> ChunkTextureAssetIdentity {
        ChunkTextureAssetIdentity {
            pointer: Arc::as_ptr(&self.assets) as usize,
            revision: self.revision,
        }
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkTextureAssets {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        Self {
            assets: Arc::clone(&source.assets),
            revision: source.revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkTextureAssetIdentity {
    pointer: usize,
    revision: u64,
}

impl ChunkTextureAssetIdentity {
    #[doc(hidden)]
    #[must_use]
    pub const fn for_test(pointer: usize, revision: u64) -> Self {
        Self { pointer, revision }
    }
}

#[must_use]
pub fn texture_asset_needs_rebuild(
    current: Option<ChunkTextureAssetIdentity>,
    next: ChunkTextureAssetIdentity,
) -> bool {
    current != Some(next)
}

const ANIMATION_TICKS_PER_SECOND: f64 = 20.0;
const ANIMATION_TICK_MODULUS: f64 = u32::MAX as f64 + 1.0;

/// Global Bedrock flipbook clock. Only this 16-byte value changes per frame;
/// texture pages and animation tables remain immutable for an asset revision.
#[repr(C)]
#[derive(
    Resource, Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, ShaderType,
)]
pub struct ChunkAnimationClock {
    tick: u32,
    partial_tick: f32,
    _padding_0: u32,
    _padding_1: u32,
}

const _: () = assert!(std::mem::size_of::<ChunkAnimationClock>() == 16);

impl Default for ChunkAnimationClock {
    fn default() -> Self {
        Self::from_parts(0, 0.0)
    }
}

impl ChunkAnimationClock {
    #[must_use]
    pub fn from_parts(tick: u32, partial_tick: f32) -> Self {
        Self {
            tick,
            partial_tick: if partial_tick.is_finite() {
                partial_tick.clamp(0.0, 0.999_999_94)
            } else {
                0.0
            },
            _padding_0: 0,
            _padding_1: 0,
        }
    }

    #[must_use]
    pub fn from_elapsed_seconds(elapsed_seconds: f64) -> Self {
        let elapsed_seconds = if elapsed_seconds.is_finite() {
            elapsed_seconds.max(0.0)
        } else {
            0.0
        };
        let elapsed_ticks = elapsed_seconds * ANIMATION_TICKS_PER_SECOND;
        let whole_ticks = elapsed_ticks.floor();
        Self::from_parts(
            whole_ticks.rem_euclid(ANIMATION_TICK_MODULUS) as u32,
            elapsed_ticks.fract() as f32,
        )
    }

    #[must_use]
    pub const fn tick(self) -> u32 {
        self.tick
    }

    #[must_use]
    pub const fn partial_tick(self) -> f32 {
        self.partial_tick
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkAnimationClock {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        *source
    }
}

/// Resolved current/next physical texture references for one material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationFrameSample {
    pub current: TextureRef,
    pub next: TextureRef,
    pub blend: f32,
}

impl AnimationFrameSample {
    #[must_use]
    pub const fn new(current: TextureRef, next: TextureRef, blend: f32) -> Self {
        Self {
            current,
            next,
            blend,
        }
    }
}

/// CPU oracle for the exact bounded frame-selection arithmetic used by WGSL.
#[must_use]
pub fn select_animation_frames(
    material: Material,
    animations: &[Animation],
    frames: &[TextureRef],
    clock: ChunkAnimationClock,
) -> AnimationFrameSample {
    let static_sample = || AnimationFrameSample::new(material.texture, material.texture, 0.0);
    if material.animation == NO_ANIMATION {
        return static_sample();
    }
    let Some(animation) = animations.get(material.animation as usize) else {
        return static_sample();
    };
    if animation.frame_count == 0 || animation.ticks_per_frame == 0 {
        return static_sample();
    }
    let current_index = (clock.tick / animation.ticks_per_frame) % animation.frame_count;
    let Some(current_offset) = animation.frame_start.checked_add(current_index) else {
        return static_sample();
    };
    let Some(&current) = frames.get(current_offset as usize) else {
        return static_sample();
    };
    if animation.flags & ANIMATION_FLAG_BLEND == 0 || animation.frame_count == 1 {
        return AnimationFrameSample::new(current, current, 0.0);
    }
    let next_index = (current_index + 1) % animation.frame_count;
    let Some(next_offset) = animation.frame_start.checked_add(next_index) else {
        return static_sample();
    };
    let Some(&next) = frames.get(next_offset as usize) else {
        return static_sample();
    };
    let blend = (clock.tick % animation.ticks_per_frame) as f32 + clock.partial_tick;
    AnimationFrameSample::new(current, next, blend / animation.ticks_per_frame as f32)
}

/// Source assigned to each of the two physical texture bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexturePageBinding {
    Asset(usize),
    DiagnosticFallback,
}

#[must_use]
pub const fn plan_texture_page_bindings(page_count: usize) -> Option<[TexturePageBinding; 2]> {
    match page_count {
        1 => Some([
            TexturePageBinding::Asset(0),
            TexturePageBinding::DiagnosticFallback,
        ]),
        2 => Some([TexturePageBinding::Asset(0), TexturePageBinding::Asset(1)]),
        _ => None,
    }
}

/// Copies physical layer zero from every mip into the real one-layer page bound
/// as page one when an asset contains only page zero.
pub fn diagnostic_texture_page(
    texture: &TextureArray,
) -> Result<TextureArray, TextureUploadPlanError> {
    if texture.layers == 0 {
        return Err(TextureUploadPlanError::InvalidMipBytes);
    }
    let mut mips = Vec::with_capacity(texture.mips.len());
    for mip in &texture.mips {
        let side = usize::try_from(mip.size).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
        let layer_bytes = side
            .checked_mul(side)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(TextureUploadPlanError::SizeOverflow)?;
        let expected = layer_bytes
            .checked_mul(texture.layers as usize)
            .ok_or(TextureUploadPlanError::SizeOverflow)?;
        if mip.rgba8.len() != expected {
            return Err(TextureUploadPlanError::InvalidMipBytes);
        }
        mips.push(TextureMip {
            size: mip.size,
            rgba8: mip.rgba8[..layer_bytes].to_vec().into_boxed_slice(),
        });
    }
    Ok(TextureArray {
        layers: 1,
        mips: mips.into_boxed_slice(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureArrayLimits {
    pub max_layers: u32,
    pub max_dimension_2d: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureLimitError {
    Layers { requested: u32, supported: u32 },
    Dimension { requested: u32, supported: u32 },
}

impl TextureArrayLimits {
    pub fn validate(self, layers: u32, dimension: u32) -> Result<(), TextureLimitError> {
        if layers > self.max_layers {
            return Err(TextureLimitError::Layers {
                requested: layers,
                supported: self.max_layers,
            });
        }
        if dimension > self.max_dimension_2d {
            return Err(TextureLimitError::Dimension {
                requested: dimension,
                supported: self.max_dimension_2d,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureMipUploadPlan {
    pub mip_level: u32,
    pub size: u32,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,
    pub layer_source_offsets: Box<[usize]>,
    pub layer_staging_offsets: Box<[usize]>,
    pub staging_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureUploadPlanError {
    ZeroAlignment,
    SizeOverflow,
    InvalidMipBytes,
}

pub fn plan_texture_mip_uploads(
    texture: &TextureArray,
    row_alignment: usize,
) -> Result<Vec<TextureMipUploadPlan>, TextureUploadPlanError> {
    if row_alignment == 0 {
        return Err(TextureUploadPlanError::ZeroAlignment);
    }
    let layers =
        usize::try_from(texture.layers).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
    texture
        .mips
        .iter()
        .enumerate()
        .map(|(mip_level, mip)| {
            let size =
                usize::try_from(mip.size).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
            let row_bytes = size
                .checked_mul(4)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let bytes_per_row = row_bytes
                .checked_add(row_alignment - 1)
                .map(|value| value / row_alignment * row_alignment)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let source_layer_bytes = row_bytes
                .checked_mul(size)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let staging_layer_bytes = bytes_per_row
                .checked_mul(size)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let expected = source_layer_bytes
                .checked_mul(layers)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            if mip.rgba8.len() != expected {
                return Err(TextureUploadPlanError::InvalidMipBytes);
            }
            let layer_source_offsets = (0..layers)
                .map(|layer| layer * source_layer_bytes)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let layer_staging_offsets = (0..layers)
                .map(|layer| layer * staging_layer_bytes)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Ok(TextureMipUploadPlan {
                mip_level: u32::try_from(mip_level)
                    .map_err(|_| TextureUploadPlanError::SizeOverflow)?,
                size: mip.size,
                bytes_per_row: u32::try_from(bytes_per_row)
                    .map_err(|_| TextureUploadPlanError::SizeOverflow)?,
                rows_per_image: mip.size,
                layer_source_offsets,
                layer_staging_offsets,
                staging_bytes: staging_layer_bytes
                    .checked_mul(layers)
                    .ok_or(TextureUploadPlanError::SizeOverflow)?,
            })
        })
        .collect()
}

#[must_use]
pub fn greedy_texture_uv(
    face: crate::Face,
    corner: u32,
    width: u32,
    height: u32,
    flags: u32,
) -> [f32; 2] {
    let width = width as f32;
    let height = height as f32;
    let horizontal_standard = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
    let horizontal_transposed = [[0.0, 0.0], [0.0, height], [width, height], [width, 0.0]];
    let vertical_standard = [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]];
    let vertical_transposed = [[0.0, height], [0.0, 0.0], [width, 0.0], [width, height]];
    let corner = (corner & 3) as usize;
    let [mut u, mut v] = match face {
        crate::Face::NegativeX | crate::Face::PositiveZ => vertical_standard[corner],
        crate::Face::PositiveX | crate::Face::NegativeZ => vertical_transposed[corner],
        crate::Face::NegativeY => horizontal_standard[corner],
        crate::Face::PositiveY => horizontal_transposed[corner],
    };
    let (extent_u, extent_v) = match flags & MATERIAL_UV_ROTATION_MASK {
        MATERIAL_UV_ROTATE_90 => {
            (u, v) = (v, width - u);
            (height, width)
        }
        MATERIAL_UV_ROTATE_180 => {
            (u, v) = (width - u, height - v);
            (width, height)
        }
        MATERIAL_UV_ROTATE_270 => {
            (u, v) = (height - v, u);
            (height, width)
        }
        _ => (width, height),
    };
    if flags & MATERIAL_UV_REFLECT_U != 0 {
        u = extent_u - u;
    }
    if flags & MATERIAL_UV_REFLECT_V != 0 {
        v = extent_v - v;
    }
    [u, v]
}

/// Maximum number of new or changed sub-chunks transferred to the render
/// world in one main-world update.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkUploadBudget {
    pub max_per_frame: usize,
}

impl Default for ChunkUploadBudget {
    fn default() -> Self {
        Self { max_per_frame: 32 }
    }
}

/// Sort key used by [`ChunkRenderQueue`] when an upload budget is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkUploadPriority(f32);

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

struct PendingUpload {
    mesh: ChunkMesh,
    biome: PackedBiomeRecord,
    tint_identity: ChunkBiomeTintIdentity,
    priority: ChunkUploadPriority,
    generation: u64,
    token: Option<ChunkUploadToken>,
}

struct PendingRemoval {
    priority: ChunkUploadPriority,
    token: Option<ChunkUploadToken>,
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
    pub manifest: Arc<[(SubChunkKey, u64)]>,
    pub view_generation: u64,
    pub render_ready_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelWitnessFrameEvaluation {
    revision: u64,
    request_hash: [u8; 32],
    total_model_ref_count: usize,
    manifest: Arc<[ModelWitnessManifestRecord]>,
    missing_key_count: usize,
    stale_generation_count: usize,
    wrong_stream_count: usize,
    zero_model_ref_count: usize,
    draw_mismatch_count: usize,
}

impl ModelWitnessFrameEvaluation {
    #[cfg(test)]
    fn is_exact(&self) -> bool {
        !self.manifest.is_empty()
            && self.total_model_ref_count != 0
            && self.missing_key_count == 0
            && self.stale_generation_count == 0
            && self.wrong_stream_count == 0
            && self.zero_model_ref_count == 0
            && self.draw_mismatch_count == 0
    }
}

fn evaluate_model_witness_frame(
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
struct CompletedFrameProbe {
    expectation: TargetRenderExpectation,
    frame_sequence: u64,
    allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    visible_allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    drawn_manifest: Arc<[(SubChunkKey, u64)]>,
    missing_target_instances: usize,
    unexpected_target_instances: usize,
    source_instances: usize,
    foreign_instances: usize,
    stale_generation_instances: usize,
    orphan_allocations: usize,
    transparent_sort_generation: u64,
    model_witness: Option<ModelWitnessFrameEvaluation>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct FrameCompletionEvidence {
    present_returned_at: Option<Instant>,
    submitted_work_done_at: Option<Instant>,
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
    fn is_model_witness_compatible(&self) -> bool {
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
            && self.drawn_manifest == self.allocation_manifest
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
            && self.view_generation == next.view_generation
            && self.render_ready_at == next.render_ready_at
            && self.transparent_sort_generation == next.transparent_sort_generation
            && self.frame_sequence.checked_add(1) == Some(next.frame_sequence)
            && self.gpu_completed_at <= next.gpu_completed_at
    }
}

#[derive(Default)]
struct PresentedFrameGateState {
    expectation: Option<TargetRenderExpectation>,
    acknowledgements: VecDeque<PresentedFrameAck>,
    in_flight_callbacks: usize,
}

/// Shared main/render-world target and bounded GPU-completed frame evidence.
#[derive(Resource, Clone)]
pub struct PresentedFrameGate {
    inner: Arc<Mutex<PresentedFrameGateState>>,
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

    fn try_reserve_callback(&self, expectation: &TargetRenderExpectation) -> bool {
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

    fn publish_reserved_probe(
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

fn build_presented_frame_ack(
    probe: CompletedFrameProbe,
    evidence: FrameCompletionEvidence,
) -> Option<PresentedFrameAck> {
    let present_returned_at = evidence.present_returned_at?;
    let gpu_completed_at = evidence.submitted_work_done_at?;
    if probe.expectation.render_ready_at > present_returned_at
        || present_returned_at > gpu_completed_at
    {
        return None;
    }
    let model_witness = probe.model_witness.map(|model| ModelWitnessFrameAck {
        revision: model.revision,
        request_hash: model.request_hash,
        frame_sequence: probe.frame_sequence,
        view_generation: probe.expectation.view_generation,
        present_returned_at,
        gpu_completed_at,
        total_model_ref_count: model.total_model_ref_count,
        manifest: model.manifest,
        missing_key_count: model.missing_key_count,
        stale_generation_count: model.stale_generation_count,
        wrong_stream_count: model.wrong_stream_count,
        zero_model_ref_count: model.zero_model_ref_count,
        draw_mismatch_count: model.draw_mismatch_count,
    });
    Some(PresentedFrameAck {
        cohort: probe.expectation.cohort,
        frame_sequence: probe.frame_sequence,
        allocation_manifest: probe.allocation_manifest,
        visible_allocation_manifest: probe.visible_allocation_manifest,
        drawn_manifest: probe.drawn_manifest,
        view_generation: probe.expectation.view_generation,
        render_ready_at: probe.expectation.render_ready_at,
        present_returned_at,
        gpu_completed_at,
        missing_target_instances: probe.missing_target_instances,
        unexpected_target_instances: probe.unexpected_target_instances,
        source_instances: probe.source_instances,
        foreign_instances: probe.foreign_instances,
        stale_generation_instances: probe.stale_generation_instances,
        orphan_allocations: probe.orphan_allocations,
        transparent_sort_generation: probe.transparent_sort_generation,
        model_witness,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameInstanceIdentity {
    entity: Entity,
    key: SubChunkKey,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameAllocationIdentity {
    entity: Entity,
    key: SubChunkKey,
    generation: u64,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
struct ChunkStreamMask(u8);

impl ChunkStreamMask {
    const CUBE: Self = Self(1 << 0);
    const MODEL: Self = Self(1 << 1);
    const LIQUID: Self = Self(1 << 2);

    const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ChunkStreamMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

trait IntoFrameAllocationEvidence {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize);
}

impl IntoFrameAllocationEvidence for FrameAllocationIdentity {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        (self, ChunkStreamMask::CUBE, 0)
    }
}

impl IntoFrameAllocationEvidence for (FrameAllocationIdentity, ChunkStreamMask) {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        (self.0, self.1, 0)
    }
}

impl IntoFrameAllocationEvidence for (FrameAllocationIdentity, ChunkStreamMask, usize) {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        self
    }
}

struct FrameProbe {
    expectation: TargetRenderExpectation,
    frame_sequence: u64,
    eligible: HashMap<Entity, (FrameAllocationIdentity, ChunkStreamMask)>,
    expected_streams: BTreeMap<(SubChunkKey, u64), ChunkStreamMask>,
    model_request: ModelWitnessRequest,
    model_allocations: BTreeMap<(SubChunkKey, u64), (ChunkStreamMask, usize)>,
    allocation_manifest: BTreeSet<(SubChunkKey, u64)>,
    visible_allocation_manifest: Mutex<BTreeSet<(SubChunkKey, u64)>>,
    target_allocation_count: usize,
    duplicate_target_instances: usize,
    drawn: Mutex<BTreeMap<(SubChunkKey, u64), ChunkStreamMask>>,
    drawn_transparent_generation: Mutex<Option<ViewSortGeneration>>,
    source_instances: usize,
    foreign_instances: usize,
    stale_generation_instances: usize,
    orphan_allocations: usize,
}

impl FrameProbe {
    #[cfg(test)]
    fn begin(
        expectation: TargetRenderExpectation,
        instances: impl IntoIterator<Item = FrameInstanceIdentity>,
        allocations: impl IntoIterator<Item = impl IntoFrameAllocationEvidence>,
    ) -> Self {
        Self::begin_with_model_witness(
            expectation,
            instances,
            allocations,
            ModelWitnessRequest::default(),
        )
    }

    fn begin_with_model_witness(
        expectation: TargetRenderExpectation,
        instances: impl IntoIterator<Item = FrameInstanceIdentity>,
        allocations: impl IntoIterator<Item = impl IntoFrameAllocationEvidence>,
        model_request: ModelWitnessRequest,
    ) -> Self {
        let model_target_keys = model_request.enabled().then(|| {
            model_request
                .keys()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        });
        let is_scoped_instance = |key: SubChunkKey| {
            model_target_keys
                .as_ref()
                .is_none_or(|target_keys| target_keys.contains(&key))
        };
        let is_target = |key: SubChunkKey| {
            model_target_keys.as_ref().map_or_else(
                || expectation.cohort.contains(key),
                |target_keys| target_keys.contains(&key),
            )
        };
        let expected = expectation
            .manifest
            .iter()
            .copied()
            .collect::<BTreeMap<_, _>>();
        let instances = instances
            .into_iter()
            .map(|instance| (instance.entity, instance))
            .collect::<HashMap<_, _>>();
        let source_instances = instances
            .values()
            .filter(|instance| {
                is_scoped_instance(instance.key)
                    && expectation
                        .source_cohort
                        .is_some_and(|source| source.contains(instance.key))
            })
            .count();
        let foreign_instances = instances
            .values()
            .filter(|instance| {
                is_scoped_instance(instance.key)
                    && !expectation.cohort.contains(instance.key)
                    && expectation
                        .source_cohort
                        .is_none_or(|source| !source.contains(instance.key))
            })
            .count();
        let target_instance_count = instances
            .values()
            .filter(|instance| is_target(instance.key))
            .count();
        let unique_target_instance_keys = instances
            .values()
            .filter(|instance| is_target(instance.key))
            .map(|instance| instance.key)
            .collect::<BTreeSet<_>>()
            .len();
        let duplicate_target_instances =
            target_instance_count.saturating_sub(unique_target_instance_keys);
        let mut stale_entities = instances
            .values()
            .filter_map(|instance| {
                expected
                    .get(&instance.key)
                    .is_some_and(|generation| *generation != instance.generation)
                    .then_some(instance.entity)
            })
            .collect::<BTreeSet<_>>();
        let mut eligible = HashMap::new();
        let mut expected_streams_by_identity = BTreeMap::new();
        let mut model_allocations = BTreeMap::new();
        let mut allocation_manifest = BTreeSet::new();
        let mut target_allocation_count = 0;
        let mut orphan_allocations = 0;
        for allocation in allocations {
            let (allocation, expected_streams, model_ref_count) = allocation.into_evidence();
            let Some(instance) = instances.get(&allocation.entity) else {
                if is_scoped_instance(allocation.key) {
                    orphan_allocations += 1;
                }
                continue;
            };
            if instance.key != allocation.key || instance.generation != allocation.generation {
                if model_target_keys.is_none()
                    || is_target(instance.key)
                    || is_target(allocation.key)
                {
                    stale_entities.insert(allocation.entity);
                }
                continue;
            }
            if is_target(allocation.key) {
                target_allocation_count += 1;
                allocation_manifest.insert((allocation.key, allocation.generation));
            }
            let identity = (allocation.key, allocation.generation);
            let mask = expected_streams_by_identity.entry(identity).or_default();
            *mask = *mask | expected_streams;
            if is_target(allocation.key) {
                let model = model_allocations
                    .entry(identity)
                    .or_insert((ChunkStreamMask::default(), 0));
                model.0 = model.0 | expected_streams;
                model.1 = model.1.max(model_ref_count);
            }
            eligible.insert(allocation.entity, (allocation, expected_streams));
        }
        Self {
            expectation,
            frame_sequence: 0,
            eligible,
            expected_streams: expected_streams_by_identity,
            model_request,
            model_allocations,
            allocation_manifest,
            visible_allocation_manifest: Mutex::new(BTreeSet::new()),
            target_allocation_count,
            duplicate_target_instances,
            drawn: Mutex::new(BTreeMap::new()),
            drawn_transparent_generation: Mutex::new(None),
            source_instances,
            foreign_instances,
            stale_generation_instances: stale_entities.len(),
            orphan_allocations,
        }
    }

    fn record_direct_draw(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        self.record_direct_streams(entity, allocation, ChunkStreamMask::CUBE)
    }

    fn record_visible(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        let Some(&(eligible, _)) = self.eligible.get(&entity) else {
            return false;
        };
        if eligible != allocation {
            return false;
        }
        let identity = (allocation.key, allocation.generation);
        if self.allocation_manifest.contains(&identity) {
            self.visible_allocation_manifest
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(identity);
        }
        true
    }

    fn record_direct_streams(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
        streams: ChunkStreamMask,
    ) -> bool {
        let Some(&(eligible, _expected_streams)) = self.eligible.get(&entity) else {
            return false;
        };
        if eligible != allocation {
            return false;
        }
        let identity = (allocation.key, allocation.generation);
        if self.allocation_manifest.contains(&identity) {
            let mut drawn = self
                .drawn
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mask = drawn.entry(identity).or_default();
            *mask = *mask | streams;
        }
        true
    }

    fn record_mdi_draws(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        draws
            .into_iter()
            .filter(|(entity, allocation)| self.record_direct_draw(*entity, *allocation))
            .count()
    }

    fn record_mdi_streams(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
        streams: ChunkStreamMask,
    ) -> usize {
        draws
            .into_iter()
            .filter(|(entity, allocation)| {
                self.record_direct_streams(*entity, *allocation, streams)
            })
            .count()
    }

    fn record_transparent_draw(
        &self,
        generation: ViewSortGeneration,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let draws = draws.into_iter().collect::<Vec<_>>();
        let encoded = !draws.is_empty();
        let count = draws
            .into_iter()
            .filter(|(entity, allocation)| {
                self.record_direct_streams(*entity, *allocation, ChunkStreamMask::LIQUID)
            })
            .count();
        if encoded {
            *self
                .drawn_transparent_generation
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(generation);
        }
        count
    }

    fn complete(self) -> CompletedFrameProbe {
        let transparent_sort_generation = self
            .drawn_transparent_generation
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner())
            .map_or(0, ViewSortGeneration::get);
        let expected = self
            .expectation
            .manifest
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let visible_allocation_manifest = self
            .visible_allocation_manifest
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        let drawn_streams = self
            .drawn
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        let model_witness = self.model_request.enabled().then(|| {
            let expected = self.expectation.manifest.as_ref();
            let allocations = self
                .model_allocations
                .iter()
                .map(|(&(key, generation), &(streams, model_ref_count))| {
                    (key, generation, streams, model_ref_count)
                })
                .collect::<Vec<_>>();
            let drawn = drawn_streams
                .iter()
                .map(|(&(key, generation), &streams)| (key, generation, streams))
                .collect::<Vec<_>>();
            evaluate_model_witness_frame(
                &self.model_request,
                self.frame_sequence,
                self.expectation.view_generation,
                expected,
                &allocations,
                &drawn,
            )
        });
        let drawn = drawn_streams
            .into_iter()
            .filter_map(|(identity, drawn)| {
                let expected = self.expected_streams.get(&identity).copied()?;
                drawn.contains(expected).then_some(identity)
            })
            .collect::<BTreeSet<_>>();
        let matched_target_instances = expected.intersection(&self.allocation_manifest).count();
        let missing_target_instances = expected.len().saturating_sub(matched_target_instances);
        let unexpected_target_instances = self
            .target_allocation_count
            .saturating_sub(matched_target_instances)
            .max(self.duplicate_target_instances);
        CompletedFrameProbe {
            expectation: self.expectation,
            frame_sequence: self.frame_sequence,
            allocation_manifest: Arc::from(
                self.allocation_manifest.into_iter().collect::<Vec<_>>(),
            ),
            visible_allocation_manifest: Arc::from(
                visible_allocation_manifest.into_iter().collect::<Vec<_>>(),
            ),
            drawn_manifest: Arc::from(drawn.into_iter().collect::<Vec<_>>()),
            missing_target_instances,
            unexpected_target_instances,
            source_instances: self.source_instances,
            foreign_instances: self.foreign_instances,
            stale_generation_instances: self.stale_generation_instances,
            orphan_allocations: self.orphan_allocations,
            transparent_sort_generation,
            model_witness,
        }
    }
}

#[derive(Default)]
struct ActiveFrameProbeState {
    current: Option<FrameProbe>,
    next_frame_sequence: u64,
}

#[derive(Resource, Default)]
struct ActiveFrameProbe(Mutex<ActiveFrameProbeState>);

impl ActiveFrameProbe {
    fn is_active(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .is_some()
    }

    fn begin(&self, mut probe: FrameProbe) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.next_frame_sequence = state.next_frame_sequence.wrapping_add(1).max(1);
        probe.frame_sequence = state.next_frame_sequence;
        state.current = Some(probe);
    }

    fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current = None;
    }

    fn accepts(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| {
                probe
                    .eligible
                    .get(&entity)
                    .is_some_and(|(eligible, _)| *eligible == allocation)
            })
    }

    fn record_visible(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_visible(entity, allocation))
    }

    fn record_direct_draw(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_direct_draw(entity, allocation))
    }

    fn record_direct_streams(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
        streams: ChunkStreamMask,
    ) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_direct_streams(entity, allocation, streams))
    }

    fn record_mdi_draws(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_mdi_draws(draws.iter().copied())
        })
    }

    fn record_mdi_streams(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
        streams: ChunkStreamMask,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_mdi_streams(draws.iter().copied(), streams)
        })
    }

    fn record_transparent_draw(
        &self,
        generation: ViewSortGeneration,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_transparent_draw(generation, draws.iter().copied())
        })
    }

    fn take_completed(&self) -> Option<CompletedFrameProbe> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .take()
            .map(FrameProbe::complete)
    }
}

enum AcknowledgementSlot {
    Reserved {
        token: ChunkUploadToken,
        prior_ready: Option<ChunkUploadAcknowledgement>,
    },
    Ready(ChunkUploadAcknowledgement),
}

struct AcknowledgementState {
    capacity: usize,
    slots: HashMap<SubChunkKey, AcknowledgementSlot>,
}

#[derive(Resource, Clone)]
pub struct ChunkUploadAcknowledgements {
    inner: Arc<Mutex<AcknowledgementState>>,
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

    fn try_reserve(&self, key: SubChunkKey, token: ChunkUploadToken) -> bool {
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

    fn cancel(&self, key: SubChunkKey, token: ChunkUploadToken) -> bool {
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

    fn complete(&self, key: SubChunkKey, token: ChunkUploadToken, applied_at: Instant) -> bool {
        self.complete_with_bytes(key, token, applied_at, 0)
    }

    fn complete_with_bytes(
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
    fn record(&self, acknowledgement: ChunkUploadAcknowledgement) -> bool {
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
    pending: HashMap<SubChunkKey, PendingUpload>,
    removals: HashMap<SubChunkKey, PendingRemoval>,
    render_manifest: BTreeMap<SubChunkKey, u64>,
    next_generation: u64,
    pending_bytes: u64,
    limits: ChunkRenderQueueLimits,
    gpu_upload_bytes: u64,
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

    fn try_remove_inner(
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

    fn try_enqueue(
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

fn mesh_byte_len(mesh: &ChunkMesh) -> u64 {
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

fn biome_record_is_fallback(record: &PackedBiomeRecord) -> bool {
    record.words() == FALLBACK_BIOME_RECORD
}

fn biome_record_byte_len(record: &PackedBiomeRecord) -> u64 {
    if biome_record_is_fallback(record) {
        0
    } else {
        record.byte_len()
    }
}

fn pending_upload_byte_len(pending: &PendingUpload) -> u64 {
    mesh_byte_len(&pending.mesh).saturating_add(biome_record_byte_len(&pending.biome))
}

/// Extracted packed geometry for one visible, frustum-cullable sub-chunk.
#[derive(Component, Clone, ExtractComponent)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<ChunkRenderInstance>)]
pub struct ChunkRenderInstance {
    key: SubChunkKey,
    cube_quads: Arc<[PackedQuad]>,
    cube_lighting: Arc<[PackedQuadLighting]>,
    model_refs: Arc<[PackedModelRef]>,
    model_lighting: Arc<[PackedQuadLighting]>,
    model_draw_refs: Arc<[PackedModelDrawRef]>,
    transparent_model_draw_refs: Arc<[PackedModelDrawRef]>,
    liquid_quads: Arc<[PackedLiquidQuad]>,
    liquid_lighting: Arc<[PackedQuadLighting]>,
    has_depth_liquid: bool,
    has_transparent_liquid: bool,
    depth_liquid_start: Option<u32>,
    biome: PackedBiomeRecord,
    tint_identity: ChunkBiomeTintIdentity,
    generation: u64,
    token: Option<ChunkUploadToken>,
    origin: [i32; 3],
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

    /// CPU-retained cube lighting sidecars. GPU consumption is integrated in a
    /// later slice without changing this extraction contract.
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
}

#[derive(Resource, Default)]
struct ChunkEntities(HashMap<SubChunkKey, Entity>);

/// Installs the capped main-world queue and the vertex-pulled Camera3d chunk
/// draw path. The renderer adds non-mesh items to Bevy's built-in opaque
/// phase, sharing its depth attachment without allocating a `Mesh` or
/// `StandardMaterial` per sub-chunk.
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugWorldPlugin {
    upload_budget: ChunkUploadBudget,
}

impl DebugWorldPlugin {
    #[must_use]
    pub const fn new(max_uploads_per_frame: usize) -> Self {
        Self {
            upload_budget: ChunkUploadBudget {
                max_per_frame: max_uploads_per_frame,
            },
        }
    }
}

impl Plugin for DebugWorldPlugin {
    fn build(&self, app: &mut App) {
        install_atmosphere(app);
        app.init_resource::<ChunkRenderQueue>()
            .init_resource::<ChunkUploadAcknowledgements>()
            .init_resource::<PresentedFrameGate>()
            .init_resource::<ChunkEntities>()
            .init_resource::<ChunkTextureAssets>()
            .init_resource::<ChunkAnimationClock>()
            .init_resource::<ChunkBiomeTints>()
            .init_resource::<TransparentSortMetrics>()
            .init_resource::<ModelWorkloadMetrics>()
            .init_resource::<TransparentWitnessRequest>()
            .init_resource::<TransparentWitnessEvidence>()
            .init_resource::<ModelWitnessRequest>()
            .init_resource::<ModelWitnessEvidence>()
            .insert_resource(self.upload_budget)
            .add_systems(
                Update,
                (apply_chunk_render_queue, update_chunk_animation_clock),
            );

        if app.get_sub_app(RenderApp).is_none() {
            return;
        }

        app.add_plugins((
            ExtractComponentPlugin::<ChunkRenderInstance>::default(),
            ExtractResourcePlugin::<ChunkTextureAssets>::default(),
            ExtractResourcePlugin::<ChunkAnimationClock>::default(),
            ExtractResourcePlugin::<ChunkBiomeTints>::default(),
            ExtractResourcePlugin::<TransparentWitnessRequest>::default(),
            ExtractResourcePlugin::<ModelWitnessRequest>::default(),
        ));

        load_internal_asset!(
            app,
            LIGHTING_SHADER_HANDLE,
            "lighting.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(app, CHUNK_SHADER_HANDLE, "chunk.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, MODEL_SHADER_HANDLE, "model.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, LIQUID_SHADER_HANDLE, "liquid.wgsl", Shader::from_wgsl);

        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        let presented_frame_gate = app.world().resource::<PresentedFrameGate>().clone();
        let transparent_sort_metrics = app.world().resource::<TransparentSortMetrics>().clone();
        let model_workload_metrics = app.world().resource::<ModelWorkloadMetrics>().clone();
        let transparent_witness_evidence =
            app.world().resource::<TransparentWitnessEvidence>().clone();

        app.sub_app_mut(RenderApp)
            .insert_resource(self.upload_budget)
            .insert_resource(acknowledgements)
            .insert_resource(presented_frame_gate)
            .insert_resource(transparent_sort_metrics)
            .insert_resource(model_workload_metrics)
            .insert_resource(transparent_witness_evidence)
            .init_resource::<ChunkPipeline>()
            .init_resource::<ChunkGpuUploadStats>()
            .init_resource::<GpuUpdateFairness>()
            .init_resource::<ChunkGpuTextureAssets>()
            .init_resource::<ChunkGpuBiomeTints>()
            .init_resource::<ChunkTextureUploadStats>()
            .init_resource::<ChunkIndirectBatches>()
            .init_resource::<ChunkModelIndirectBatches>()
            .init_resource::<ChunkDepthLiquidIndirectBatches>()
            .init_resource::<ActiveFrameProbe>()
            .init_resource::<TransparentSortRuntime>()
            .init_resource::<TransparentModelSortRuntime>()
            .init_resource::<TransparentUploadBudget>()
            .init_resource::<TransparentPresentationFence>()
            .init_resource::<TransparentRetirementFence>()
            .add_render_command::<Opaque3d, DrawChunkCommands>()
            .add_render_command::<Opaque3d, DrawChunkIndirectCommands>()
            .add_render_command::<Opaque3d, DrawModelCommands>()
            .add_render_command::<Opaque3d, DrawModelIndirectCommands>()
            .add_render_command::<Opaque3d, DrawDepthLiquidCommands>()
            .add_render_command::<Opaque3d, DrawDepthLiquidIndirectCommands>()
            .add_render_command::<Transparent3d, DrawTransparentModelCommands>()
            .add_render_command::<Transparent3d, DrawTransparentLiquidCommands>()
            .add_render_command::<Transparent3d, DrawTransparentLiquidIndirectCommands>()
            .add_systems(
                RenderStartup,
                (init_chunk_gpu_arena, init_chunk_gpu_animation_clock),
            )
            .add_systems(
                Render,
                (
                    queue_chunks.in_set(RenderSystems::Queue),
                    queue_transparent_chunks.in_set(RenderSystems::Queue),
                    prepare_chunk_texture_assets.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_animation_clock.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_biome_tints.in_set(RenderSystems::PrepareResources),
                    prepare_gpu_chunks.in_set(RenderSystems::PrepareResources),
                    prepare_transparent_sorts
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_transparent_model_sorts
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_transparent_sorts),
                    prepare_chunk_indirect_batches
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_chunk_bind_group.in_set(RenderSystems::PrepareBindGroups),
                    submit_presented_frame_probe
                        .in_set(RenderSystems::Render)
                        .after(bevy::render::renderer::render_system),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        install_atmosphere(app);
    }
}

fn update_chunk_animation_clock(time: Option<Res<Time>>, mut clock: ResMut<ChunkAnimationClock>) {
    let elapsed_seconds = time.map_or(0.0, |time| time.elapsed_secs_f64());
    *clock = ChunkAnimationClock::from_elapsed_seconds(elapsed_seconds);
}

fn apply_chunk_render_queue(
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

    let mut applied = 0;
    for (key, _, removal) in ready {
        if applied >= budget.max_per_frame {
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
            applied += 1;
            continue;
        }
        let Some(pending) = queue.pending.get(&key) else {
            continue;
        };
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
            applied += 1;
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
        applied += 1;
    }
}

const fn chunk_origin(key: SubChunkKey) -> [i32; 3] {
    [
        key.x.saturating_mul(16),
        key.y.saturating_mul(16),
        key.z.saturating_mul(16),
    ]
}

struct ChunkPipelineSpecializer;

#[derive(Resource)]
struct ChunkPipeline {
    variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    model_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    transparent_model_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    liquid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    depth_liquid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ChunkPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "chunk vertex-pulling bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 8,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 9,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 10,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 11,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(ChunkAnimationClock::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 12,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 13,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 14,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 15,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(AtmosphereFrame::min_size()),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("packed chunk pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: CHUNK_SHADER_HANDLE,
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CHUNK_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                cull_mode: Some(CullFace::Back),
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        let mut model_descriptor = descriptor.clone();
        model_descriptor.label = Some("packed model pipeline".into());
        model_descriptor.vertex.shader = MODEL_SHADER_HANDLE;
        model_descriptor
            .fragment
            .as_mut()
            .expect("model fragment")
            .shader = MODEL_SHADER_HANDLE;
        model_descriptor
            .fragment
            .as_mut()
            .expect("model fragment")
            .entry_point = Some("fragment".into());
        model_descriptor.primitive.cull_mode = None;
        let mut transparent_model_descriptor = model_descriptor.clone();
        transparent_model_descriptor.label = Some("packed transparent model pipeline".into());
        let transparent_model_fragment = transparent_model_descriptor
            .fragment
            .as_mut()
            .expect("transparent model fragment");
        transparent_model_fragment.entry_point = Some("fragment_blend".into());
        transparent_model_fragment.targets[0]
            .as_mut()
            .expect("transparent model colour target")
            .blend = Some(BlendState::ALPHA_BLENDING);
        transparent_model_descriptor
            .depth_stencil
            .as_mut()
            .expect("transparent model depth state")
            .depth_write_enabled = false;
        let mut liquid_descriptor = descriptor.clone();
        liquid_descriptor.label = Some("packed transparent liquid pipeline".into());
        liquid_descriptor.vertex.shader = LIQUID_SHADER_HANDLE;
        liquid_descriptor.vertex.entry_point = Some("vertex".into());
        liquid_descriptor
            .fragment
            .as_mut()
            .expect("liquid fragment")
            .shader = LIQUID_SHADER_HANDLE;
        liquid_descriptor
            .fragment
            .as_mut()
            .expect("liquid fragment")
            .entry_point = Some("fragment".into());
        liquid_descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .blend = Some(BlendState::ALPHA_BLENDING);
        liquid_descriptor
            .depth_stencil
            .as_mut()
            .expect("liquid depth state")
            .depth_write_enabled = false;
        liquid_descriptor
            .depth_stencil
            .as_mut()
            .expect("liquid depth state")
            .depth_compare = CompareFunction::GreaterEqual;
        liquid_descriptor.primitive.cull_mode = None;
        let mut depth_liquid_descriptor = descriptor.clone();
        depth_liquid_descriptor.label = Some("packed depth-writing liquid pipeline".into());
        depth_liquid_descriptor.vertex.shader = LIQUID_SHADER_HANDLE;
        depth_liquid_descriptor.vertex.entry_point = Some("vertex_depth".into());
        let depth_fragment = depth_liquid_descriptor
            .fragment
            .as_mut()
            .expect("depth-writing liquid fragment");
        depth_fragment.shader = LIQUID_SHADER_HANDLE;
        depth_fragment.entry_point = Some("fragment_depth".into());
        depth_liquid_descriptor.primitive.cull_mode = None;
        Self {
            variants: Variants::new(ChunkPipelineSpecializer, descriptor),
            model_variants: Variants::new(ChunkPipelineSpecializer, model_descriptor),
            transparent_model_variants: Variants::new(
                ChunkPipelineSpecializer,
                transparent_model_descriptor,
            ),
            liquid_variants: Variants::new(ChunkPipelineSpecializer, liquid_descriptor),
            depth_liquid_variants: Variants::new(ChunkPipelineSpecializer, depth_liquid_descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct ChunkPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for ChunkPipelineSpecializer {
    type Key = ChunkPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        Ok(key)
    }
}

#[derive(Component, Clone)]
struct GpuChunkAllocation {
    key: SubChunkKey,
    generation: u64,
    tint_identity: ChunkBiomeTintIdentity,
    quad_range: Range<u32>,
    cube_lighting_range: Option<Range<u32>>,
    model_range: Option<Range<u32>>,
    model_lighting_range: Option<Range<u32>>,
    model_draw_range: Option<Range<u32>>,
    transparent_model_draw_range: Option<Range<u32>>,
    liquid_range: Option<Range<u32>>,
    liquid_lighting_range: Option<Range<u32>>,
    has_depth_liquid: bool,
    has_transparent_liquid: bool,
    depth_liquid_range: Option<Range<u32>>,
    metadata_index: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StreamAddresses {
    cube: Option<Range<u32>>,
    cube_lighting: Option<Range<u32>>,
    model: Option<Range<u32>>,
    model_lighting: Option<Range<u32>>,
    model_draw: Option<Range<u32>>,
    transparent_model_draw: Option<Range<u32>>,
    liquid: Option<Range<u32>>,
    liquid_lighting: Option<Range<u32>>,
}

fn direct_stream_addresses(allocation: &GpuChunkAllocation) -> StreamAddresses {
    StreamAddresses {
        cube: (!allocation.quad_range.is_empty()).then(|| allocation.quad_range.clone()),
        cube_lighting: allocation.cube_lighting_range.clone(),
        model: allocation.model_range.clone(),
        model_lighting: allocation.model_lighting_range.clone(),
        model_draw: allocation.model_draw_range.clone(),
        transparent_model_draw: allocation.transparent_model_draw_range.clone(),
        liquid: allocation.liquid_range.clone(),
        liquid_lighting: allocation.liquid_lighting_range.clone(),
    }
}

fn mdi_stream_addresses(allocation: &GpuChunkAllocation) -> StreamAddresses {
    direct_stream_addresses(allocation)
}

fn cube_lighting_record_address(addresses: &StreamAddresses, global_quad: u32) -> Option<u32> {
    if !cube_stream_addresses_valid(addresses) {
        return None;
    }
    let cube = addresses.cube.as_ref()?;
    let lighting = addresses.cube_lighting.as_ref()?;
    if global_quad < cube.start || global_quad >= cube.end || !lighting.start.is_multiple_of(2) {
        return None;
    }
    let local = global_quad.checked_sub(cube.start)?;
    let record = lighting.start.checked_div(2)?.checked_add(local)?;
    (record < lighting.end.checked_div(2)?).then_some(record)
}

fn cube_stream_addresses_valid(addresses: &StreamAddresses) -> bool {
    match (addresses.cube.as_ref(), addresses.cube_lighting.as_ref()) {
        (None, None) => true,
        (Some(cube), Some(lighting)) => {
            !cube.is_empty()
                && !lighting.is_empty()
                && lighting.start.is_multiple_of(2)
                && lighting.end.is_multiple_of(2)
                && cube.end.checked_sub(cube.start)
                    == lighting
                        .end
                        .checked_sub(lighting.start)
                        .and_then(|words| words.checked_div(2))
        }
        _ => false,
    }
}

fn shared_stream_ranges_disjoint(addresses: &StreamAddresses) -> bool {
    let ranges = [
        addresses.model.as_ref(),
        addresses.model_lighting.as_ref(),
        addresses.model_draw.as_ref(),
        addresses.transparent_model_draw.as_ref(),
        addresses.liquid.as_ref(),
        addresses.liquid_lighting.as_ref(),
        addresses.cube_lighting.as_ref(),
    ];
    ranges.iter().enumerate().all(|(index, left)| {
        left.is_none_or(|left| {
            !left.is_empty()
                && ranges[index + 1..]
                    .iter()
                    .flatten()
                    .all(|right| left.end <= right.start || right.end <= left.start)
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkDrawMode {
    Direct,
    MultiDrawIndirect,
    Unsupported,
}

fn select_chunk_draw_mode(
    downlevel_flags: DownlevelFlags,
    features: WgpuFeatures,
    is_dx12: bool,
    debug_assertions: bool,
) -> ChunkDrawMode {
    if !downlevel_flags.contains(DownlevelFlags::BASE_VERTEX) {
        ChunkDrawMode::Unsupported
    } else if downlevel_flags.contains(DownlevelFlags::INDIRECT_EXECUTION)
        && features.contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE)
        // wgpu 27's DX12 indirect validator expands each indexed command
        // from 20 to 32 bytes for special constants, but its debug batching
        // assertion still assumes the unexpanded stride. Preserve MDI in
        // release builds and use the equivalent direct path while that
        // validator is active.
        && !(debug_assertions && is_dx12)
    {
        ChunkDrawMode::MultiDrawIndirect
    } else {
        ChunkDrawMode::Direct
    }
}

fn indexed_indirect_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    let addresses = mdi_stream_addresses(allocation);
    if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
        return None;
    }
    let cube = addresses.cube.as_ref()?;
    let instance_count = cube.end.checked_sub(cube.start)?;
    if instance_count == 0 {
        return None;
    }
    cube_lighting_record_address(&addresses, cube.start)?;
    cube_lighting_record_address(&addresses, cube.end.checked_sub(1)?)?;
    let base_vertex = metadata_base_vertex(allocation.metadata_index)?;
    Some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex,
        first_instance: cube.start,
    })
}

fn model_draw_command(
    allocation: &GpuChunkAllocation,
    addresses: StreamAddresses,
) -> Option<DrawIndexedIndirectArgs> {
    let model = addresses.model?;
    let model_lighting = addresses.model_lighting?;
    let model_draw = addresses.model_draw?;
    let expected_draw_start =
        if allocation.transparent_model_draw_range.as_ref() == Some(&model_draw) {
            allocation
                .model_draw_range
                .as_ref()
                .map_or(model_lighting.end, |range| range.end)
        } else {
            model_lighting.end
        };
    if model.is_empty()
        || !model.start.is_multiple_of(4)
        || !model.end.is_multiple_of(4)
        || model_lighting.is_empty()
        || !model_lighting.start.is_multiple_of(2)
        || !model_lighting.end.is_multiple_of(2)
        || model_draw.is_empty()
        || !model_draw.start.is_multiple_of(2)
        || !model_draw.end.is_multiple_of(2)
        || model.end != model_lighting.start
        || expected_draw_start != model_draw.start
    {
        return None;
    }
    let first_instance = model_draw.start.checked_div(2)?;
    let end_instance = model_draw.end.checked_div(2)?;
    let instance_count = end_instance.checked_sub(first_instance)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: MODEL_INDEX_COUNT,
        instance_count,
        first_index: 0,
        base_vertex: allocation
            .metadata_index
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())?,
        first_instance,
    })
}

fn model_direct_draw_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    model_draw_command(allocation, direct_stream_addresses(allocation))
}

fn model_mdi_draw_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    model_draw_command(allocation, mdi_stream_addresses(allocation))
}

fn transparent_model_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let mut addresses = direct_stream_addresses(allocation);
    addresses.model_draw = addresses.transparent_model_draw.clone();
    model_draw_command(allocation, addresses)
}

fn model_ref_count_for_witness(allocation: &GpuChunkAllocation) -> usize {
    allocation.model_range.as_ref().map_or(0, |range| {
        range.end.saturating_sub(range.start) as usize
            / (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize
    })
}

const LEGACY_FIXED_MODEL_QUADS_PER_REF: usize = 32;

fn summarize_model_workload<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> ModelWorkloadCount {
    allocations
        .into_iter()
        .filter(|allocation| {
            model_direct_draw_command(allocation).is_some()
                || transparent_model_direct_draw_command(allocation).is_some()
        })
        .fold(ModelWorkloadCount::default(), |mut total, allocation| {
            let model_ref_count = model_ref_count_for_witness(allocation);
            let model_draw_ref_count = allocation
                .model_draw_range
                .as_ref()
                .into_iter()
                .chain(allocation.transparent_model_draw_range.as_ref())
                .fold(0_usize, |total, range| {
                    total.saturating_add(
                        range.end.saturating_sub(range.start) as usize
                            / (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize,
                    )
                });
            total.model_ref_count = total.model_ref_count.saturating_add(model_ref_count);
            total.model_draw_ref_count = total
                .model_draw_ref_count
                .saturating_add(model_draw_ref_count);
            total.legacy_fixed_slot_quad_invocations_avoided = total
                .legacy_fixed_slot_quad_invocations_avoided
                .saturating_add(
                    model_ref_count
                        .saturating_mul(LEGACY_FIXED_MODEL_QUADS_PER_REF)
                        .saturating_sub(model_draw_ref_count),
                );
            total
        })
}

fn depth_liquid_draw_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    if !allocation.has_depth_liquid {
        return None;
    }
    allocation.liquid_lighting_range.as_ref()?;
    let liquid = allocation.depth_liquid_range.as_ref()?;
    let instance_count = liquid.end.checked_sub(liquid.start)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: metadata_base_vertex(allocation.metadata_index)?,
        first_instance: liquid.start,
    })
}

fn depth_liquid_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

fn depth_liquid_mdi_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

fn metadata_base_vertex(metadata_index: u32) -> Option<i32> {
    metadata_index
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuChunkOrigin {
    value: [i32; 4],
    cube_bases: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<GpuChunkOrigin>() == CHUNK_ORIGIN_BYTES as usize);

fn gpu_chunk_origin(
    origin: [i32; 3],
    biome_start: u32,
    cube_quad_base: u32,
    cube_lighting_word_start: u32,
) -> Option<GpuChunkOrigin> {
    if !cube_lighting_word_start.is_multiple_of(2) {
        return None;
    }
    Some(GpuChunkOrigin {
        value: [
            origin[0],
            origin[1],
            origin[2],
            i32::try_from(biome_start).ok()?,
        ],
        cube_bases: [
            cube_quad_base,
            cube_lighting_word_start.checked_div(2)?,
            0,
            0,
        ],
    })
}

#[cfg(test)]
fn build_indexed_indirect_commands<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> Vec<DrawIndexedIndirectArgs> {
    allocations
        .into_iter()
        .filter_map(indexed_indirect_command)
        .collect()
}

struct ChunkIndirectBatch {
    visible_entities: Vec<Entity>,
    drawn_allocations: Vec<(Entity, FrameAllocationIdentity)>,
    indirect_offset: u64,
    command_count: u32,
}

#[derive(Resource, Default)]
struct ChunkIndirectBatches(HashMap<Entity, ChunkIndirectBatch>);

#[derive(Resource, Default)]
struct ChunkModelIndirectBatches(HashMap<Entity, ChunkIndirectBatch>);

#[derive(Resource, Default)]
struct ChunkDepthLiquidIndirectBatches(HashMap<Entity, ChunkIndirectBatch>);

#[derive(Clone)]
struct ArenaAllocation {
    generation: u64,
    tint_identity: ChunkBiomeTintIdentity,
    cube_range: Option<Range<u32>>,
    cube_lighting_range: Option<Range<u32>>,
    model_range: Option<Range<u32>>,
    model_lighting_range: Option<Range<u32>>,
    model_draw_range: Option<Range<u32>>,
    transparent_model_draw_range: Option<Range<u32>>,
    liquid_range: Option<Range<u32>>,
    liquid_lighting_range: Option<Range<u32>>,
    quad_capacity: u32,
    geometry_stream_range: Option<Range<u32>>,
    geometry_stream_capacity: u32,
    biome_range: Range<u32>,
    biome_capacity: u32,
    gpu: GpuChunkAllocation,
}

#[derive(Clone)]
struct RetiredArenaAllocation {
    entity: Entity,
    identity: GpuChunkAllocation,
    quad: Option<(Range<u32>, u32)>,
    geometry: Option<(Range<u32>, u32)>,
    biome: Option<(Range<u32>, u32)>,
    origin: Option<u32>,
    release_epoch: Option<u64>,
}

impl RetiredArenaAllocation {
    fn geometry_only(entity: Entity, allocation: &ArenaAllocation) -> Option<Self> {
        Some(Self {
            entity,
            identity: allocation.gpu.clone(),
            quad: None,
            geometry: Some((
                allocation.geometry_stream_range.clone()?,
                allocation.geometry_stream_capacity,
            )),
            biome: None,
            origin: None,
            release_epoch: None,
        })
    }

    fn full(entity: Entity, allocation: ArenaAllocation) -> Self {
        let metadata_index = allocation.gpu.metadata_index;
        Self {
            entity,
            identity: allocation.gpu,
            quad: allocation
                .cube_range
                .map(|range| (range, allocation.quad_capacity)),
            geometry: allocation
                .geometry_stream_range
                .map(|range| (range, allocation.geometry_stream_capacity)),
            biome: (allocation.biome_capacity != 0)
                .then_some((allocation.biome_range, allocation.biome_capacity)),
            origin: Some(metadata_index),
            release_epoch: None,
        }
    }

    fn owned_bytes(&self) -> u64 {
        let quad = self
            .quad
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * PACKED_QUAD_BYTES);
        let geometry = self.geometry.as_ref().map_or(0, |(_, capacity)| {
            u64::from(*capacity) * GEOMETRY_STREAM_WORD_BYTES
        });
        let biome = self
            .biome
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * BIOME_WORD_BYTES);
        let origin = self.origin.map_or(0, |_| CHUNK_ORIGIN_BYTES);
        quad.saturating_add(geometry)
            .saturating_add(biome)
            .saturating_add(origin)
    }

    fn can_release(&self, completed_epoch: u64) -> bool {
        self.release_epoch
            .is_some_and(|epoch| epoch <= completed_epoch)
    }
}

impl ArenaAllocation {
    fn expected_streams(&self) -> ChunkStreamMask {
        let mut mask = ChunkStreamMask::default();
        if self.cube_range.is_some() || self.cube_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::CUBE;
        }
        if self.model_range.is_some()
            || self.model_lighting_range.is_some()
            || self.model_draw_range.is_some()
            || self.transparent_model_draw_range.is_some()
        {
            mask = mask | ChunkStreamMask::MODEL;
        }
        if self.liquid_range.is_some() || self.liquid_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::LIQUID;
        }
        mask
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ChunkBindGroupBuffers {
    view: BufferId,
    quads: BufferId,
    origins: BufferId,
    biomes: BufferId,
    materials: BufferId,
    animations: BufferId,
    animation_frames: BufferId,
    animation_clock: BufferId,
    model_templates: BufferId,
    geometry_streams: BufferId,
    transparent_refs: BufferId,
    biome_tints: BufferId,
    atmosphere: BufferId,
    biome_tint_table: ChunkBiomeTintResourceIdentity,
    textures: ChunkTextureAssetIdentity,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialGpu {
    texture: u32,
    flags: u32,
    animation: u32,
}

const _: () = assert!(std::mem::size_of::<MaterialGpu>() == 12);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AnimationGpu {
    frame_start: u32,
    frame_count: u32,
    ticks_per_frame: u32,
    flags: u32,
}

const _: () = assert!(std::mem::size_of::<AnimationGpu>() == 16);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BiomeTintGpu {
    grass: u32,
    foliage: u32,
    birch: u32,
    evergreen: u32,
    dry_foliage: u32,
    water: u32,
    flags: u32,
    _padding: u32,
}

const _: () = assert!(std::mem::size_of::<BiomeTintGpu>() == 32);

fn pack_linear_rgb10(rgb: [f32; 3]) -> u32 {
    let component = |value: f32| {
        if value.is_finite() {
            (value.clamp(0.0, 1.0) * 1023.0).round() as u32
        } else {
            0
        }
    };
    component(rgb[0]) | (component(rgb[1]) << 10) | (component(rgb[2]) << 20)
}

fn prepare_biome_tint_entries(entries: &[BiomeTint]) -> Vec<BiomeTintGpu> {
    entries
        .iter()
        .map(|entry| BiomeTintGpu {
            grass: pack_linear_rgb10(entry.grass),
            foliage: pack_linear_rgb10(entry.foliage),
            birch: pack_linear_rgb10(entry.birch),
            evergreen: pack_linear_rgb10(entry.evergreen),
            dry_foliage: pack_linear_rgb10(entry.dry_foliage),
            water: pack_linear_rgb10(entry.water),
            flags: entry.flags,
            _padding: 0,
        })
        .collect()
}

struct PreparedChunkBiomeTints {
    identity: ChunkBiomeTintResourceIdentity,
    buffer: Buffer,
}

#[derive(Resource, Default)]
struct ChunkGpuBiomeTints {
    prepared: Option<PreparedChunkBiomeTints>,
    _retained_entries: Option<Arc<[BiomeTint]>>,
}

fn biome_tint_gpu_buffer_needs_rebuild(
    current: Option<ChunkBiomeTintResourceIdentity>,
    next: ChunkBiomeTintResourceIdentity,
) -> bool {
    current != Some(next)
}

fn biome_tint_bind_group_needs_rebuild(
    current: Option<ChunkBiomeTintResourceIdentity>,
    next: ChunkBiomeTintResourceIdentity,
) -> bool {
    current != Some(next)
}

fn prepare_chunk_biome_tints(
    render_device: Res<RenderDevice>,
    source: Res<ChunkBiomeTints>,
    mut gpu: ResMut<ChunkGpuBiomeTints>,
) {
    let identity = source.resource_identity();
    if !biome_tint_gpu_buffer_needs_rebuild(
        gpu.prepared.as_ref().map(|prepared| prepared.identity),
        identity,
    ) {
        return;
    }
    let entries = prepare_biome_tint_entries(source.entries());
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("packed chunk biome tints"),
        contents: bytemuck::cast_slice(&entries),
        usage: BufferUsages::STORAGE,
    });
    gpu._retained_entries = Some(Arc::clone(&source.entries));
    gpu.prepared = Some(PreparedChunkBiomeTints { identity, buffer });
}

struct PreparedChunkTextureAssets {
    identity: ChunkTextureAssetIdentity,
    material_buffer: Buffer,
    animation_buffer: Buffer,
    animation_frame_buffer: Buffer,
    model_template_buffer: Buffer,
    _textures: [Texture; 2],
    views: [TextureView; 2],
    sampler: Sampler,
}

#[derive(Resource)]
struct ChunkGpuAnimationClock {
    buffer: Buffer,
}

fn init_chunk_gpu_animation_clock(mut commands: Commands, render_device: Res<RenderDevice>) {
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animation clock"),
        contents: bytemuck::bytes_of(&ChunkAnimationClock::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    commands.insert_resource(ChunkGpuAnimationClock { buffer });
}

fn prepare_chunk_animation_clock(
    clock: Res<ChunkAnimationClock>,
    gpu_clock: Res<ChunkGpuAnimationClock>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(&gpu_clock.buffer, 0, bytemuck::bytes_of(&*clock));
}

#[derive(Resource, Default)]
struct ChunkGpuTextureAssets {
    attempted_identity: Option<ChunkTextureAssetIdentity>,
    _attempted_assets: Option<Arc<RuntimeAssets>>,
    prepared: Option<PreparedChunkTextureAssets>,
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkTextureUploadStats {
    pub upload_count: u64,
    pub material_bytes: u64,
    pub animation_bytes: u64,
    pub animation_frame_bytes: u64,
    pub texture_bytes_including_mips: u64,
    pub padded_upload_bytes: u64,
}

fn prepare_chunk_texture_assets(
    assets: Res<ChunkTextureAssets>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu_assets: ResMut<ChunkGpuTextureAssets>,
    mut stats: ResMut<ChunkTextureUploadStats>,
) {
    let identity = assets.identity();
    if !texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity) {
        return;
    }
    gpu_assets.attempted_identity = Some(identity);
    gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));

    let pages = assets.assets().texture_pages();
    let Some(page_bindings) = plan_texture_page_bindings(pages.len()) else {
        bevy::log::error!(
            page_count = pages.len(),
            "chunk assets require one or two texture pages"
        );
        return;
    };
    let diagnostic_fallback = if page_bindings.contains(&TexturePageBinding::DiagnosticFallback) {
        match diagnostic_texture_page(&pages[0].texture) {
            Ok(texture) => Some(texture),
            Err(error) => {
                bevy::log::error!(?error, "invalid diagnostic texture-page fallback");
                return;
            }
        }
    } else {
        None
    };
    let bound_pages = page_bindings.map(|binding| match binding {
        TexturePageBinding::Asset(index) => &pages[index].texture,
        TexturePageBinding::DiagnosticFallback => diagnostic_fallback
            .as_ref()
            .expect("binding plan includes a diagnostic fallback"),
    });
    let device_limits = render_device.limits();
    if device_limits.max_sampled_textures_per_shader_stage < 2 {
        bevy::log::error!(
            supported = device_limits.max_sampled_textures_per_shader_stage,
            "chunk renderer requires two sampled texture bindings"
        );
        return;
    }
    let limits = TextureArrayLimits {
        max_layers: device_limits.max_texture_array_layers,
        max_dimension_2d: device_limits.max_texture_dimension_2d,
    };
    let mut upload_plans = Vec::with_capacity(2);
    for texture in bound_pages {
        if let Err(error) = limits.validate(texture.layers, assets::TILE_SIZE) {
            bevy::log::error!(?error, "chunk texture page exceeds adapter limits");
            return;
        }
        let plans =
            match plan_texture_mip_uploads(texture, RenderDevice::align_copy_bytes_per_row(1)) {
                Ok(plans) => plans,
                Err(error) => {
                    bevy::log::error!(?error, "invalid chunk texture-page upload layout");
                    return;
                }
            };
        upload_plans.push(plans);
    }

    let material_words = assets
        .assets()
        .materials()
        .iter()
        .map(|material| MaterialGpu {
            texture: material.texture.raw(),
            flags: material.flags,
            animation: material.animation,
        })
        .collect::<Vec<_>>();
    let animation_words = assets
        .assets()
        .animations()
        .iter()
        .map(|animation| AnimationGpu {
            frame_start: animation.frame_start,
            frame_count: animation.frame_count,
            ticks_per_frame: animation.ticks_per_frame,
            flags: animation.flags,
        })
        .collect::<Vec<_>>();
    let animation_frame_words = assets
        .assets()
        .animation_frames()
        .iter()
        .map(|frame| frame.raw())
        .collect::<Vec<_>>();
    let model_template_words = encode_model_template_words(assets.assets());
    let material_bytes = material_words
        .len()
        .saturating_mul(std::mem::size_of::<MaterialGpu>());
    let animation_bytes = animation_words
        .len()
        .saturating_mul(std::mem::size_of::<AnimationGpu>());
    let animation_frame_bytes = animation_frame_words
        .len()
        .saturating_mul(std::mem::size_of::<u32>());
    let model_template_bytes = model_template_words
        .len()
        .saturating_mul(std::mem::size_of::<u32>());
    for (label, bytes) in [
        ("material", material_bytes),
        ("animation", animation_bytes),
        ("animation frame", animation_frame_bytes),
        ("model template", model_template_bytes),
    ] {
        if !storage_table_fits(
            bytes,
            device_limits.max_buffer_size,
            device_limits.max_storage_buffer_binding_size,
        ) {
            bevy::log::error!(label, bytes, "chunk asset table exceeds adapter limits");
            return;
        }
    }
    let material_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk materials"),
        contents: bytemuck::cast_slice(&material_words),
        usage: BufferUsages::STORAGE,
    });
    let animation_sentinel = [AnimationGpu {
        frame_start: 0,
        frame_count: 1,
        ticks_per_frame: 1,
        flags: 0,
    }];
    let animation_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animations"),
        contents: if animation_words.is_empty() {
            bytemuck::cast_slice(&animation_sentinel)
        } else {
            bytemuck::cast_slice(&animation_words)
        },
        usage: BufferUsages::STORAGE,
    });
    let animation_frame_sentinel = [TextureRef::DIAGNOSTIC.raw()];
    let animation_frame_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animation frames"),
        contents: bytemuck::cast_slice(if animation_frame_words.is_empty() {
            &animation_frame_sentinel
        } else {
            &animation_frame_words
        }),
        usage: BufferUsages::STORAGE,
    });
    let model_template_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk model templates"),
        contents: bytemuck::cast_slice(&model_template_words),
        usage: BufferUsages::STORAGE,
    });
    let (texture_0, view_0, padded_0) = upload_texture_page(
        &render_device,
        &render_queue,
        bound_pages[0],
        &upload_plans[0],
        "global chunk texture page 0",
    );
    let (texture_1, view_1, padded_1) = upload_texture_page(
        &render_device,
        &render_queue,
        bound_pages[1],
        &upload_plans[1],
        "global chunk texture page 1",
    );
    let sampler = render_device.create_sampler(&chunk_sampler_descriptor());

    stats.upload_count = 1;
    stats.material_bytes = material_bytes as u64;
    stats.animation_bytes = animation_bytes as u64;
    stats.animation_frame_bytes = animation_frame_bytes as u64;
    stats.texture_bytes_including_mips = bound_pages
        .iter()
        .flat_map(|texture| texture.mips.iter())
        .map(|mip| mip.rgba8.len() as u64)
        .sum();
    stats.padded_upload_bytes = padded_0.saturating_add(padded_1);
    gpu_assets.prepared = Some(PreparedChunkTextureAssets {
        identity,
        material_buffer,
        animation_buffer,
        animation_frame_buffer,
        model_template_buffer,
        _textures: [texture_0, texture_1],
        views: [view_0, view_1],
        sampler,
    });
}

fn chunk_sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("global chunk repeat sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        // Vanilla's native 16x16 texels stay crisp when enlarged. Minification
        // remains linear across the independently generated mip chain to avoid
        // shimmering in distant geometry. Anisotropy stays disabled because
        // wgpu requires linear magnification when anisotropy is greater than 1.
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Linear,
        mipmap_filter: FilterMode::Linear,
        anisotropy_clamp: 1,
        ..Default::default()
    }
}

fn encode_model_template_words(assets: &RuntimeAssets) -> Vec<u32> {
    let template_count = u32::try_from(assets.model_templates().len()).unwrap_or(u32::MAX);
    let mut words = Vec::with_capacity(
        1 + assets.model_templates().len() * 3 + assets.model_quads().len() * 12,
    );
    words.push(template_count);
    for template in assets.model_templates() {
        words.extend([template.quad_start, template.quad_count, template.flags]);
    }
    for quad in assets.model_quads() {
        let mut i16_values = quad.positions.iter().flatten().copied();
        for _ in 0..6 {
            let low = i16_values.next().expect("twelve model position components") as u16;
            let high = i16_values.next().expect("twelve model position components") as u16;
            words.push(u32::from(low) | (u32::from(high) << 16));
        }
        let mut u16_values = quad.uvs.iter().flatten().copied();
        for _ in 0..4 {
            let low = u16_values.next().expect("eight model UV components");
            let high = u16_values.next().expect("eight model UV components");
            words.push(u32::from(low) | (u32::from(high) << 16));
        }
        words.extend([quad.material, quad.flags]);
    }
    words
}

fn storage_table_fits(bytes: usize, max_buffer_size: u64, max_binding_size: u32) -> bool {
    u64::try_from(bytes)
        .is_ok_and(|bytes| bytes <= max_buffer_size && bytes <= u64::from(max_binding_size))
}

fn upload_texture_page(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture_array: &TextureArray,
    upload_plans: &[TextureMipUploadPlan],
    label: &'static str,
) -> (Texture, TextureView, u64) {
    let mip_level_count = u32::try_from(texture_array.mips.len())
        .expect("validated texture pages have a bounded mip count");
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: assets::TILE_SIZE,
            height: assets::TILE_SIZE,
            depth_or_array_layers: texture_array.layers,
        },
        mip_level_count,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut padded_upload_bytes = 0_u64;
    for (mip, plan) in texture_array.mips.iter().zip(upload_plans) {
        let staging = padded_mip_bytes(mip.rgba8.as_ref(), texture_array.layers, plan);
        padded_upload_bytes = padded_upload_bytes.saturating_add(staging.len() as u64);
        render_queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: plan.mip_level,
                origin: Origin3d::default(),
                aspect: Default::default(),
            },
            &staging,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plan.bytes_per_row),
                rows_per_image: Some(plan.rows_per_image),
            },
            Extent3d {
                width: plan.size,
                height: plan.size,
                depth_or_array_layers: texture_array.layers,
            },
        );
    }
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2Array),
        mip_level_count: Some(mip_level_count),
        array_layer_count: Some(texture_array.layers),
        ..Default::default()
    });
    (texture, view, padded_upload_bytes)
}

fn padded_mip_bytes(rgba8: &[u8], layers: u32, plan: &TextureMipUploadPlan) -> Vec<u8> {
    let mut staging = vec![0; plan.staging_bytes];
    let row_bytes = plan.size as usize * 4;
    let padded_row_bytes = plan.bytes_per_row as usize;
    for layer in 0..layers as usize {
        let source_layer = plan.layer_source_offsets[layer];
        let staging_layer = plan.layer_staging_offsets[layer];
        for row in 0..plan.size as usize {
            let source = source_layer + row * row_bytes;
            let destination = staging_layer + row * padded_row_bytes;
            staging[destination..destination + row_bytes]
                .copy_from_slice(&rgba8[source..source + row_bytes]);
        }
    }
    staging
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ChunkGpuUploadStats {
    // Actual packed-quad/origin arena writes for the most recent render frame.
    // Indirect-command uploads are visibility work and are intentionally separate.
    chunk_updates: usize,
    chunk_budget: usize,
    incremental_bytes: u64,
    gpu_copy_bytes: u64,
    full_shadow_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArenaLimits {
    max_quad_items: usize,
    max_geometry_stream_words: usize,
    max_origin_items: usize,
    max_biome_words: usize,
}

fn arena_limits_from_device_limits(
    max_buffer_size: u64,
    max_storage_buffer_binding_size: u64,
) -> ArenaLimits {
    let storage_bytes = max_buffer_size.min(max_storage_buffer_binding_size);
    let max_quad_items = (storage_bytes / PACKED_QUAD_BYTES)
        .min(u64::from(u32::MAX))
        .try_into()
        .unwrap_or(usize::MAX);
    let max_geometry_stream_words = (storage_bytes / GEOMETRY_STREAM_WORD_BYTES)
        .min(u64::from(u32::MAX))
        .try_into()
        .unwrap_or(usize::MAX);
    let max_origin_items = (storage_bytes / CHUNK_ORIGIN_BYTES)
        .min((i32::MAX as u64) / 4)
        .try_into()
        .unwrap_or(usize::MAX);
    let max_biome_words = (storage_bytes / BIOME_WORD_BYTES)
        .min(i32::MAX as u64)
        .try_into()
        .unwrap_or(usize::MAX);
    ArenaLimits {
        max_quad_items,
        max_geometry_stream_words,
        max_origin_items,
        max_biome_words,
    }
}

#[derive(Resource)]
struct ChunkGpuArena {
    quad_buffer: Buffer,
    geometry_stream_buffer: Buffer,
    origin_buffer: Buffer,
    biome_buffer: Buffer,
    index_buffer: Buffer,
    model_index_buffer: Buffer,
    indirect_buffer: Buffer,
    transparent_indirect_buffer: Buffer,
    transparent_ref_buffer: Buffer,
    bind_group: Option<BindGroup>,
    bind_group_buffers: Option<ChunkBindGroupBuffers>,
    quad_capacity: usize,
    geometry_stream_capacity: usize,
    origin_capacity: usize,
    biome_capacity: usize,
    indirect_capacity: usize,
    quad_len: usize,
    geometry_stream_len: usize,
    origin_len: usize,
    biome_len: usize,
    limits: ArenaLimits,
    free_quads: Vec<Range<u32>>,
    free_geometry_stream_words: Vec<Range<u32>>,
    free_origins: Vec<u32>,
    free_biomes: Vec<Range<u32>>,
    allocations: HashMap<Entity, ArenaAllocation>,
    retired_allocations: Vec<RetiredArenaAllocation>,
    pending_removals: BTreeSet<Entity>,
    retirement_budget: TransparentRetirementBudget,
}

fn init_chunk_gpu_arena(mut commands: Commands, render_device: Res<RenderDevice>) {
    commands.insert_resource(ChunkGpuArena::new(&render_device));
}

impl ChunkGpuArena {
    fn new(render_device: &RenderDevice) -> Self {
        let device_limits = render_device.limits();
        let limits = arena_limits_from_device_limits(
            device_limits.max_buffer_size,
            u64::from(device_limits.max_storage_buffer_binding_size),
        );
        Self {
            quad_buffer: create_storage_buffer(
                render_device,
                "packed chunk quads",
                PACKED_QUAD_BYTES,
            ),
            geometry_stream_buffer: create_storage_buffer(
                render_device,
                "packed chunk geometry streams",
                GEOMETRY_STREAM_WORD_BYTES,
            ),
            origin_buffer: create_storage_buffer(
                render_device,
                "packed chunk origins",
                CHUNK_ORIGIN_BYTES,
            ),
            biome_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("packed chunk biome records"),
                contents: bytemuck::cast_slice(&FALLBACK_BIOME_RECORD),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            }),
            index_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared chunk quad indices"),
                contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                usage: BufferUsages::INDEX,
            }),
            model_index_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared model template quad indices"),
                contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                usage: BufferUsages::INDEX,
            }),
            indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_ref_buffer: create_storage_buffer(
                render_device,
                "double-buffered transparent draw refs",
                TRANSPARENT_REF_BUFFER_BYTES as u64,
            ),
            bind_group: None,
            bind_group_buffers: None,
            quad_capacity: 1,
            geometry_stream_capacity: 1,
            origin_capacity: 1,
            biome_capacity: FALLBACK_BIOME_WORDS,
            indirect_capacity: 1,
            quad_len: 0,
            geometry_stream_len: 0,
            origin_len: 0,
            biome_len: FALLBACK_BIOME_WORDS,
            limits,
            free_quads: Vec::new(),
            free_geometry_stream_words: Vec::new(),
            free_origins: Vec::new(),
            free_biomes: Vec::new(),
            allocations: HashMap::new(),
            retired_allocations: Vec::new(),
            pending_removals: BTreeSet::new(),
            retirement_budget: TransparentRetirementBudget::with_limits(
                MAX_TRANSPARENT_RETIRED_ALLOCATIONS,
                MAX_TRANSPARENT_RETIRED_BYTES,
            ),
        }
    }
}

fn create_storage_buffer(render_device: &RenderDevice, label: &'static str, size: u64) -> Buffer {
    render_device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_indirect_buffer(render_device: &RenderDevice, command_capacity: usize) -> Buffer {
    render_device.create_buffer(&BufferDescriptor {
        label: Some("packed chunk indirect commands"),
        size: command_capacity as u64 * INDEXED_INDIRECT_BYTES,
        usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[derive(Debug, Clone, Copy)]
struct GpuUpdateCandidate {
    entity: Entity,
    key: SubChunkKey,
    generation: u64,
    tint_identity: ChunkBiomeTintIdentity,
}

const MAX_GPU_UPDATE_FAIRNESS_ENTRIES: usize = 65_536;
const GPU_UPDATE_OVERDUE_FRAMES: u32 = 2;

#[derive(Resource)]
struct GpuUpdateFairness {
    wait_ages: HashMap<Entity, u32>,
    limit: usize,
}

impl Default for GpuUpdateFairness {
    fn default() -> Self {
        Self::with_limit(MAX_GPU_UPDATE_FAIRNESS_ENTRIES)
    }
}

impl GpuUpdateFairness {
    fn with_limit(limit: usize) -> Self {
        Self {
            wait_ages: HashMap::new(),
            limit,
        }
    }

    fn wait_age(&self, entity: Entity) -> u32 {
        self.wait_ages.get(&entity).copied().unwrap_or(0)
    }

    fn finish_frame(&mut self, active: &[Entity], successful: &[Entity]) {
        let active_set = active.iter().copied().collect::<HashSet<_>>();
        let successful = successful.iter().copied().collect::<HashSet<_>>();
        self.wait_ages
            .retain(|entity, _| active_set.contains(entity));
        for &entity in &successful {
            self.wait_ages.remove(&entity);
        }
        for &entity in active.iter().filter(|entity| !successful.contains(entity)) {
            if let Some(age) = self.wait_ages.get_mut(&entity) {
                *age = age.saturating_add(1);
            } else if self.wait_ages.len() < self.limit {
                self.wait_ages.insert(entity, 1);
            }
        }
    }

    #[cfg(test)]
    fn reset(&mut self) {
        self.wait_ages.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.wait_ages.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.wait_ages.is_empty()
    }
}

const fn chunk_tint_identity_is_active(
    record: ChunkBiomeTintIdentity,
    active: ChunkBiomeTintIdentity,
) -> bool {
    record.stream == active.stream && record.revision == active.revision
}

fn plan_gpu_chunk_updates(
    mut candidates: Vec<GpuUpdateCandidate>,
    allocations: &HashMap<Entity, ArenaAllocation>,
    camera_position: Vec3,
    active_tint_identity: ChunkBiomeTintIdentity,
    fairness: &GpuUpdateFairness,
) -> Vec<Entity> {
    candidates.retain(|candidate| {
        chunk_tint_identity_is_active(candidate.tint_identity, active_tint_identity)
            && allocations.get(&candidate.entity).is_none_or(|allocation| {
                allocation.generation != candidate.generation
                    || allocation.tint_identity != candidate.tint_identity
            })
    });
    candidates.sort_by(|left, right| {
        let left_age = fairness.wait_age(left.entity);
        let right_age = fairness.wait_age(right.entity);
        let left_overdue = left_age >= GPU_UPDATE_OVERDUE_FRAMES;
        let right_overdue = right_age >= GPU_UPDATE_OVERDUE_FRAMES;
        right_overdue
            .cmp(&left_overdue)
            .then_with(|| {
                if left_overdue && right_overdue {
                    right_age.cmp(&left_age)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| {
                ChunkUploadPriority::from_camera(left.key, camera_position)
                    .distance_squared()
                    .total_cmp(
                        &ChunkUploadPriority::from_camera(right.key, camera_position)
                            .distance_squared(),
                    )
            })
            .then_with(|| left.key.cmp(&right.key))
    });
    candidates
        .into_iter()
        .map(|candidate| candidate.entity)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn prepare_gpu_chunks(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    views: Query<&ExtractedView, With<ExtractedCamera>>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    mut arena: ResMut<ChunkGpuArena>,
    budget: Res<ChunkUploadBudget>,
    mut upload_stats: ResMut<ChunkGpuUploadStats>,
    biome_tints: Res<ChunkBiomeTints>,
    texture_assets: Res<ChunkTextureAssets>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    retirement_fence: Res<TransparentRetirementFence>,
    mut fairness: ResMut<GpuUpdateFairness>,
) {
    release_completed_transparent_retirements(&mut arena, retirement_fence.completed_epoch());
    let candidates = instances
        .iter()
        .map(|(entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
        })
        .collect();
    let camera_position = views
        .iter()
        .next()
        .map(|view| view.world_from_view.translation())
        .unwrap_or(Vec3::ZERO);
    let selected = plan_gpu_chunk_updates(
        candidates,
        &arena.allocations,
        camera_position,
        biome_tints.table_identity(),
        &fairness,
    );

    arena.pending_removals.extend(removed_instances.read());
    for entity in arena.pending_removals.iter().copied().collect::<Vec<_>>() {
        let Some(allocation) = arena.allocations.get(&entity).cloned() else {
            arena.pending_removals.remove(&entity);
            continue;
        };
        if allocation.liquid_range.is_none() {
            free_allocation(&mut arena, entity);
            arena.pending_removals.remove(&entity);
            continue;
        }
        let retirement = RetiredArenaAllocation::full(entity, allocation);
        let bytes = retirement.owned_bytes();
        if !arena.retirement_budget.can_reserve(1, bytes) {
            continue;
        }
        arena
            .allocations
            .remove(&entity)
            .expect("pending removal retains its arena allocation");
        assert!(arena.retirement_budget.try_reserve(1, bytes));
        arena.retired_allocations.push(retirement);
        arena.pending_removals.remove(&entity);
    }

    let mut quad_writes = Vec::new();
    let mut model_writes = Vec::new();
    let mut model_lighting_writes = Vec::new();
    let mut model_draw_writes = Vec::new();
    let mut transparent_model_draw_writes = Vec::new();
    let mut liquid_writes = Vec::new();
    let mut liquid_lighting_writes = Vec::new();
    let mut cube_lighting_writes = Vec::new();
    let mut biome_writes = Vec::new();
    let mut origin_writes = Vec::new();
    let mut applied_tokens = Vec::new();
    let mut successful_updates = Vec::new();
    let mut chunk_updates = 0;
    for &entity in &selected {
        if chunk_updates >= budget.max_per_frame {
            break;
        }
        let Ok((_, instance)) = instances.get(entity) else {
            continue;
        };
        if !validate_partitioned_model_streams(
            &instance.model_refs,
            &instance.model_lighting,
            &instance.model_draw_refs,
            &instance.transparent_model_draw_refs,
            texture_assets.assets().model_templates(),
            texture_assets.assets().model_quads(),
            texture_assets.assets().materials(),
        ) {
            bevy::log::error!("sub-chunk model streams are not an exact material partition");
            continue;
        }
        let old = arena.allocations.get(&entity).cloned();
        let required = match u32::try_from(instance.cube_quads.len()) {
            Ok(required) => required,
            Err(_) => {
                bevy::log::error!("sub-chunk mesh exceeds the u32 instance range");
                continue;
            }
        };
        let Ok(cube_lighting_required) = u32::try_from(instance.cube_lighting.len()) else {
            bevy::log::error!("sub-chunk cube-lighting stream exceeds the u32 instance range");
            continue;
        };
        if required != cube_lighting_required {
            bevy::log::error!(
                "sub-chunk cube-lighting count must exactly match the cube-quad count"
            );
            continue;
        }
        let Ok(model_required) = u32::try_from(instance.model_refs.len()) else {
            bevy::log::error!("sub-chunk model stream exceeds the u32 instance range");
            continue;
        };
        let Ok(model_lighting_required) = u32::try_from(instance.model_lighting.len()) else {
            bevy::log::error!("sub-chunk model-lighting stream exceeds the u32 instance range");
            continue;
        };
        let Ok(model_draw_required) = u32::try_from(instance.model_draw_refs.len()) else {
            bevy::log::error!("sub-chunk model-draw stream exceeds the u32 instance range");
            continue;
        };
        let Ok(transparent_model_draw_required) =
            u32::try_from(instance.transparent_model_draw_refs.len())
        else {
            bevy::log::error!(
                "sub-chunk transparent-model-draw stream exceeds the u32 instance range"
            );
            continue;
        };
        let Ok(liquid_required) = u32::try_from(instance.liquid_quads.len()) else {
            bevy::log::error!("sub-chunk liquid stream exceeds the u32 instance range");
            continue;
        };
        let Ok(liquid_lighting_required) = u32::try_from(instance.liquid_lighting.len()) else {
            bevy::log::error!("sub-chunk liquid-lighting stream exceeds the u32 instance range");
            continue;
        };
        let biome_words = if biome_record_is_fallback(&instance.biome) {
            Vec::new()
        } else {
            instance.biome.words().to_vec()
        };
        let biome_required = match u32::try_from(biome_words.len()) {
            Ok(required) => required,
            Err(_) => {
                bevy::log::error!("sub-chunk biome record exceeds the u32 word range");
                continue;
            }
        };
        if old.is_none()
            && arena.free_origins.is_empty()
            && arena.origin_len >= arena.limits.max_origin_items
        {
            bevy::log::warn!("chunk origin arena is at the adapter storage-buffer limit");
            continue;
        }
        if instance
            .token
            .is_some_and(|token| !acknowledgements.try_reserve(instance.key, token))
        {
            continue;
        }
        let stream_counts = GeometryStreamCounts {
            cube: required,
            cube_lighting: cube_lighting_required,
            model: model_required,
            model_lighting: model_lighting_required,
            model_draw: model_draw_required,
            transparent_model_draw: transparent_model_draw_required,
            liquid: liquid_required,
            liquid_lighting: liquid_lighting_required,
        };
        let preserve_old_geometry = old
            .as_ref()
            .is_some_and(|old| transparent_geometry_update_requires_cow(old, stream_counts));
        let retirement = preserve_old_geometry
            .then(|| {
                old.as_ref()
                    .and_then(|old| RetiredArenaAllocation::geometry_only(entity, old))
            })
            .flatten();
        if let Some(retirement) = retirement.as_ref()
            && !arena
                .retirement_budget
                .can_reserve(1, retirement.owned_bytes())
        {
            if let Some(token) = instance.token {
                acknowledgements.cancel(instance.key, token);
            }
            continue;
        }
        let Some(plan) = allocate_for_chunk_update(
            &mut arena,
            stream_counts,
            biome_required,
            old.as_ref(),
            preserve_old_geometry,
        ) else {
            if let Some(token) = instance.token {
                acknowledgements.cancel(instance.key, token);
            }
            bevy::log::warn!("chunk quad arena is at the adapter storage-buffer limit");
            continue;
        };
        if let Some(retirement) = retirement {
            let bytes = retirement.owned_bytes();
            assert!(arena.retirement_budget.try_reserve(1, bytes));
            arena.retired_allocations.push(retirement);
        }
        let metadata_index = match old {
            Some(old) => old.gpu.metadata_index,
            None => allocate_origin(&mut arena)
                .expect("origin capacity was checked before quad allocation"),
        };
        let cube_range = checked_geometry_range(plan.quad_start, required);
        let cube_lighting_range = checked_geometry_range(
            plan.cube_lighting_start,
            cube_lighting_required
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)
                .expect("validated cube-lighting layout fits u32 words"),
        );
        let model_range = checked_geometry_range(
            plan.model_start,
            model_required * (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let model_lighting_range = checked_geometry_range(
            plan.model_lighting_start,
            model_lighting_required
                * (PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let model_draw_range = checked_geometry_range(
            plan.model_draw_start,
            model_draw_required * (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let transparent_model_draw_range = checked_geometry_range(
            plan.transparent_model_draw_start,
            transparent_model_draw_required
                * (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let liquid_range = checked_geometry_range(
            plan.liquid_start,
            liquid_required * (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let liquid_lighting_range = checked_geometry_range(
            plan.liquid_lighting_start,
            liquid_lighting_required
                * (PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let depth_liquid_range = instance.depth_liquid_start.and_then(|local_start| {
            let liquid = liquid_range.as_ref()?;
            let record_start = liquid.start.checked_div(4)?;
            let record_end = liquid.end.checked_div(4)?;
            Some(record_start.checked_add(local_start)?..record_end)
        });
        let quad_range = cube_range
            .clone()
            .unwrap_or(plan.quad_start..plan.quad_start);
        let words = instance
            .cube_quads
            .iter()
            .map(PackedQuad::words)
            .collect::<Vec<_>>();
        let mut model_words = instance
            .model_refs
            .iter()
            .copied()
            .map(PackedModelRef::words)
            .collect::<Vec<_>>();
        absolutize_model_lighting_bases(&mut model_words, plan.model_lighting_start);
        let model_lighting_words = packed_lighting_records(&instance.model_lighting);
        let mut model_draw_words = instance
            .model_draw_refs
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>();
        let mut transparent_model_draw_words = instance
            .transparent_model_draw_refs
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>();
        absolutize_partitioned_model_draw_refs(
            &mut model_draw_words,
            &mut transparent_model_draw_words,
            plan.model_start,
        )
        .expect("validated model draw refs and atomic arena plan fit absolute addressing");
        let mut liquid_words = instance
            .liquid_quads
            .iter()
            .copied()
            .map(PackedLiquidQuad::words)
            .collect::<Vec<_>>();
        absolutize_liquid_lighting_indices(&mut liquid_words, plan.liquid_lighting_start);
        let liquid_lighting_words = packed_lighting_records(&instance.liquid_lighting);
        let cube_lighting_words = packed_lighting_records(&instance.cube_lighting);
        let origin = gpu_chunk_origin(
            instance.origin,
            plan.biome_start,
            plan.quad_start,
            plan.cube_lighting_start,
        )
        .expect("aligned arena layout and bounded biome offset produce a valid origin record");
        quad_writes.push((plan.quad_start, words));
        model_writes.push((plan.model_start, model_words));
        model_lighting_writes.push((plan.model_lighting_start, model_lighting_words));
        model_draw_writes.push((plan.model_draw_start, model_draw_words));
        transparent_model_draw_writes.push((
            plan.transparent_model_draw_start,
            transparent_model_draw_words,
        ));
        liquid_writes.push((plan.liquid_start, liquid_words));
        liquid_lighting_writes.push((plan.liquid_lighting_start, liquid_lighting_words));
        cube_lighting_writes.push((plan.cube_lighting_start, cube_lighting_words));
        if !biome_words.is_empty() {
            biome_writes.push((plan.biome_start, biome_words));
        }
        origin_writes.push((metadata_index, origin));
        let gpu = GpuChunkAllocation {
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            quad_range,
            cube_lighting_range: cube_lighting_range.clone(),
            model_range,
            model_lighting_range,
            model_draw_range,
            transparent_model_draw_range,
            liquid_range,
            liquid_lighting_range,
            has_depth_liquid: instance.has_depth_liquid,
            has_transparent_liquid: instance.has_transparent_liquid,
            depth_liquid_range,
            metadata_index,
        };
        commands.entity(entity).insert(gpu.clone());
        arena.allocations.insert(
            entity,
            ArenaAllocation {
                generation: instance.generation,
                tint_identity: instance.tint_identity,
                cube_range,
                cube_lighting_range,
                model_range: gpu.model_range.clone(),
                model_lighting_range: gpu.model_lighting_range.clone(),
                model_draw_range: gpu.model_draw_range.clone(),
                transparent_model_draw_range: gpu.transparent_model_draw_range.clone(),
                liquid_range: gpu.liquid_range.clone(),
                liquid_lighting_range: gpu.liquid_lighting_range.clone(),
                quad_capacity: plan.quad_capacity,
                geometry_stream_range: checked_geometry_range(
                    plan.geometry_stream_start,
                    GeometryStreamCounts {
                        cube: required,
                        cube_lighting: cube_lighting_required,
                        model: model_required,
                        model_lighting: model_lighting_required,
                        model_draw: model_draw_required,
                        transparent_model_draw: transparent_model_draw_required,
                        liquid: liquid_required,
                        liquid_lighting: liquid_lighting_required,
                    }
                    .shared_word_count()
                    .expect("stream counts were checked before allocation"),
                ),
                geometry_stream_capacity: plan.geometry_stream_capacity,
                biome_range: plan.biome_start..plan.biome_start + biome_required,
                biome_capacity: plan.biome_capacity,
                gpu,
            },
        );
        if let Some(token) = instance.token {
            let uploaded_bytes = buffer_byte_len(instance.cube_quads.len(), PACKED_QUAD_BYTES)
                .saturating_add(buffer_byte_len(
                    instance.cube_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_refs.len(),
                    PACKED_MODEL_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_draw_refs.len(),
                    PACKED_MODEL_DRAW_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.transparent_model_draw_refs.len(),
                    PACKED_MODEL_DRAW_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.liquid_quads.len(),
                    PACKED_LIQUID_QUAD_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.liquid_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(CHUNK_ORIGIN_BYTES)
                .saturating_add(biome_record_byte_len(&instance.biome));
            applied_tokens.push((instance.key, token, uploaded_bytes));
        }
        chunk_updates += 1;
        successful_updates.push(entity);
    }
    fairness.finish_frame(&selected, &successful_updates);

    let quad_incremental_bytes = quad_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_BYTES))
    });
    let stream_incremental_bytes = model_writes
        .iter()
        .fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_REF_BYTES))
        })
        .saturating_add(
            model_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        )
        .saturating_add(model_draw_writes.iter().fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_DRAW_REF_BYTES))
        }))
        .saturating_add(
            transparent_model_draw_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_DRAW_REF_BYTES))
                }),
        )
        .saturating_add(liquid_writes.iter().fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_LIQUID_QUAD_BYTES))
        }))
        .saturating_add(
            liquid_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        )
        .saturating_add(
            cube_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        );
    let origin_incremental_bytes = buffer_byte_len(origin_writes.len(), CHUNK_ORIGIN_BYTES);
    let biome_incremental_bytes = biome_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), BIOME_WORD_BYTES))
    });
    let quad_gpu_copy_bytes = ensure_quad_capacity(&mut arena, &render_device, &render_queue);
    let stream_gpu_copy_bytes =
        ensure_geometry_stream_capacities(&mut arena, &render_device, &render_queue);
    let origin_gpu_copy_bytes = ensure_origin_capacity(&mut arena, &render_device, &render_queue);
    let biome_gpu_copy_bytes = ensure_biome_capacity(&mut arena, &render_device, &render_queue);
    for (offset, words) in quad_writes {
        if !words.is_empty() {
            render_queue.write_buffer(
                &arena.quad_buffer,
                u64::from(offset) * PACKED_QUAD_BYTES,
                bytemuck::cast_slice(&words),
            );
        }
    }
    for (index, origin) in origin_writes {
        render_queue.write_buffer(
            &arena.origin_buffer,
            u64::from(index) * CHUNK_ORIGIN_BYTES,
            bytemuck::bytes_of(&origin),
        );
    }
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_lighting_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_draw_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        transparent_model_draw_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        liquid_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        liquid_lighting_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        cube_lighting_writes,
    );
    for (offset, words) in biome_writes {
        render_queue.write_buffer(
            &arena.biome_buffer,
            u64::from(offset) * BIOME_WORD_BYTES,
            bytemuck::cast_slice(&words),
        );
    }
    let applied_at = Instant::now();
    for (key, token, uploaded_bytes) in applied_tokens {
        acknowledgements.complete_with_bytes(key, token, applied_at, uploaded_bytes);
    }

    *upload_stats = account_chunk_gpu_uploads(
        *budget,
        chunk_updates,
        quad_incremental_bytes.saturating_add(stream_incremental_bytes),
        origin_incremental_bytes,
        biome_incremental_bytes,
        quad_gpu_copy_bytes.saturating_add(stream_gpu_copy_bytes),
        origin_gpu_copy_bytes,
        biome_gpu_copy_bytes,
    );
    if upload_stats.chunk_updates > upload_stats.chunk_budget {
        bevy::log::warn!(
            "chunk GPU preparation observed {} updates despite a {}-chunk upload budget",
            upload_stats.chunk_updates,
            upload_stats.chunk_budget,
        );
    }
}

fn liquid_quad_centroid(chunk_origin: [i32; 3], quad: PackedLiquidQuad) -> [f32; 3] {
    let origin = quad.origin();
    let heights = quad.heights();
    let average_height = heights.into_iter().map(f32::from).sum::<f32>() / (4.0 * 255.0);
    let mut centroid = [
        chunk_origin[0] as f32 + f32::from(origin[0]) + 0.5,
        chunk_origin[1] as f32 + f32::from(origin[1]) + average_height,
        chunk_origin[2] as f32 + f32::from(origin[2]) + 0.5,
    ];
    match quad.face() {
        crate::Face::NegativeX => centroid[0] -= 0.5,
        crate::Face::PositiveX => centroid[0] += 0.5,
        crate::Face::NegativeY => centroid[1] = chunk_origin[1] as f32 + f32::from(origin[1]),
        crate::Face::PositiveY => {}
        crate::Face::NegativeZ => centroid[2] -= 0.5,
        crate::Face::PositiveZ => centroid[2] += 0.5,
    }
    centroid
}

fn transparent_allocation_matches(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    if instance.key != allocation.key
        || instance.generation != allocation.generation
        || instance.tint_identity != allocation.tint_identity
        || allocation.tint_identity != active_tint_identity
        || instance.liquid_quads.len() != instance.liquid_lighting.len()
    {
        return false;
    }
    let (Some(liquid), Some(lighting)) = (
        allocation.liquid_range.as_ref(),
        allocation.liquid_lighting_range.as_ref(),
    ) else {
        return instance.liquid_quads.is_empty();
    };
    liquid.start % 4 == 0
        && liquid.end % 4 == 0
        && lighting.start.is_multiple_of(2)
        && lighting.end.is_multiple_of(2)
        && usize::try_from(liquid.end.saturating_sub(liquid.start)).ok()
            == instance.liquid_quads.len().checked_mul(4)
        && usize::try_from(lighting.end.saturating_sub(lighting.start)).ok()
            == instance.liquid_lighting.len().checked_mul(2)
}

fn packed_stream_range_matches(
    range: Option<&Range<u32>>,
    record_count: usize,
    words_per_record: usize,
) -> bool {
    match range {
        Some(range) => {
            record_count != 0
                && usize::try_from(range.start)
                    .ok()
                    .is_some_and(|start| start.is_multiple_of(words_per_record))
                && usize::try_from(range.end.saturating_sub(range.start)).ok()
                    == record_count.checked_mul(words_per_record)
        }
        None => record_count == 0,
    }
}

fn transparent_model_allocation_matches(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
) -> bool {
    instance.key == allocation.key
        && instance.generation == allocation.generation
        && packed_stream_range_matches(
            allocation.model_range.as_ref(),
            instance.model_refs.len(),
            4,
        )
        && packed_stream_range_matches(
            allocation.model_lighting_range.as_ref(),
            instance.model_lighting.len(),
            2,
        )
        && packed_stream_range_matches(
            allocation.model_draw_range.as_ref(),
            instance.model_draw_refs.len(),
            2,
        )
        && packed_stream_range_matches(
            allocation.transparent_model_draw_range.as_ref(),
            instance.transparent_model_draw_refs.len(),
            2,
        )
}

fn transparent_snapshot_addresses_are_resident<'a, 'b>(
    snapshot: &TransparentOrderedSnapshot,
    resident_allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
    retired_allocations: impl IntoIterator<Item = &'b GpuChunkAllocation>,
    active_asset_identity: ChunkTextureAssetIdentity,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    if snapshot.key.asset_identity != active_asset_identity
        || snapshot.key.tint_identity != active_tint_identity
    {
        return false;
    }
    let resident_allocations = resident_allocations
        .into_iter()
        .filter(|allocation| allocation.tint_identity == active_tint_identity)
        .filter(|allocation| {
            let (Some(liquid), Some(lighting)) = (
                allocation.liquid_range.as_ref(),
                allocation.liquid_lighting_range.as_ref(),
            ) else {
                return false;
            };
            !liquid.is_empty()
                && !lighting.is_empty()
                && liquid.start % 4 == 0
                && liquid.end % 4 == 0
                && lighting.start.is_multiple_of(2)
                && lighting.end.is_multiple_of(2)
                && liquid.end.saturating_sub(liquid.start) / 4
                    == lighting.end.saturating_sub(lighting.start) / 2
        })
        .collect::<Vec<_>>();
    let retired_allocations = retired_allocations
        .into_iter()
        .filter(|allocation| allocation.tint_identity == active_tint_identity)
        .collect::<Vec<_>>();
    snapshot.key.visible_allocations.iter().all(|identity| {
        let active = resident_allocations.iter().any(|allocation| {
            let liquid = allocation
                .liquid_range
                .as_ref()
                .expect("resident allocations retain a liquid range");
            allocation.key == identity.key
                && allocation.metadata_index == identity.metadata_index
                && liquid.start == identity.liquid_range.start
                && liquid.end >= identity.liquid_range.end
        });
        active
            || retired_allocations.iter().any(|allocation| {
                allocation.key == identity.key
                    && allocation.generation == identity.mesh_generation
                    && allocation.metadata_index == identity.metadata_index
                    && allocation.liquid_range.as_ref() == Some(&identity.liquid_range)
                    && allocation.liquid_lighting_range.as_ref() == Some(&identity.lighting_range)
            })
    })
}

fn build_transparent_candidates(
    visible_entities: &RenderVisibleEntities,
    instances: &Query<&ChunkRenderInstance>,
    allocations: &Query<&GpuChunkAllocation>,
    biome_tints: &ChunkBiomeTints,
) -> Result<(Vec<TransparentSortCandidate>, usize), TransparentSortError> {
    let mut candidates = Vec::new();
    let mut distinct_tint_colors = BTreeSet::new();
    for &(entity, _) in visible_entities.get::<ChunkRenderInstance>() {
        let (Ok(instance), Ok(allocation)) = (instances.get(entity), allocations.get(entity))
        else {
            continue;
        };
        if !transparent_allocation_matches(instance, allocation, biome_tints.table_identity()) {
            continue;
        }
        let (Some(liquid_range), Some(_lighting_range)) = (
            allocation.liquid_range.as_ref(),
            allocation.liquid_lighting_range.as_ref(),
        ) else {
            continue;
        };
        let Some(record_start) = liquid_range.start.checked_div(4) else {
            continue;
        };
        let subchunk_center = [
            instance.origin[0] as f32 + 8.0,
            instance.origin[1] as f32 + 8.0,
            instance.origin[2] as f32 + 8.0,
        ];
        let transparent_end = instance
            .depth_liquid_start
            .map_or(instance.liquid_quads.len(), |start| start as usize);
        for (local_index, &quad) in instance.liquid_quads[..transparent_end].iter().enumerate() {
            let local_quad_index =
                u32::try_from(local_index).map_err(|_| TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                })?;
            if candidates.len() == MAX_TRANSPARENT_DRAW_REFS {
                return Err(TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                });
            }
            let liquid_record_index = record_start.checked_add(local_quad_index).ok_or(
                TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                },
            )?;
            let local = quad.origin();
            if let Some(tint_index) = instance.biome.tint_index(local[0], local[1], local[2])
                && let Some(tint) = biome_tints.entries().get(tint_index as usize)
            {
                distinct_tint_colors.insert(tint.water.map(f32::to_bits));
            }
            candidates.push(TransparentSortCandidate::new(
                instance.key,
                local_quad_index,
                liquid_record_index,
                allocation.metadata_index,
                subchunk_center,
                liquid_quad_centroid(instance.origin, quad),
            ));
        }
    }
    validate_transparent_sort_ref_count(candidates.len())?;
    Ok((candidates, distinct_tint_colors.len()))
}

#[allow(clippy::too_many_arguments)]
fn prepare_transparent_sorts(
    views: Query<(Entity, &ExtractedView, &RenderVisibleEntities), With<ExtractedCamera>>,
    instances: Query<&ChunkRenderInstance>,
    diagnostic_instances: Query<(Entity, &ChunkRenderInstance)>,
    allocations: Query<&GpuChunkAllocation>,
    texture_assets: Res<ChunkTextureAssets>,
    biome_tints: Res<ChunkBiomeTints>,
    render_queue: Res<RenderQueue>,
    arena: Res<ChunkGpuArena>,
    mut runtime: ResMut<TransparentSortRuntime>,
    metrics: Res<TransparentSortMetrics>,
    witness_request: Res<TransparentWitnessRequest>,
    witness_evidence: Res<TransparentWitnessEvidence>,
    mut upload_budget: ResMut<TransparentUploadBudget>,
) {
    upload_budget.reset();
    let completed = {
        let receiver = runtime
            .result_receiver
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        receiver.try_recv().ok()
    };
    if let Some(result) = completed {
        let next = runtime.gate.complete(result.generation);
        metrics.update(|snapshot| {
            snapshot.result_generation = result.generation.get();
            snapshot.cpu_duration = result.cpu_duration;
        });
        match result.refs {
            Ok(refs) => {
                let ref_bytes =
                    refs.len() as u64 * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
                let sort_result = TransparentSortResult::new(result.generation, result.key, refs)
                    .expect("worker prevalidates the hard transparent reference ceiling");
                match runtime.state.complete(sort_result) {
                    Ok(true) => {
                        runtime.committed_distinct_tint_count = result.distinct_tint_count;
                        let ref_count = runtime
                            .state
                            .committed()
                            .map_or(0, |snapshot| snapshot.refs().len());
                        runtime.requested_at.remove(&result.generation);
                        let latency = transparent_request_to_commit_latency(
                            result.requested_at,
                            Instant::now(),
                        );
                        runtime
                            .staged_distinct_tint_counts
                            .remove(&result.generation);
                        metrics.update(|snapshot| {
                            snapshot.committed_generation = result.generation.get();
                            snapshot.ref_count = ref_count;
                            snapshot.request_to_commit_latency = latency;
                            snapshot.active_slot_age_frames = 0;
                            snapshot.transparent_water_distinct_tint_count =
                                result.distinct_tint_count;
                        });
                    }
                    Ok(false) => {
                        if runtime.state.staged_ref_count() != 0 {
                            runtime
                                .requested_at
                                .insert(result.generation, result.requested_at);
                            runtime
                                .staged_distinct_tint_counts
                                .insert(result.generation, result.distinct_tint_count);
                            metrics.update(|snapshot| {
                                snapshot.staged_bytes =
                                    snapshot.staged_bytes.saturating_add(ref_bytes);
                            });
                        } else {
                            runtime.requested_at.remove(&result.generation);
                            runtime
                                .staged_distinct_tint_counts
                                .remove(&result.generation);
                            metrics.update(|snapshot| {
                                snapshot.stale_reject_count =
                                    snapshot.stale_reject_count.saturating_add(1);
                            });
                        }
                    }
                    Err(TransparentSortError::ReferenceCeiling { .. }) => {
                        runtime.requested_at.remove(&result.generation);
                        metrics.update(|snapshot| {
                            snapshot.ceiling_reject_count =
                                snapshot.ceiling_reject_count.saturating_add(1);
                        });
                    }
                    Err(TransparentSortError::ConflictingAllocation { .. }) => unreachable!(),
                    Err(TransparentSortError::InvalidCameraTransform) => unreachable!(),
                }
            }
            Err(TransparentSortError::ReferenceCeiling { .. }) => {
                runtime.requested_at.remove(&result.generation);
                metrics.update(|snapshot| {
                    snapshot.ceiling_reject_count = snapshot.ceiling_reject_count.saturating_add(1);
                });
            }
            Err(TransparentSortError::ConflictingAllocation { .. }) => {}
            Err(TransparentSortError::InvalidCameraTransform) => {}
        }
        if let Some((_generation, work)) = next {
            spawn_transparent_sort(runtime.result_sender.clone(), work);
        }
        runtime.prune_request_metadata();
    }

    let mut visible_views = views.iter().collect::<Vec<_>>();
    visible_views.sort_by_key(|(entity, _, _)| *entity);
    if visible_views.len() > MAX_TRANSPARENT_VIEWS {
        bevy::log::warn!(
            "transparent chunk renderer supports one retained 3D view; extra views are rejected"
        );
        visible_views.truncate(MAX_TRANSPARENT_VIEWS);
    }
    let Some((view_entity, view, visible_entities)) = visible_views.into_iter().next() else {
        if runtime.view_entity.is_some() {
            runtime.reset_for_view(None);
            clear_active_transparent_metrics(&metrics);
        }
        return;
    };
    if runtime.view_entity != Some(view_entity) {
        runtime.reset_for_view(Some(view_entity));
        clear_active_transparent_metrics(&metrics);
    }

    let mut manifest = Vec::new();
    for &(entity, _) in visible_entities.get::<ChunkRenderInstance>() {
        let (Ok(instance), Ok(allocation)) = (instances.get(entity), allocations.get(entity))
        else {
            continue;
        };
        if !transparent_allocation_matches(instance, allocation, biome_tints.table_identity()) {
            continue;
        }
        if allocation.has_transparent_liquid
            && let (Some(liquid), Some(lighting)) = (
                allocation.liquid_range.clone(),
                allocation.liquid_lighting_range.clone(),
            )
        {
            manifest.push(TransparentAllocationIdentity::new(
                allocation.key,
                allocation.generation,
                liquid,
                lighting,
                allocation.metadata_index,
            ));
        }
    }
    let world_from_view = view.world_from_view;
    let (_, rotation, translation) = world_from_view.to_scale_rotation_translation();
    let texture_identity = texture_assets.identity();
    let tint_identity = biome_tints.table_identity();
    let key = match ViewSortKey::try_new(
        translation.to_array(),
        rotation.to_array(),
        manifest,
        texture_identity,
        tint_identity,
    ) {
        Ok(key) => key,
        Err(error @ TransparentSortError::ConflictingAllocation { .. })
        | Err(error @ TransparentSortError::InvalidCameraTransform) => {
            fail_closed_transparent_sort_key_error(&mut runtime, &metrics, error);
            return;
        }
        Err(TransparentSortError::ReferenceCeiling { .. }) => unreachable!(),
    };
    if witness_request.enabled() {
        let visible = visible_entities
            .get::<ChunkRenderInstance>()
            .iter()
            .map(|&(entity, _)| entity)
            .collect::<BTreeSet<_>>();
        let committed = runtime.state.committed();
        let records = witness_request
            .keys()
            .iter()
            .copied()
            .map(|required| {
                let found = diagnostic_instances
                    .iter()
                    .find(|(_, instance)| instance.key == required);
                let (entity, instance) = found.unzip();
                let allocation = entity.and_then(|entity| allocations.get(entity).ok());
                TransparentWitnessStageRecord {
                    key: required,
                    extracted_visible: entity.is_some_and(|entity| visible.contains(&entity)),
                    instance_present: instance.is_some(),
                    liquid_quad_count: instance.map_or(0, |instance| instance.liquid_quads.len()),
                    instance_generation: instance.map_or(0, |instance| instance.generation),
                    allocation_present: allocation.is_some(),
                    liquid_range_len: allocation
                        .and_then(|allocation| allocation.liquid_range.as_ref())
                        .map_or(0, |range| range.end.saturating_sub(range.start)),
                    lighting_range_len: allocation
                        .and_then(|allocation| allocation.liquid_lighting_range.as_ref())
                        .map_or(0, |range| range.end.saturating_sub(range.start)),
                    allocation_matches: instance.zip(allocation).is_some_and(
                        |(instance, allocation)| {
                            transparent_allocation_matches(
                                instance,
                                allocation,
                                biome_tints.table_identity(),
                            )
                        },
                    ),
                    committed_member: committed.is_some_and(|snapshot| {
                        snapshot
                            .key()
                            .visible_allocations
                            .iter()
                            .any(|allocation| allocation.key == required)
                    }),
                }
            })
            .collect();
        witness_evidence.record_stage_snapshot(
            witness_request.revision(),
            committed.map_or(0, |snapshot| snapshot.generation().get()),
            records,
        );
    }
    let committed_matches = runtime
        .state
        .committed()
        .is_some_and(|snapshot| snapshot.key() == &key)
        && runtime.state.staged_ref_count() == 0;
    if !committed_matches {
        let had_committed = runtime.state.committed().is_some();
        let committed_addresses_are_resident = runtime.state.committed().is_some_and(|snapshot| {
            snapshot.key.address_identity_eq(&key)
                || transparent_snapshot_addresses_are_resident(
                    snapshot,
                    arena.allocations.values().map(|allocation| &allocation.gpu),
                    arena
                        .retired_allocations
                        .iter()
                        .map(|allocation| &allocation.identity),
                    texture_identity,
                    tint_identity,
                )
        });
        let canceled_staged = runtime.state.staged_generation();
        let generation = runtime
            .state
            .request_retaining_resident_snapshot(&key, committed_addresses_are_resident);
        if had_committed && runtime.state.committed().is_none() {
            runtime.committed_distinct_tint_count = 0;
            metrics.update(|snapshot| {
                snapshot.committed_generation = 0;
                snapshot.encoded_generation = 0;
                snapshot.presented_generation = 0;
                snapshot.ref_count = 0;
                snapshot.active_slot_age_frames = 0;
                snapshot.transparent_water_distinct_tint_count = 0;
            });
        }
        if let Some(canceled) = canceled_staged
            && runtime.state.staged_generation() != Some(canceled)
        {
            runtime.requested_at.remove(&canceled);
            runtime.staged_distinct_tint_counts.remove(&canceled);
        }
        metrics.update(|snapshot| snapshot.request_generation = generation.get());
        if runtime.generation_needs_sort_job(generation) {
            let requested_at = Instant::now();
            match runtime.resolve_candidate_cache(&key, || {
                build_transparent_candidates(
                    visible_entities,
                    &instances,
                    &allocations,
                    &biome_tints,
                )
            }) {
                Ok((candidates, distinct_tint_count)) => {
                    let request = TransparentSortRequest {
                        generation,
                        requested_at,
                        key,
                        view_from_world: Mat4::from(world_from_view.affine().inverse()),
                    };
                    let work = TransparentSortWork {
                        generation: request.generation,
                        requested_at: request.requested_at,
                        key: request.key,
                        view_from_world: request.view_from_world,
                        candidates,
                        distinct_tint_count,
                    };
                    runtime.requested_at.insert(generation, requested_at);
                    let (start, replaced) = runtime.gate.submit_with_replacement(generation, work);
                    if let Some(replaced) = replaced {
                        runtime.requested_at.remove(&replaced);
                        runtime.staged_distinct_tint_counts.remove(&replaced);
                    }
                    if let Some((_generation, work)) = start {
                        spawn_transparent_sort(runtime.result_sender.clone(), work);
                    }
                    runtime.prune_request_metadata();
                }
                Err(TransparentSortError::ReferenceCeiling { .. }) => {
                    metrics.update(|snapshot| {
                        snapshot.ceiling_reject_count =
                            snapshot.ceiling_reject_count.saturating_add(1);
                    });
                }
                Err(TransparentSortError::ConflictingAllocation { .. }) => {}
                Err(TransparentSortError::InvalidCameraTransform) => {}
            }
        }
    }

    let mut uploaded_bytes = 0_u64;
    if let Some(batch) = runtime.state.next_upload_batch() {
        if !upload_budget.consume(batch.refs().len()) {
            bevy::log::error!(
                "transparent water sort batch exceeds the shared per-frame reference upload budget"
            );
            return;
        }
        let offset = u64::try_from(batch.buffer_slot() as usize * TRANSPARENT_REF_SLOT_BYTES)
            .unwrap()
            .saturating_add(
                u64::try_from(
                    batch.ref_range().start * std::mem::size_of::<PackedTransparentDrawRef>(),
                )
                .unwrap(),
            );
        render_queue.write_buffer(
            &arena.transparent_ref_buffer,
            offset,
            bytemuck::cast_slice(batch.refs()),
        );
        uploaded_bytes =
            batch.refs().len() as u64 * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
    }
    if uploaded_bytes != 0 {
        let committed = runtime.state.acknowledge_upload();
        metrics.update(|snapshot| {
            snapshot.upload_bytes = snapshot.upload_bytes.saturating_add(uploaded_bytes);
        });
        if committed
            && let Some((generation, ref_count)) = runtime
                .state
                .committed()
                .map(|snapshot| (snapshot.generation(), snapshot.refs().len()))
        {
            runtime.committed_distinct_tint_count = runtime
                .staged_distinct_tint_counts
                .remove(&generation)
                .unwrap_or_default();
            let requested_at = runtime
                .requested_at
                .remove(&generation)
                .expect("accepted staged generation retains its request timestamp");
            let latency = transparent_request_to_commit_latency(requested_at, Instant::now());
            let tint_count = runtime.committed_distinct_tint_count;
            metrics.update(|current| {
                current.committed_generation = generation.get();
                current.ref_count = ref_count;
                current.request_to_commit_latency = latency;
                current.active_slot_age_frames = 0;
                current.transparent_water_distinct_tint_count = tint_count;
            });
        }
    }
    metrics.update(|snapshot| {
        if runtime.state.committed().is_some() {
            snapshot.active_slot_age_frames = snapshot.active_slot_age_frames.saturating_add(1);
        }
    });
    if let Some((identity, command)) = runtime.state.committed().and_then(|snapshot| {
        Some((
            (snapshot.buffer_slot(), snapshot.refs().len()),
            transparent_indirect_args(snapshot)?,
        ))
    }) && runtime.last_indirect_identity != Some(identity)
    {
        render_queue.write_buffer(
            &arena.transparent_indirect_buffer,
            0,
            bytemuck::bytes_of(&command),
        );
        runtime.last_indirect_identity = Some(identity);
    }
}

fn absolutize_model_lighting_bases(model_refs: &mut [[u32; 4]], lighting_word_start: u32) {
    let lighting_record_base = lighting_word_start / 2;
    for words in model_refs {
        words[2] = words[2]
            .checked_add(lighting_record_base)
            .expect("atomic model-lighting arena plan fits u32 record addressing");
    }
}

#[cfg(test)]
fn validate_local_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    model_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
) -> bool {
    let present = [
        !model_refs.is_empty(),
        !model_lighting.is_empty(),
        !model_draw_refs.is_empty(),
    ];
    if present.iter().any(|&value| value) && !present.iter().all(|&value| value) {
        return false;
    }
    if !present[0] {
        return true;
    }

    let mut draw_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || lighting_base != expected_lighting_base
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;
        let mut visible = words[3];
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(draw_ref) = model_draw_refs.get(draw_index).copied() else {
                return false;
            };
            let draw_words = draw_ref.words();
            if draw_words != [model_ref_index, quad_index]
                || quad_index >= 32
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            draw_index += 1;
            visible &= visible - 1;
        }
    }
    draw_index == model_draw_refs.len() && expected_lighting_base == model_lighting.len()
}

fn validate_partitioned_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    opaque_draw_refs: &[PackedModelDrawRef],
    blend_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
    model_quads: &[assets::ModelQuad],
    materials: &[Material],
) -> bool {
    let any_draw = !opaque_draw_refs.is_empty() || !blend_draw_refs.is_empty();
    if model_refs.is_empty() != model_lighting.is_empty() || model_refs.is_empty() == any_draw {
        return false;
    }
    if model_refs.is_empty() {
        return true;
    }

    let mut opaque_index = 0;
    let mut blend_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        let template_start = template.quad_start as usize;
        let Some(template_end) = template_start.checked_add(template_quad_count) else {
            return false;
        };
        let Some(template_quads) = model_quads.get(template_start..template_end) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || lighting_base != expected_lighting_base
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;

        let mut visible = visible_mask;
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(quad) = template_quads.get(quad_index as usize) else {
                return false;
            };
            let is_blend = if quad.material == assets::DIAGNOSTIC_MATERIAL {
                false
            } else {
                let Some(material) = materials.get(quad.material as usize) else {
                    return false;
                };
                material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND != 0
            };
            let (draw_refs, draw_index) = if is_blend {
                (blend_draw_refs, &mut blend_index)
            } else {
                (opaque_draw_refs, &mut opaque_index)
            };
            let Some(draw_ref) = draw_refs.get(*draw_index).copied() else {
                return false;
            };
            if draw_ref.words() != [model_ref_index, quad_index]
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            *draw_index += 1;
            visible &= visible - 1;
        }
    }
    opaque_index == opaque_draw_refs.len()
        && blend_index == blend_draw_refs.len()
        && expected_lighting_base == model_lighting.len()
}

#[cfg(test)]
fn absolutize_model_draw_refs(draw_refs: &mut [[u32; 2]], model_word_start: u32) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if draw_refs
        .iter()
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in draw_refs {
        words[0] += model_record_base;
    }
    Some(())
}

fn absolutize_partitioned_model_draw_refs(
    opaque_draw_refs: &mut [[u32; 2]],
    blend_draw_refs: &mut [[u32; 2]],
    model_word_start: u32,
) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if opaque_draw_refs
        .iter()
        .chain(blend_draw_refs.iter())
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in opaque_draw_refs
        .iter_mut()
        .chain(blend_draw_refs.iter_mut())
    {
        words[0] += model_record_base;
    }
    Some(())
}

fn absolutize_liquid_lighting_indices(liquid_quads: &mut [[u32; 4]], lighting_word_start: u32) {
    let lighting_record_base = lighting_word_start / 2;
    for words in liquid_quads {
        words[3] = words[3]
            .checked_add(lighting_record_base)
            .expect("atomic liquid-lighting arena plan fits u32 record addressing");
    }
}

#[cfg(test)]
fn packed_light_factor(sample: u16, daylight: f32) -> f32 {
    const CURVE: [f32; 16] = [
        0.0,
        0.017_543_86,
        0.037_037_037,
        0.058_823_53,
        0.083_333_336,
        0.111_111_11,
        0.142_857_15,
        0.179_487_18,
        0.222_222_22,
        0.272_727_28,
        0.333_333_34,
        0.407_407_4,
        0.5,
        0.619_047_64,
        0.777_777_8,
        1.0,
    ];
    let block_light = CURVE[usize::from(sample & 0x0f)];
    let sky_light = CURVE[usize::from((sample >> 4) & 0x0f)] * daylight.clamp(0.0, 1.0);
    let ao = f32::from((sample >> 8) & 0x03);
    block_light.max(sky_light) * (1.0 - ao * 0.12)
}

fn packed_lighting_records(lighting: &[PackedQuadLighting]) -> Vec<[u16; 4]> {
    lighting
        .iter()
        .copied()
        .map(PackedQuadLighting::samples)
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GeometryStreamCounts {
    cube: u32,
    cube_lighting: u32,
    model: u32,
    model_lighting: u32,
    model_draw: u32,
    transparent_model_draw: u32,
    liquid: u32,
    liquid_lighting: u32,
}

const SHARED_GEOMETRY_ALIGNMENT_WORDS: u32 =
    (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeometryStreamLayout {
    model_offset: u32,
    model_lighting_offset: u32,
    model_draw_offset: u32,
    transparent_model_draw_offset: u32,
    liquid_offset: u32,
    liquid_lighting_offset: u32,
    cube_lighting_offset: u32,
    word_count: u32,
}

impl GeometryStreamCounts {
    fn layout(self) -> Option<GeometryStreamLayout> {
        let model_offset = 0;
        let model_lighting_offset = self
            .model
            .checked_mul((PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?;
        let model_lighting_end = model_lighting_offset.checked_add(
            self.model_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        let model_draw_offset = model_lighting_end;
        let model_draw_end = model_draw_offset
            .checked_add(self.model_draw.checked_mul(
                (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
            )?)?;
        let transparent_model_draw_offset = model_draw_end;
        let transparent_model_draw_end = transparent_model_draw_offset
            .checked_add(self.transparent_model_draw.checked_mul(
                (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
            )?)?;
        let liquid_offset = if self.liquid == 0 && self.liquid_lighting == 0 {
            transparent_model_draw_end
        } else {
            checked_align_up(transparent_model_draw_end, SHARED_GEOMETRY_ALIGNMENT_WORDS)?
        };
        let liquid_lighting_offset =
            liquid_offset
                .checked_add(self.liquid.checked_mul(
                    (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
                )?)?;
        let cube_lighting_offset = liquid_lighting_offset.checked_add(
            self.liquid_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        let word_count = cube_lighting_offset.checked_add(
            self.cube_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        Some(GeometryStreamLayout {
            model_offset,
            model_lighting_offset,
            model_draw_offset,
            transparent_model_draw_offset,
            liquid_offset,
            liquid_lighting_offset,
            cube_lighting_offset,
            word_count,
        })
    }

    fn shared_word_count(self) -> Option<u32> {
        Some(self.layout()?.word_count)
    }
}

fn checked_align_up(value: u32, alignment: u32) -> Option<u32> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value & !(alignment - 1))
}

fn transparent_geometry_update_requires_cow(
    old: &ArenaAllocation,
    required: GeometryStreamCounts,
) -> bool {
    if !old.gpu.has_transparent_liquid {
        return false;
    }
    let Some(old_liquid) = old.liquid_range.as_ref() else {
        return false;
    };
    let Some(stream) = old.geometry_stream_range.as_ref() else {
        return true;
    };
    let Some(layout) = required.layout() else {
        return true;
    };
    let Some(liquid_start) = stream.start.checked_add(layout.liquid_offset) else {
        return true;
    };
    let Some(liquid_end) = liquid_start.checked_add(
        required
            .liquid
            .saturating_mul((PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32),
    ) else {
        return true;
    };
    layout.word_count > old.geometry_stream_capacity
        || liquid_start != old_liquid.start
        || liquid_end < old_liquid.end
}

fn allocate_for_chunk_update(
    arena: &mut ChunkGpuArena,
    required: GeometryStreamCounts,
    biome_required: u32,
    old: Option<&ArenaAllocation>,
    preserve_old_geometry: bool,
) -> Option<ChunkRangePlan> {
    let plan = plan_chunk_range_update(
        arena.quad_len,
        &arena.free_quads,
        arena.geometry_stream_len,
        &arena.free_geometry_stream_words,
        arena.biome_len,
        &arena.free_biomes,
        required,
        biome_required,
        old,
        preserve_old_geometry,
        arena.limits,
    )?;
    arena.quad_len = plan.quad_len;
    arena.free_quads = plan.free_quads.clone();
    arena.geometry_stream_len = plan.geometry_stream_len;
    arena.free_geometry_stream_words = plan.free_geometry_stream_words.clone();
    arena.biome_len = plan.biome_len;
    arena.free_biomes = plan.free_biomes.clone();
    Some(plan)
}

fn checked_geometry_range(start: u32, count: u32) -> Option<Range<u32>> {
    if count == 0 {
        return None;
    }
    start.checked_add(count).map(|end| start..end)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChunkRangePlan {
    quad_start: u32,
    quad_capacity: u32,
    geometry_stream_start: u32,
    geometry_stream_capacity: u32,
    model_start: u32,
    model_lighting_start: u32,
    model_draw_start: u32,
    transparent_model_draw_start: u32,
    liquid_start: u32,
    liquid_lighting_start: u32,
    cube_lighting_start: u32,
    biome_start: u32,
    biome_capacity: u32,
    quad_len: usize,
    free_quads: Vec<Range<u32>>,
    geometry_stream_len: usize,
    free_geometry_stream_words: Vec<Range<u32>>,
    biome_len: usize,
    free_biomes: Vec<Range<u32>>,
}

#[allow(clippy::too_many_arguments)]
fn plan_chunk_range_update(
    mut quad_len: usize,
    current_free_quads: &[Range<u32>],
    mut geometry_stream_len: usize,
    current_free_geometry_stream_words: &[Range<u32>],
    mut biome_len: usize,
    current_free_biomes: &[Range<u32>],
    required: GeometryStreamCounts,
    biome_required: u32,
    old: Option<&ArenaAllocation>,
    preserve_old_geometry: bool,
    limits: ArenaLimits,
) -> Option<ChunkRangePlan> {
    let mut free_quads = current_free_quads.to_vec();
    let (quad_start, quad_capacity) = allocate_range_for_update(
        &mut quad_len,
        &mut free_quads,
        required.cube,
        old.and_then(|old| {
            old.cube_range
                .as_ref()
                .map(|range| (range.start, old.quad_capacity))
        }),
        limits.max_quad_items,
        0,
    )?;

    let geometry_layout = required.layout()?;
    let geometry_words_required = geometry_layout.word_count;
    let mut free_geometry_stream_words = current_free_geometry_stream_words.to_vec();
    let (geometry_stream_start, geometry_stream_capacity) = allocate_aligned_range_for_update(
        &mut geometry_stream_len,
        &mut free_geometry_stream_words,
        geometry_words_required,
        (!preserve_old_geometry)
            .then_some(old)
            .flatten()
            .and_then(|old| {
                old.geometry_stream_range
                    .as_ref()
                    .map(|range| (range.start, old.geometry_stream_capacity))
            }),
        limits.max_geometry_stream_words,
        0,
        SHARED_GEOMETRY_ALIGNMENT_WORDS,
    )?;
    let model_start = geometry_stream_start.checked_add(geometry_layout.model_offset)?;
    let model_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.model_lighting_offset)?;
    let model_draw_start = geometry_stream_start.checked_add(geometry_layout.model_draw_offset)?;
    let transparent_model_draw_start =
        geometry_stream_start.checked_add(geometry_layout.transparent_model_draw_offset)?;
    let liquid_start = geometry_stream_start.checked_add(geometry_layout.liquid_offset)?;
    let liquid_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.liquid_lighting_offset)?;
    let cube_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.cube_lighting_offset)?;

    let mut free_biomes = current_free_biomes.to_vec();
    let (biome_start, biome_capacity) = allocate_range_for_update(
        &mut biome_len,
        &mut free_biomes,
        biome_required,
        old.map(|old| (old.biome_range.start, old.biome_capacity)),
        limits.max_biome_words,
        0,
    )?;
    Some(ChunkRangePlan {
        quad_start,
        quad_capacity,
        geometry_stream_start,
        geometry_stream_capacity,
        model_start,
        model_lighting_start,
        model_draw_start,
        transparent_model_draw_start,
        liquid_start,
        liquid_lighting_start,
        cube_lighting_start,
        biome_start,
        biome_capacity,
        quad_len,
        free_quads,
        geometry_stream_len,
        free_geometry_stream_words,
        biome_len,
        free_biomes,
    })
}

fn allocate_range_for_update(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    old: Option<(u32, u32)>,
    max_items: usize,
    empty_start: u32,
) -> Option<(u32, u32)> {
    if let Some((start, capacity)) = old
        && required != 0
        && required <= capacity
    {
        return Some((start, capacity));
    }
    if let Some((start, capacity)) = old
        && capacity != 0
    {
        release_quad_range(len, free, start..start.saturating_add(capacity));
    }
    if required == 0 {
        return Some((empty_start, 0));
    }
    allocate_quad_range(len, free, required, max_items).map(|start| (start, required))
}

#[allow(clippy::too_many_arguments)]
fn allocate_aligned_range_for_update(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    old: Option<(u32, u32)>,
    max_items: usize,
    empty_start: u32,
    alignment: u32,
) -> Option<(u32, u32)> {
    if let Some((start, capacity)) = old
        && required != 0
        && required <= capacity
        && start.is_multiple_of(alignment)
    {
        return Some((start, capacity));
    }
    if let Some((start, capacity)) = old
        && capacity != 0
    {
        release_quad_range(len, free, start..start.saturating_add(capacity));
    }
    if required == 0 {
        return Some((empty_start, 0));
    }
    allocate_aligned_quad_range(len, free, required, max_items, alignment)
        .map(|start| (start, required))
}

fn allocate_aligned_quad_range(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    max_items: usize,
    alignment: u32,
) -> Option<u32> {
    for index in 0..free.len() {
        let start = checked_align_up(free[index].start, alignment)?;
        let end = start.checked_add(required)?;
        if end > free[index].end {
            continue;
        }
        let source = free.remove(index);
        insert_free_quad_range(free, source.start..start);
        insert_free_quad_range(free, end..source.end);
        return Some(start);
    }

    let current = u32::try_from(*len).ok()?;
    let start = checked_align_up(current, alignment)?;
    let end = start.checked_add(required)?;
    if end as usize > max_items {
        return None;
    }
    insert_free_quad_range(free, current..start);
    *len = end as usize;
    Some(start)
}

fn allocate_quad_range(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    max_items: usize,
) -> Option<u32> {
    if let Some(start) = take_free_quad_range(free, required) {
        return Some(start);
    }
    let required = required as usize;
    let next = len.checked_add(required)?;
    if next > max_items || *len > u32::MAX as usize {
        return None;
    }
    let start = *len as u32;
    *len = next;
    Some(start)
}

fn release_quad_range(len: &mut usize, free: &mut Vec<Range<u32>>, freed: Range<u32>) {
    insert_free_quad_range(free, freed);
    while let Some(last) = free.last() {
        if last.end as usize != *len {
            break;
        }
        *len = last.start as usize;
        free.pop();
    }
}

fn insert_free_quad_range(free: &mut Vec<Range<u32>>, mut freed: Range<u32>) {
    if freed.is_empty() {
        return;
    }

    let index = free.partition_point(|range| range.end < freed.start);
    while index < free.len() && free[index].start <= freed.end {
        let adjacent = free.remove(index);
        freed.start = freed.start.min(adjacent.start);
        freed.end = freed.end.max(adjacent.end);
    }
    free.insert(index, freed);
}

fn take_free_quad_range(free: &mut Vec<Range<u32>>, required: u32) -> Option<u32> {
    let index = free
        .iter()
        .position(|range| range.end - range.start >= required)?;
    let start = free[index].start;
    free[index].start += required;
    if free[index].is_empty() {
        free.remove(index);
    }
    Some(start)
}

fn allocate_origin(arena: &mut ChunkGpuArena) -> Option<u32> {
    if let Some(index) = arena.free_origins.pop() {
        return Some(index);
    }
    if arena.origin_len >= arena.limits.max_origin_items || arena.origin_len > u32::MAX as usize {
        return None;
    }
    let index = arena.origin_len as u32;
    arena.origin_len += 1;
    Some(index)
}

fn release_origin(arena: &mut ChunkGpuArena, index: u32) {
    arena.free_origins.push(index);
    while arena.origin_len > 0 {
        let tail = (arena.origin_len - 1) as u32;
        let Some(position) = arena.free_origins.iter().position(|free| *free == tail) else {
            break;
        };
        arena.free_origins.swap_remove(position);
        arena.origin_len -= 1;
    }
}

fn free_allocation(arena: &mut ChunkGpuArena, entity: Entity) {
    if let Some(allocation) = arena.allocations.remove(&entity) {
        if let Some(range) = allocation.cube_range {
            release_quad_range(
                &mut arena.quad_len,
                &mut arena.free_quads,
                range.start..range.start + allocation.quad_capacity,
            );
        }
        if let Some(range) = allocation.geometry_stream_range {
            release_quad_range(
                &mut arena.geometry_stream_len,
                &mut arena.free_geometry_stream_words,
                range.start..range.start + allocation.geometry_stream_capacity,
            );
        }
        if allocation.biome_capacity != 0 {
            let freed = allocation.biome_range.start
                ..allocation.biome_range.start + allocation.biome_capacity;
            release_quad_range(&mut arena.biome_len, &mut arena.free_biomes, freed);
        }
        release_origin(arena, allocation.gpu.metadata_index);
    }
}

fn release_completed_transparent_retirements(arena: &mut ChunkGpuArena, completed_epoch: u64) {
    let mut retained = Vec::with_capacity(arena.retired_allocations.len());
    for retirement in std::mem::take(&mut arena.retired_allocations) {
        if !retirement.can_release(completed_epoch) {
            retained.push(retirement);
            continue;
        }
        let bytes = retirement.owned_bytes();
        if let Some((range, capacity)) = retirement.quad {
            release_quad_range(
                &mut arena.quad_len,
                &mut arena.free_quads,
                range.start..range.start + capacity,
            );
        }
        if let Some((range, capacity)) = retirement.geometry {
            release_quad_range(
                &mut arena.geometry_stream_len,
                &mut arena.free_geometry_stream_words,
                range.start..range.start + capacity,
            );
        }
        if let Some((range, capacity)) = retirement.biome {
            release_quad_range(
                &mut arena.biome_len,
                &mut arena.free_biomes,
                range.start..range.start + capacity,
            );
        }
        if let Some(origin) = retirement.origin {
            release_origin(arena, origin);
        }
        arena.retirement_budget.release(1, bytes);
    }
    arena.retired_allocations = retained;
}

fn buffer_byte_len(item_count: usize, item_bytes: u64) -> u64 {
    u64::try_from(item_count)
        .unwrap_or(u64::MAX)
        .saturating_mul(item_bytes)
}

#[allow(clippy::too_many_arguments)]
fn account_chunk_gpu_uploads(
    budget: ChunkUploadBudget,
    chunk_updates: usize,
    quad_incremental_bytes: u64,
    origin_incremental_bytes: u64,
    biome_incremental_bytes: u64,
    quad_gpu_copy_bytes: u64,
    origin_gpu_copy_bytes: u64,
    biome_gpu_copy_bytes: u64,
) -> ChunkGpuUploadStats {
    let incremental_bytes = quad_incremental_bytes
        .saturating_add(origin_incremental_bytes)
        .saturating_add(biome_incremental_bytes);
    let gpu_copy_bytes = quad_gpu_copy_bytes
        .saturating_add(origin_gpu_copy_bytes)
        .saturating_add(biome_gpu_copy_bytes);
    ChunkGpuUploadStats {
        chunk_updates,
        chunk_budget: budget.max_per_frame,
        incremental_bytes,
        gpu_copy_bytes,
        full_shadow_bytes: 0,
        total_bytes: incremental_bytes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArenaGrowthPlan {
    new_capacity: usize,
    gpu_copy_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArenaGrowthError;

fn plan_arena_growth(
    current_capacity: usize,
    required_len: usize,
    item_bytes: u64,
    max_items: usize,
) -> Result<Option<ArenaGrowthPlan>, ArenaGrowthError> {
    if required_len > max_items {
        return Err(ArenaGrowthError);
    }
    if required_len <= current_capacity {
        return Ok(None);
    }
    let new_capacity = required_len
        .checked_next_power_of_two()
        .unwrap_or(max_items)
        .min(max_items);
    Ok(Some(ArenaGrowthPlan {
        new_capacity,
        gpu_copy_bytes: buffer_byte_len(current_capacity, item_bytes),
    }))
}

fn ensure_quad_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.quad_capacity,
        arena.quad_len,
        PACKED_QUAD_BYTES,
        arena.limits.max_quad_items,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk quads",
        growth.new_capacity as u64 * PACKED_QUAD_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.quad_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.quad_capacity = growth.new_capacity;
    arena.quad_buffer = next;
    growth.gpu_copy_bytes
}

fn ensure_geometry_stream_capacities(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    ensure_stream_capacity(
        &mut arena.geometry_stream_buffer,
        &mut arena.geometry_stream_capacity,
        arena.geometry_stream_len,
        arena.limits.max_geometry_stream_words,
        GEOMETRY_STREAM_WORD_BYTES,
        "packed chunk geometry streams",
        render_device,
        render_queue,
    )
}

#[allow(clippy::too_many_arguments)]
fn ensure_stream_capacity(
    buffer: &mut Buffer,
    capacity: &mut usize,
    required_len: usize,
    max_items: usize,
    item_bytes: u64,
    label: &'static str,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(*capacity, required_len, item_bytes, max_items) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        label,
        growth.new_capacity as u64 * item_bytes,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    *capacity = growth.new_capacity;
    *buffer = next;
    growth.gpu_copy_bytes
}

fn write_stream_records<T: bytemuck::Pod>(
    render_queue: &RenderQueue,
    buffer: &Buffer,
    item_bytes: u64,
    writes: Vec<(u32, Vec<T>)>,
) {
    for (offset, records) in writes {
        if !records.is_empty() {
            render_queue.write_buffer(
                buffer,
                u64::from(offset) * item_bytes,
                bytemuck::cast_slice(&records),
            );
        }
    }
}

fn ensure_origin_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.origin_capacity,
        arena.origin_len,
        CHUNK_ORIGIN_BYTES,
        arena.limits.max_origin_items,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk origins",
        growth.new_capacity as u64 * CHUNK_ORIGIN_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.origin_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.origin_capacity = growth.new_capacity;
    arena.origin_buffer = next;
    growth.gpu_copy_bytes
}

fn ensure_biome_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.biome_capacity,
        arena.biome_len,
        BIOME_WORD_BYTES,
        arena.limits.max_biome_words,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk biome records",
        growth.new_capacity as u64 * BIOME_WORD_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.biome_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.biome_capacity = growth.new_capacity;
    arena.biome_buffer = next;
    growth.gpu_copy_bytes
}

fn copy_gpu_buffer(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    source: &Buffer,
    destination: &Buffer,
    bytes: u64,
) {
    if bytes == 0 {
        return;
    }
    let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("grow packed chunk arena"),
    });
    encoder.copy_buffer_to_buffer(source, 0, destination, 0, bytes);
    render_queue.submit([encoder.finish()]);
}

fn bind_group_needs_rebuild<K: PartialEq>(
    bind_group_exists: bool,
    cached: Option<&K>,
    next: &K,
) -> bool {
    !bind_group_exists || cached != Some(next)
}

#[allow(clippy::too_many_arguments)]
fn prepare_chunk_bind_group(
    pipeline: Res<ChunkPipeline>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    render_device: Res<RenderDevice>,
    texture_assets: Res<ChunkGpuTextureAssets>,
    clock: Res<ChunkGpuAnimationClock>,
    biome_tints: Res<ChunkGpuBiomeTints>,
    atmosphere: Res<AtmosphereGpu>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let Some(texture_assets) = texture_assets.prepared.as_ref() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(view_buffer) = view_uniforms.uniforms.buffer() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(biome_tints) = biome_tints.prepared.as_ref() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let buffers = ChunkBindGroupBuffers {
        view: view_buffer.id(),
        quads: arena.quad_buffer.id(),
        origins: arena.origin_buffer.id(),
        biomes: arena.biome_buffer.id(),
        materials: texture_assets.material_buffer.id(),
        animations: texture_assets.animation_buffer.id(),
        animation_frames: texture_assets.animation_frame_buffer.id(),
        animation_clock: clock.buffer.id(),
        model_templates: texture_assets.model_template_buffer.id(),
        geometry_streams: arena.geometry_stream_buffer.id(),
        transparent_refs: arena.transparent_ref_buffer.id(),
        biome_tints: biome_tints.buffer.id(),
        atmosphere: atmosphere.buffer.id(),
        biome_tint_table: biome_tints.identity,
        textures: texture_assets.identity,
    };
    if !bind_group_needs_rebuild(
        arena.bind_group.is_some(),
        arena.bind_group_buffers.as_ref(),
        &buffers,
    ) && !biome_tint_bind_group_needs_rebuild(
        arena
            .bind_group_buffers
            .as_ref()
            .map(|buffers| buffers.biome_tint_table),
        biome_tints.identity,
    ) {
        return;
    }
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let bind_group = render_device.create_bind_group(
        "shared packed chunk bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: arena.quad_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: arena.origin_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: texture_assets.material_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(&texture_assets.views[0]),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(&texture_assets.views[1]),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::Sampler(&texture_assets.sampler),
            },
            BindGroupEntry {
                binding: 7,
                resource: arena.biome_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 8,
                resource: biome_tints.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 9,
                resource: texture_assets.animation_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 10,
                resource: texture_assets.animation_frame_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 11,
                resource: clock.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 12,
                resource: texture_assets.model_template_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 13,
                resource: arena.geometry_stream_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 14,
                resource: arena.transparent_ref_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 15,
                resource: atmosphere.buffer.as_entire_binding(),
            },
        ],
    );
    arena.bind_group = Some(bind_group);
    arena.bind_group_buffers = Some(buffers);
}

fn drawable_allocation_identity(
    frame_probe: &ActiveFrameProbe,
    entity: Entity,
    allocation: &GpuChunkAllocation,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> Option<FrameAllocationIdentity> {
    if !chunk_tint_identity_is_active(allocation.tint_identity, active_tint_identity) {
        return None;
    }
    let identity = FrameAllocationIdentity {
        entity,
        key: allocation.key,
        generation: allocation.generation,
    };
    frame_probe.accepts(entity, identity).then_some(identity)
}

fn prepare_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = indexed_indirect_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

fn prepare_model_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = model_mdi_draw_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

fn prepare_depth_liquid_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = depth_liquid_mdi_draw_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

#[allow(clippy::too_many_arguments)]
fn prepare_chunk_indirect_batches(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    allocations: Query<&GpuChunkAllocation>,
    biome_tints: Res<ChunkBiomeTints>,
    frame_probe: Res<ActiveFrameProbe>,
    mut batches: ResMut<ChunkIndirectBatches>,
    mut model_batches: ResMut<ChunkModelIndirectBatches>,
    mut depth_liquid_batches: ResMut<ChunkDepthLiquidIndirectBatches>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let mut all_commands = Vec::new();
    for batch in batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }
    for batch in model_batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_model_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }
    for batch in depth_liquid_batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_depth_liquid_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }

    if all_commands.is_empty() {
        return;
    }
    if all_commands.len() > arena.indirect_capacity {
        arena.indirect_capacity = all_commands.len().next_power_of_two();
        arena.indirect_buffer = create_indirect_buffer(&render_device, arena.indirect_capacity);
    }
    render_queue.write_buffer(
        &arena.indirect_buffer,
        0,
        bytemuck::cast_slice(&all_commands),
    );
}

fn sorted_visible_entities<T>(visible: impl IntoIterator<Item = (Entity, T)>) -> Vec<(Entity, T)> {
    let mut visible = visible.into_iter().collect::<Vec<_>>();
    visible.sort_by_key(|(render_entity, _)| *render_entity);
    visible
}

#[allow(clippy::too_many_arguments)]
fn queue_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &RenderVisibleEntities,
        &Msaa,
    )>,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    allocations: Query<&GpuChunkAllocation>,
    arena: Res<ChunkGpuArena>,
    biome_tints: Res<ChunkBiomeTints>,
    mut model_witness_resources: ParamSet<(
        Res<PresentedFrameGate>,
        Res<ModelWitnessRequest>,
        Res<ModelWorkloadMetrics>,
    )>,
    frame_probe: Res<ActiveFrameProbe>,
    mut indirect_batch_sets: ParamSet<(
        ResMut<ChunkIndirectBatches>,
        ResMut<ChunkModelIndirectBatches>,
        ResMut<ChunkDepthLiquidIndirectBatches>,
    )>,
    mut next_tick: Local<Tick>,
    mut unsupported_reported: Local<bool>,
) {
    model_witness_resources
        .p2()
        .begin_frame(summarize_model_workload(allocations.iter()));
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend).contains(Backends::DX12),
        cfg!(debug_assertions),
    );
    let draw_functions = draw_functions.read();
    let direct_draw = draw_functions.id::<DrawChunkCommands>();
    let indirect_draw = draw_functions.id::<DrawChunkIndirectCommands>();
    let model_direct_draw = draw_functions.id::<DrawModelCommands>();
    let model_indirect_draw = draw_functions.id::<DrawModelIndirectCommands>();
    let depth_liquid_direct_draw = draw_functions.id::<DrawDepthLiquidCommands>();
    let depth_liquid_indirect_draw = draw_functions.id::<DrawDepthLiquidIndirectCommands>();
    indirect_batch_sets.p0().0.clear();
    indirect_batch_sets.p1().0.clear();
    indirect_batch_sets.p2().0.clear();
    if draw_mode == ChunkDrawMode::Unsupported {
        frame_probe.clear();
        if !*unsupported_reported {
            bevy::log::error!(
                "packed chunk renderer requires DownlevelFlags::BASE_VERTEX; this adapter is unsupported"
            );
            *unsupported_reported = true;
        }
        return;
    }
    *unsupported_reported = false;
    if let Some(expectation) = model_witness_resources.p0().expectation() {
        let model_witness_request = (*model_witness_resources.p1()).clone();
        frame_probe.begin(FrameProbe::begin_with_model_witness(
            expectation,
            instances
                .iter()
                .map(|(entity, instance)| FrameInstanceIdentity {
                    entity,
                    key: instance.key,
                    generation: instance.generation,
                }),
            arena.allocations.iter().map(|(&entity, allocation)| {
                let model_ref_count = model_ref_count_for_witness(&allocation.gpu);
                (
                    FrameAllocationIdentity {
                        entity,
                        key: allocation.gpu.key,
                        generation: allocation.gpu.generation,
                    },
                    allocation.expected_streams(),
                    model_ref_count,
                )
            }),
            model_witness_request,
        ));
    } else {
        frame_probe.clear();
    }
    for (view_entity, view_main_entity, view, visible_entities, msaa) in &views {
        let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        let Ok(model_pipeline_id) = pipeline.model_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        let Ok(depth_liquid_pipeline_id) = pipeline.depth_liquid_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };

        model_witness_resources
            .p2()
            .record_visible(summarize_model_workload(
                visible_entities
                    .get::<ChunkRenderInstance>()
                    .iter()
                    .filter_map(|(render_entity, _)| {
                        let allocation = allocations.get(*render_entity).ok()?;
                        drawable_allocation_identity(
                            &frame_probe,
                            *render_entity,
                            allocation,
                            biome_tints.table_identity(),
                        )?;
                        Some(allocation)
                    }),
            ));

        if draw_mode == ChunkDrawMode::MultiDrawIndirect {
            let visible = sorted_visible_entities(
                visible_entities
                    .get::<ChunkRenderInstance>()
                    .iter()
                    .copied(),
            )
            .into_iter()
            .filter(|(entity, _)| {
                let Ok(allocation) = allocations.get(*entity) else {
                    return false;
                };
                let Some(identity) = drawable_allocation_identity(
                    &frame_probe,
                    *entity,
                    allocation,
                    biome_tints.table_identity(),
                ) else {
                    return false;
                };
                frame_probe.record_visible(*entity, identity)
            })
            .collect::<Vec<_>>();

            if visible.is_empty() {
                continue;
            }
            indirect_batch_sets.p0().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );
            indirect_batch_sets.p1().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );
            indirect_batch_sets.p2().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );

            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: indirect_draw,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: model_indirect_draw,
                    pipeline: model_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: depth_liquid_indirect_draw,
                    pipeline: depth_liquid_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            continue;
        }

        for &(render_entity, main_entity) in visible_entities.get::<ChunkRenderInstance>() {
            let Ok(allocation) = allocations.get(render_entity) else {
                continue;
            };
            let Some(identity) = drawable_allocation_identity(
                &frame_probe,
                render_entity,
                allocation,
                biome_tints.table_identity(),
            ) else {
                continue;
            };
            if !frame_probe.record_visible(render_entity, identity) {
                continue;
            }
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: direct_draw,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (render_entity, main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            if model_direct_draw_command(allocation).is_some() {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: model_direct_draw,
                        pipeline: model_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        vertex_slab: default(),
                        index_slab: None,
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::invalid().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    *next_tick,
                );
            }
            if depth_liquid_direct_draw_command(allocation).is_some() {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: depth_liquid_direct_draw,
                        pipeline: depth_liquid_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        vertex_slab: default(),
                        index_slab: None,
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::invalid().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    *next_tick,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_transparent_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    mut transparent_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &RenderVisibleEntities,
        &Msaa,
    )>,
    allocations: Query<&GpuChunkAllocation>,
    runtime: Res<TransparentSortRuntime>,
) {
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend).contains(Backends::DX12),
        cfg!(debug_assertions),
    );
    if draw_mode == ChunkDrawMode::Unsupported {
        return;
    }
    let draw_functions = draw_functions.read();
    let transparent_model_draw = draw_functions.id::<DrawTransparentModelCommands>();
    let direct_draw = draw_functions.id::<DrawTransparentLiquidCommands>();
    for (view_entity, main_entity, view, visible_entities, msaa) in &views {
        if runtime.view_entity != Some(view_entity) {
            continue;
        }
        let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
        };
        let rangefinder = view.rangefinder3d();
        if let Ok(model_pipeline_id) = pipeline
            .transparent_model_variants
            .specialize(&pipeline_cache, key)
        {
            for &(render_entity, main_entity) in visible_entities.get::<ChunkRenderInstance>() {
                let Ok(allocation) = allocations.get(render_entity) else {
                    continue;
                };
                if transparent_model_direct_draw_command(allocation).is_none() {
                    continue;
                }
                phase.add(Transparent3d {
                    entity: (render_entity, main_entity),
                    pipeline: model_pipeline_id,
                    draw_function: transparent_model_draw,
                    distance: transparent_model_phase_distance(&rangefinder, allocation.key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: true,
                });
            }
        }

        let Some(snapshot) = runtime.state.committed() else {
            continue;
        };
        if snapshot.refs().is_empty() || runtime.view_entity != Some(view_entity) {
            continue;
        }
        let Some(groups) = transparent_liquid_phase_groups(snapshot) else {
            bevy::log::error!(
                "committed transparent-liquid snapshot is not an exact contiguous sub-chunk partition"
            );
            continue;
        };
        let Ok(pipeline_id) = pipeline.liquid_variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        // Keep each sub-chunk's worker-sorted water refs contiguous while
        // giving water and blend models the same phase-distance contract.
        for group in groups {
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: direct_draw,
                distance: transparent_liquid_phase_distance(&rangefinder, group.key),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                    range: group.ref_range,
                    batch_set_index: None,
                },
                indexed: true,
            });
        }
    }
}

fn transparent_model_subchunk_center(key: SubChunkKey) -> Vec3 {
    let origin = chunk_origin(key);
    Vec3::new(
        origin[0] as f32 + 8.0,
        origin[1] as f32 + 8.0,
        origin[2] as f32 + 8.0,
    )
}

fn transparent_model_phase_distance(rangefinder: &ViewRangefinder3d, key: SubChunkKey) -> f32 {
    rangefinder.distance(&transparent_model_subchunk_center(key))
}

fn transparent_model_draw_candidate(
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
fn sorted_transparent_model_draw_words(
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

fn canonical_transparent_rotation_bits(mut rotation: Quat) -> Option<[u32; 4]> {
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

fn sort_transparent_model_candidates(
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

fn take_transparent_model_upload_batches(
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

fn spawn_transparent_model_sort(
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
fn prepare_transparent_model_sorts(
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

fn transparent_liquid_phase_distance(rangefinder: &ViewRangefinder3d, key: SubChunkKey) -> f32 {
    transparent_model_phase_distance(rangefinder, key)
}

fn transparent_frame_draws(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
) -> Vec<(Entity, FrameAllocationIdentity)> {
    let active = arena
        .allocations
        .iter()
        .map(|(&entity, allocation)| (entity, &allocation.gpu));
    let retired = arena
        .retired_allocations
        .iter()
        .map(|allocation| (allocation.entity, &allocation.identity));
    active
        .chain(retired)
        .filter_map(|(entity, allocation)| {
            transparent_snapshot_references_allocation(snapshot, allocation).then_some((
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            ))
        })
        .collect()
}

fn transparent_frame_draw_for_range(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
    ref_range: Range<u32>,
) -> Option<(Entity, FrameAllocationIdentity)> {
    let start = usize::try_from(ref_range.start).ok()?;
    let end = usize::try_from(ref_range.end).ok()?;
    let refs = snapshot.refs().get(start..end)?;
    let metadata_index = refs.first()?.metadata_index();
    if refs
        .iter()
        .any(|draw_ref| draw_ref.metadata_index() != metadata_index)
    {
        return None;
    }
    let active = arena
        .allocations
        .iter()
        .map(|(&entity, allocation)| (entity, &allocation.gpu));
    let retired = arena
        .retired_allocations
        .iter()
        .map(|allocation| (allocation.entity, &allocation.identity));
    active
        .chain(retired)
        .find(|(_, allocation)| {
            allocation.metadata_index == metadata_index
                && transparent_snapshot_references_allocation(snapshot, allocation)
        })
        .map(|(entity, allocation)| {
            (
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            )
        })
}

fn transparent_snapshot_references_allocation(
    snapshot: &TransparentOrderedSnapshot,
    allocation: &GpuChunkAllocation,
) -> bool {
    snapshot.key.visible_allocations.iter().any(|visible| {
        visible.key == allocation.key
            && visible.mesh_generation == allocation.generation
            && visible.metadata_index == allocation.metadata_index
            && allocation.liquid_range.as_ref() == Some(&visible.liquid_range)
            && allocation.liquid_lighting_range.as_ref() == Some(&visible.lighting_range)
    })
}

#[cfg(test)]
fn transparent_view_key_satisfies_witness(
    key: &ViewSortKey,
    request: &TransparentWitnessRequest,
) -> bool {
    request.enabled()
        && request.keys.iter().all(|required| {
            key.visible_allocations
                .iter()
                .any(|allocation| allocation.key == *required)
        })
}

fn transparent_view_missing_witness_keys(
    key: &ViewSortKey,
    request: &TransparentWitnessRequest,
) -> Vec<SubChunkKey> {
    request
        .keys
        .iter()
        .copied()
        .filter(|required| {
            !key.visible_allocations
                .iter()
                .any(|allocation| allocation.key == *required)
        })
        .collect()
}

fn transparent_retirement_can_arm(
    committed: Option<&TransparentOrderedSnapshot>,
    retired: &GpuChunkAllocation,
) -> bool {
    committed.is_none_or(|snapshot| !transparent_snapshot_references_allocation(snapshot, retired))
}

type DrawChunkCommands = (SetItemPipeline, DrawPackedChunk);
type DrawChunkIndirectCommands = (SetItemPipeline, DrawPackedChunksIndirect);
type DrawModelCommands = (SetItemPipeline, DrawPackedModel);
type DrawModelIndirectCommands = (SetItemPipeline, DrawPackedModelsIndirect);
type DrawTransparentModelCommands = (SetItemPipeline, DrawPackedTransparentModel);
type DrawDepthLiquidCommands = (SetItemPipeline, DrawDepthLiquid);
type DrawDepthLiquidIndirectCommands = (SetItemPipeline, DrawDepthLiquidsIndirect);
type DrawTransparentLiquidCommands = (SetItemPipeline, DrawTransparentLiquid);
type DrawTransparentLiquidIndirectCommands = (SetItemPipeline, DrawTransparentLiquidIndirect);

// Both supported paths use `first_instance` to select packed quad records and
// `base_vertex / 4` to select the per-draw origin. Direct drawing is the
// fallback only on adapters that expose BASE_VERTEX.
struct DrawPackedChunk;

struct DrawTransparentLiquid;

struct DrawDepthLiquid;

impl<P: PhaseItem> RenderCommand<P> for DrawDepthLiquid {
    type Param = (SRes<ChunkGpuArena>, SRes<ActiveFrameProbe>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(command) = depth_liquid_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            command.first_index..command.first_index + command.index_count,
            command.base_vertex,
            command.first_instance..command.first_instance + command.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::LIQUID);
        RenderCommandResult::Success
    }
}

impl RenderCommand<Transparent3d> for DrawTransparentLiquid {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<TransparentSortRuntime>,
        SRes<TransparentSortMetrics>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &Transparent3d,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, runtime, metrics, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let runtime = runtime.into_inner();
        if runtime.view_entity != Some(item.entity()) {
            return RenderCommandResult::Skip;
        }
        let (Some(bind_group), Some(snapshot)) = (&arena.bind_group, runtime.state.committed())
        else {
            return RenderCommandResult::Skip;
        };
        let PhaseItemExtraIndex::IndirectParametersIndex {
            range: ref_range, ..
        } = item.extra_index.clone()
        else {
            return RenderCommandResult::Skip;
        };
        let Some(args) = transparent_draw_range_args(snapshot.buffer_slot(), ref_range.clone())
        else {
            return RenderCommandResult::Skip;
        };
        if args.instance_count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            args.first_index..args.first_index + args.index_count,
            args.base_vertex,
            args.first_instance..args.first_instance + args.instance_count,
        );
        let frame_probe = frame_probe.into_inner();
        if frame_probe.is_active()
            && let Some(draw) = transparent_frame_draw_for_range(snapshot, arena, ref_range)
        {
            frame_probe.record_transparent_draw(snapshot.generation(), [draw]);
        }
        let generation = snapshot.generation().get();
        record_encoded_transparent_generation(metrics.into_inner(), ViewSortGeneration(generation));
        RenderCommandResult::Success
    }
}

struct DrawTransparentLiquidIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawTransparentLiquidIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<TransparentSortRuntime>,
        SRes<TransparentSortMetrics>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, runtime, metrics, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let runtime = runtime.into_inner();
        if runtime.view_entity != Some(item.entity()) {
            return RenderCommandResult::Skip;
        }
        let (Some(bind_group), Some(snapshot)) = (&arena.bind_group, runtime.state.committed())
        else {
            return RenderCommandResult::Skip;
        };
        if snapshot.refs().is_empty() {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed_indirect(&arena.transparent_indirect_buffer, 0);
        let frame_probe = frame_probe.into_inner();
        if frame_probe.is_active() {
            frame_probe.record_transparent_draw(
                snapshot.generation(),
                transparent_frame_draws(snapshot, arena),
            );
        }
        let generation = snapshot.generation().get();
        record_encoded_transparent_generation(metrics.into_inner(), ViewSortGeneration(generation));
        RenderCommandResult::Success
    }
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedChunk {
    type Param = (SRes<ChunkGpuArena>, SRes<ActiveFrameProbe>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(base_vertex) = metadata_base_vertex(allocation.metadata_index) else {
            return RenderCommandResult::Skip;
        };
        let addresses = direct_stream_addresses(allocation);
        if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
            return RenderCommandResult::Skip;
        }
        let Some(cube_range) = addresses.cube.as_ref() else {
            return RenderCommandResult::Skip;
        };
        if cube_lighting_record_address(&addresses, cube_range.start).is_none()
            || cube_lighting_record_address(&addresses, cube_range.end.saturating_sub(1)).is_none()
        {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            0..STATIC_QUAD_INDICES.len() as u32,
            base_vertex,
            cube_range.clone(),
        );
        frame_probe.record_direct_draw(item.entity(), identity);
        RenderCommandResult::Success
    }
}

struct DrawPackedModel;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedModel {
    type Param = (SRes<ChunkGpuArena>, SRes<ActiveFrameProbe>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(draw) = model_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            draw.first_index..draw.first_index + draw.index_count,
            draw.base_vertex,
            draw.first_instance..draw.first_instance + draw.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::MODEL);
        RenderCommandResult::Success
    }
}

struct DrawPackedTransparentModel;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedTransparentModel {
    type Param = (SRes<ChunkGpuArena>, SRes<ActiveFrameProbe>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(draw) = transparent_model_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            draw.first_index..draw.first_index + draw.index_count,
            draw.base_vertex,
            draw.first_instance..draw.first_instance + draw.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::MODEL);
        RenderCommandResult::Success
    }
}

struct DrawPackedChunksIndirect;

struct DrawPackedModelsIndirect;

fn indirect_batch_draw_args(
    batches: &ChunkIndirectBatches,
    item_entity: Entity,
) -> Option<(u64, u32)> {
    let batch = batches.0.get(&item_entity)?;
    (batch.command_count != 0).then_some((batch.indirect_offset, batch.command_count))
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedChunksIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkIndirectBatches>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some((indirect_offset, command_count)) =
            indirect_batch_draw_args(batches, item.entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&arena.indirect_buffer, indirect_offset, command_count);
        if let Some(batch) = batches.0.get(&item.entity()) {
            frame_probe.record_mdi_draws(batch.drawn_allocations.iter().copied());
        }
        RenderCommandResult::Success
    }
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedModelsIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkModelIndirectBatches>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some(batch) = batches.0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        let (indirect_offset, command_count) = (batch.indirect_offset, batch.command_count);
        if command_count == 0 {
            return RenderCommandResult::Skip;
        }
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&arena.indirect_buffer, indirect_offset, command_count);
        frame_probe.record_mdi_streams(
            batch.drawn_allocations.iter().copied(),
            ChunkStreamMask::MODEL,
        );
        RenderCommandResult::Success
    }
}

struct DrawDepthLiquidsIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawDepthLiquidsIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkDepthLiquidIndirectBatches>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some(batch) = batches.0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        if batch.command_count == 0 {
            return RenderCommandResult::Skip;
        }
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(
            &arena.indirect_buffer,
            batch.indirect_offset,
            batch.command_count,
        );
        frame_probe.record_mdi_streams(
            batch.drawn_allocations.iter().copied(),
            ChunkStreamMask::LIQUID,
        );
        RenderCommandResult::Success
    }
}

#[allow(clippy::too_many_arguments)]
fn submit_presented_frame_probe(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    frame_probe: Res<ActiveFrameProbe>,
    presented_frame_gate: Res<PresentedFrameGate>,
    transparent_metrics: Res<TransparentSortMetrics>,
    transparent_fence: Res<TransparentPresentationFence>,
    transparent_runtime: Res<TransparentSortRuntime>,
    mut arena: ResMut<ChunkGpuArena>,
    retirement_fence: Res<TransparentRetirementFence>,
    witness_request: Res<TransparentWitnessRequest>,
    witness_evidence: Res<TransparentWitnessEvidence>,
) {
    let completed_probe = frame_probe.take_completed().and_then(|probe| {
        presented_frame_gate
            .try_reserve_callback(&probe.expectation)
            .then_some(probe)
    });
    let transparent_snapshot = transparent_metrics.snapshot();
    let transparent_generation = (transparent_snapshot.encoded_generation != 0
        && transparent_snapshot.encoded_generation == transparent_snapshot.committed_generation
        && transparent_snapshot.encoded_generation != transparent_snapshot.presented_generation
        && transparent_fence.try_reserve(transparent_snapshot.encoded_generation))
    .then_some(transparent_snapshot.encoded_generation);
    let has_releasable_retirement = arena.retired_allocations.iter().any(|retirement| {
        retirement.release_epoch.is_none()
            && transparent_retirement_can_arm(
                transparent_runtime.state.committed(),
                &retirement.identity,
            )
    });
    let retirement_epoch = has_releasable_retirement
        .then(|| retirement_fence.try_reserve())
        .flatten();
    if let Some(epoch) = retirement_epoch {
        for retirement in &mut arena.retired_allocations {
            if retirement.release_epoch.is_none()
                && transparent_retirement_can_arm(
                    transparent_runtime.state.committed(),
                    &retirement.identity,
                )
            {
                retirement.release_epoch = Some(epoch);
            }
        }
    }
    let witness_generation = transparent_runtime
        .state
        .committed()
        .map_or(0, |snapshot| snapshot.generation().get());
    let witness_missing = transparent_runtime.state.committed().map_or_else(
        || witness_request.keys().to_vec(),
        |snapshot| transparent_view_missing_witness_keys(snapshot.key(), &witness_request),
    );
    let witness_token =
        witness_evidence.try_reserve_missing(&witness_request, witness_generation, witness_missing);
    if completed_probe.is_none()
        && transparent_generation.is_none()
        && retirement_epoch.is_none()
        && witness_token.is_none()
    {
        if let Err(error) = render_device.poll(PollType::Poll) {
            bevy::log::warn!(
                ?error,
                "could not nonblockingly poll presented-frame fences"
            );
        }
        return;
    }
    let present_returned_at = Instant::now();
    let encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("presented frame completion sentinel"),
    });
    let command_buffer = encoder.finish();
    let callback_gate = presented_frame_gate.clone();
    let callback_metrics = transparent_metrics.clone();
    let callback_transparent_fence = transparent_fence.clone();
    let callback_retirement_fence = retirement_fence.clone();
    let callback_witness_evidence = witness_evidence.clone();
    command_buffer.on_submitted_work_done(move || {
        if let Some(completed_probe) = completed_probe {
            callback_gate.publish_reserved_probe(
                completed_probe,
                present_returned_at,
                Instant::now(),
            );
        }
        if let Some(generation) = transparent_generation
            && callback_transparent_fence.complete(generation)
        {
            record_gpu_completed_transparent_generation(&callback_metrics, generation);
        }
        if let Some(epoch) = retirement_epoch {
            callback_retirement_fence.complete(epoch);
        }
        if let Some(token) = witness_token {
            callback_witness_evidence.complete(token);
        }
    });
    render_queue.submit([command_buffer]);
    if let Err(error) = render_device.poll(PollType::Poll) {
        bevy::log::warn!(
            ?error,
            "could not nonblockingly poll presented-frame fences"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{
        mem::size_of,
        sync::{Arc, OnceLock},
    };

    use assets::{
        BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NO_ANIMATION,
        NO_MODEL_TEMPLATE, NetworkIdMode, TextureMip, TexturePage, TextureRef, VisualKind,
        encode_blob,
    };
    use bevy::{
        prelude::*,
        render::render_resource::{DownlevelFlags, DrawIndexedIndirectArgs, WgpuFeatures},
    };
    use world::SubChunk;

    use super::*;

    #[test]
    fn chunk_sampler_keeps_native_texels_crisp_without_discarding_minification_mips() {
        let descriptor = chunk_sampler_descriptor();
        assert_eq!(descriptor.mag_filter, FilterMode::Nearest);
        assert_eq!(descriptor.min_filter, FilterMode::Linear);
        assert_eq!(descriptor.mipmap_filter, FilterMode::Linear);
        assert_eq!(descriptor.anisotropy_clamp, 1);
    }

    #[test]
    fn gpu_binding_and_pipeline_owners_are_global_resources() {
        fn assert_resource<T: Resource>() {}
        assert_resource::<ChunkGpuArena>();
        assert_resource::<ChunkPipeline>();
    }

    #[test]
    fn allocation_is_atomic_across_streams() {
        let free_cube = std::iter::once(0..1).collect::<Vec<_>>();
        let required = GeometryStreamCounts {
            cube: 1,
            cube_lighting: 1,
            model: 1,
            model_lighting: 1,
            model_draw: 1,
            transparent_model_draw: 0,
            liquid: 1,
            liquid_lighting: 1,
        };
        let plan = plan_chunk_range_update(
            1,
            &free_cube,
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 1,
                max_geometry_stream_words: 15,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        );

        assert!(plan.is_none());
        assert_eq!(free_cube.len(), 1);
        assert_eq!(free_cube[0].start, 0);
        assert_eq!(free_cube[0].end, 1);

        let retry = plan_chunk_range_update(
            1,
            &free_cube,
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 1,
                max_geometry_stream_words: 16,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("the unchanged state retries once every stream can fit");
        assert_eq!(retry.quad_start, 0);
        assert_eq!(retry.model_start, 0);
        assert_eq!(retry.model_lighting_start, 4);
        assert_eq!(retry.model_draw_start, 6);
        assert_eq!(retry.liquid_start, 8);
        assert_eq!(retry.liquid_lighting_start, 12);
        assert_eq!(retry.cube_lighting_start, 14);
        assert_eq!(retry.geometry_stream_capacity, 16);

        let empty = plan_chunk_range_update(
            0,
            &[],
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            GeometryStreamCounts::default(),
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 0,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("empty streams need no geometry arena capacity");
        assert_eq!(empty.quad_capacity, 0);
        assert_eq!(empty.geometry_stream_capacity, 0);
    }

    #[test]
    fn shared_geometry_layout_aligns_liquid_records_after_odd_model_lighting() {
        let required = GeometryStreamCounts {
            model: 1,
            model_lighting: 1,
            model_draw: 1,
            liquid: 1,
            liquid_lighting: 1,
            ..Default::default()
        };
        let plan = plan_chunk_range_update(
            0,
            &[],
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 16,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("the aligned shared streams fit the arena");

        assert_eq!(plan.liquid_start % 4, 0);
        assert_eq!(plan.model_draw_start, plan.model_lighting_start + 2);
        assert_eq!(plan.liquid_lighting_start % 2, 0);
    }

    #[test]
    fn shared_geometry_tail_alignment_is_included_in_arena_accounting() {
        let required = GeometryStreamCounts {
            liquid: 1,
            liquid_lighting: 1,
            ..Default::default()
        };
        let plan = plan_chunk_range_update(
            0,
            &[],
            2,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 10,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("two padding words plus six stream words fit exactly");

        assert_eq!(plan.geometry_stream_start, 4);
        assert_eq!(plan.liquid_start, 4);
        assert_eq!(plan.liquid_lighting_start, 8);
        assert_eq!(plan.geometry_stream_capacity, 6);
        assert_eq!(plan.geometry_stream_len, 10);
        assert_eq!(plan.free_geometry_stream_words, vec![2..4]);
    }

    #[test]
    fn aligned_shared_geometry_reuses_an_eligible_old_allocation() {
        let required = GeometryStreamCounts {
            model: 1,
            model_lighting: 1,
            model_draw: 1,
            liquid: 1,
            liquid_lighting: 1,
            ..Default::default()
        };
        let mut old = retirement_test_allocation();
        old.model_range = Some(8..12);
        old.model_lighting_range = Some(12..14);
        old.model_draw_range = Some(14..16);
        old.liquid_range = Some(16..20);
        old.liquid_lighting_range = Some(20..22);
        old.geometry_stream_range = Some(8..22);
        old.geometry_stream_capacity = 14;
        let plan = plan_chunk_range_update(
            0,
            &[],
            22,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            Some(&old),
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 22,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("the aligned old allocation remains reusable");

        assert_eq!(plan.geometry_stream_start, 8);
        assert_eq!(plan.geometry_stream_capacity, 14);
        assert_eq!(plan.liquid_start, 16);
        assert_eq!(plan.geometry_stream_len, 22);
        assert!(plan.free_geometry_stream_words.is_empty());
    }

    #[test]
    fn aligned_shared_geometry_is_transparent_validator_eligible() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let tint = ChunkBiomeTintIdentity::new(2, 3);
        let required = GeometryStreamCounts {
            model: 1,
            model_lighting: 1,
            liquid: 1,
            liquid_lighting: 1,
            ..Default::default()
        };
        let plan = plan_chunk_range_update(
            0,
            &[],
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 14,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("aligned streams fit exactly");
        let instance = ChunkRenderInstance {
            key,
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            model_refs: Arc::from([]),
            model_lighting: Arc::from([]),
            model_draw_refs: Arc::from([]),
            transparent_model_draw_refs: Arc::from([]),
            liquid_quads: Arc::from([PackedLiquidQuad::try_pack(
                [0, 0, 0],
                crate::Face::PositiveY,
                [255; 4],
                1,
                0,
                [0, 0],
                false,
            )
            .unwrap()]),
            liquid_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_start: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity: tint,
            generation: 9,
            token: None,
            origin: [0; 3],
        };
        let allocation = GpuChunkAllocation {
            key,
            generation: 9,
            tint_identity: tint,
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: Some(plan.model_start..plan.model_lighting_start),
            model_lighting_range: Some(plan.model_lighting_start..plan.liquid_start),
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(plan.liquid_start..plan.liquid_start + 4),
            liquid_lighting_range: Some(plan.liquid_lighting_start..plan.liquid_lighting_start + 2),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            metadata_index: 0,
        };

        assert!(transparent_allocation_matches(&instance, &allocation, tint));
    }

    #[test]
    fn presentation_waits_for_expected_stream_mask() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1 << 32 | 1);
        let identity = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let probe = FrameProbe::begin(
            target_expectation(now, [(key, 7)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [(identity, ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID)],
        );

        assert!(probe.record_direct_streams(entity, identity, ChunkStreamMask::MODEL));
        assert!(probe.complete().drawn_manifest.is_empty());

        let probe = FrameProbe::begin(
            target_expectation(now, [(key, 7)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [(identity, ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID)],
        );
        assert!(probe.record_direct_streams(
            entity,
            identity,
            ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID,
        ));
        assert_eq!(probe.complete().drawn_manifest.as_ref(), &[(key, 7)]);
    }

    fn normalize_source_newlines(source: &str) -> String {
        source.replace("\r\n", "\n")
    }

    #[test]
    fn source_parser_normalizes_crlf_for_windows_worktrees() {
        assert_eq!(
            normalize_source_newlines("first\r\nsecond\r\n"),
            "first\nsecond\n"
        );
    }

    #[test]
    fn presentation_completion_uses_keyed_expected_mask_lookup() {
        let source = normalize_source_newlines(include_str!("plugin.rs"));
        let complete = source
            .split_once("    fn complete(self) -> CompletedFrameProbe {")
            .expect("frame probe completion")
            .1
            .split_once("\n    }\n}\n\n#[derive(Default)]")
            .expect("end of frame probe completion")
            .0;

        assert!(
            !complete.contains("self.eligible.values().find_map"),
            "completion must not linearly scan every eligible allocation per drawn identity"
        );
    }

    #[test]
    fn realizable_packed_model_upload_addresses_are_identical_for_direct_and_mdi() {
        let model_quad_counts = [2_u32, 12, 20];
        let required = GeometryStreamCounts {
            model: model_quad_counts.len() as u32,
            model_lighting: model_quad_counts.iter().sum(),
            model_draw: model_quad_counts.iter().sum(),
            ..Default::default()
        };
        let plan = plan_chunk_range_update(
            0,
            &[],
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 192,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("three packed model refs and all lighting sidecars fit");
        let model_range = checked_geometry_range(plan.model_start, required.model * 4).unwrap();
        let model_lighting_range =
            checked_geometry_range(plan.model_lighting_start, required.model_lighting * 2).unwrap();
        let model_draw_range =
            checked_geometry_range(plan.model_draw_start, required.model_draw * 2).unwrap();
        let allocation = GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: Some(model_range.clone()),
            model_lighting_range: Some(model_lighting_range.clone()),
            model_draw_range: Some(model_draw_range.clone()),
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 3,
        };

        let direct_addresses = direct_stream_addresses(&allocation);
        let mdi_addresses = mdi_stream_addresses(&allocation);
        assert_eq!(direct_addresses, mdi_addresses);
        assert_eq!(direct_addresses.model, Some(model_range.clone()));
        assert_eq!(
            direct_addresses.model_lighting,
            Some(model_lighting_range.clone())
        );
        assert_eq!(direct_addresses.model_draw, Some(model_draw_range.clone()));

        let mut refs = Vec::new();
        let mut relative_lighting_base = 0;
        for (template, quad_count) in model_quad_counts.into_iter().enumerate() {
            refs.push([0x888, template as u32, relative_lighting_base, u32::MAX]);
            relative_lighting_base += quad_count;
        }
        absolutize_model_lighting_bases(&mut refs, plan.model_lighting_start);
        let lighting_record_range = model_lighting_range.start / 2..model_lighting_range.end / 2;
        assert_eq!(
            refs.iter().map(|words| words[2]).collect::<Vec<_>>(),
            vec![
                lighting_record_range.start,
                lighting_record_range.start + 2,
                lighting_record_range.start + 14,
            ]
        );
        assert!(
            refs.iter()
                .all(|words| lighting_record_range.contains(&words[2]))
        );

        let direct = model_direct_draw_command(&allocation).expect("direct model draw");
        let mdi = model_mdi_draw_command(&allocation).expect("MDI model draw");
        assert_eq!(direct.index_count, mdi.index_count);
        assert_eq!(direct.instance_count, mdi.instance_count);
        assert_eq!(direct.first_index, mdi.first_index);
        assert_eq!(direct.base_vertex, mdi.base_vertex);
        assert_eq!(direct.first_instance, mdi.first_instance);
        assert_eq!(direct.index_count, 6);
        assert_eq!(
            direct.instance_count,
            model_quad_counts.iter().copied().sum::<u32>()
        );
        assert_eq!(direct.first_instance, model_draw_range.start / 2);
        assert_eq!(direct.base_vertex, 3 * 4);

        for malformed in [
            GpuChunkAllocation {
                model_range: None,
                ..allocation.clone()
            },
            GpuChunkAllocation {
                model_lighting_range: None,
                ..allocation.clone()
            },
            GpuChunkAllocation {
                model_draw_range: None,
                ..allocation.clone()
            },
            GpuChunkAllocation {
                model_draw_range: Some(model_draw_range.start + 2..model_draw_range.end + 2),
                ..allocation.clone()
            },
            GpuChunkAllocation {
                model_draw_range: Some(model_draw_range.start + 1..model_draw_range.end),
                ..allocation.clone()
            },
        ] {
            assert!(model_direct_draw_command(&malformed).is_none());
            assert!(model_mdi_draw_command(&malformed).is_none());
        }
    }

    #[test]
    fn model_lighting_base_patches_to_shared_arena_records_without_mutating_other_words() {
        let mut refs = vec![[0x432, 7, 0, 0b11], [0x765, 8, 2, 0b101]];
        absolutize_model_lighting_bases(&mut refs, 20);
        assert_eq!(refs, [[0x432, 7, 10, 0b11], [0x765, 8, 12, 0b101]]);
    }

    #[test]
    fn model_upload_validation_requires_an_exact_reachable_triplet() {
        let templates = [assets::ModelTemplate {
            quad_start: 0,
            quad_count: 6,
            flags: 0,
        }];
        let refs = [PackedModelRef::new(0x432, 0, 0, 0b10_1101)];
        let lighting = [PackedQuadLighting::new([0; 4]); 6];
        let draws = [
            PackedModelDrawRef::new(0, 0),
            PackedModelDrawRef::new(0, 2),
            PackedModelDrawRef::new(0, 3),
            PackedModelDrawRef::new(0, 5),
        ];
        assert!(validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
        assert!(!validate_local_model_streams(
            &refs,
            &lighting,
            &[],
            &templates
        ));
        assert!(!validate_local_model_streams(
            &[],
            &lighting,
            &draws,
            &templates
        ));
        assert!(!validate_local_model_streams(
            &refs,
            &[],
            &draws,
            &templates
        ));
        assert!(!validate_local_model_streams(
            &refs,
            &lighting,
            &[PackedModelDrawRef::new(1, 0)],
            &templates,
        ));
        assert!(!validate_local_model_streams(
            &refs,
            &lighting,
            &[PackedModelDrawRef::new(0, 32)],
            &templates,
        ));
        assert!(!validate_local_model_streams(
            &refs,
            &lighting,
            &[PackedModelDrawRef::new(0, 1)],
            &templates,
        ));
        assert!(!validate_local_model_streams(
            &[PackedModelRef::new(0x432, 0, 5, 0b10_0000)],
            &lighting,
            &[PackedModelDrawRef::new(0, 5)],
            &templates,
        ));
    }

    #[test]
    fn transparent_model_draw_uses_only_its_exact_partitioned_range() {
        let required = GeometryStreamCounts {
            model: 1,
            model_lighting: 2,
            model_draw: 1,
            transparent_model_draw: 2,
            ..Default::default()
        };
        let plan = plan_chunk_range_update(
            0,
            &[],
            0,
            &[],
            FALLBACK_BIOME_WORDS,
            &[],
            required,
            0,
            None,
            false,
            ArenaLimits {
                max_quad_items: 0,
                max_geometry_stream_words: 32,
                max_origin_items: 1,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .expect("partitioned model upload fits");
        let transparent_range = plan.transparent_model_draw_start
            ..plan.transparent_model_draw_start + required.transparent_model_draw * 2;
        let allocation = GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: Some(plan.model_start..plan.model_lighting_start),
            model_lighting_range: Some(plan.model_lighting_start..plan.model_draw_start),
            model_draw_range: Some(plan.model_draw_start..plan.transparent_model_draw_start),
            transparent_model_draw_range: Some(transparent_range.clone()),
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 3,
        };

        let draw = transparent_model_direct_draw_command(&allocation)
            .expect("transparent model draw command");
        assert_eq!(draw.index_count, MODEL_INDEX_COUNT);
        assert_eq!(draw.instance_count, 2);
        assert_eq!(draw.first_instance, transparent_range.start / 2);
        assert_eq!(draw.base_vertex, 12);
        assert!(
            transparent_model_direct_draw_command(&GpuChunkAllocation {
                transparent_model_draw_range: None,
                ..allocation
            })
            .is_none()
        );
    }

    #[test]
    fn transparent_model_phase_distance_is_monotonic_by_subchunk_center() {
        let rangefinder = bevy::render::render_phase::ViewRangefinder3d::from_world_from_view(
            &bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -1.0)),
        );
        let near = SubChunkKey::new(0, 0, 0, 0);
        let far = SubChunkKey::new(0, 0, 0, 2);

        assert_eq!(transparent_model_subchunk_center(near), Vec3::splat(8.0));
        assert!(
            transparent_model_phase_distance(&rangefinder, far)
                > transparent_model_phase_distance(&rangefinder, near)
        );
    }

    #[test]
    fn transparent_liquid_groups_share_the_model_subchunk_distance_contract() {
        let near =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 1, 8..12, 24..26, 10);
        let far =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 2), 2, 0..8, 20..24, 20);
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![near.clone(), far.clone()],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let snapshot = committed_transparent_state(
            &key,
            vec![
                PackedTransparentDrawRef::new(0, far.metadata_index),
                PackedTransparentDrawRef::new(1, far.metadata_index),
                PackedTransparentDrawRef::new(2, near.metadata_index),
            ],
        )
        .committed()
        .unwrap()
        .clone();

        let groups = transparent_liquid_phase_groups(&snapshot).expect("exact grouped snapshot");
        assert_eq!(
            groups,
            [
                TransparentLiquidPhaseGroup {
                    key: far.key,
                    ref_range: 0..2,
                },
                TransparentLiquidPhaseGroup {
                    key: near.key,
                    ref_range: 2..3,
                },
            ]
        );
        let rangefinder = bevy::render::render_phase::ViewRangefinder3d::from_world_from_view(
            &bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(
            transparent_liquid_phase_distance(&rangefinder, groups[0].key),
            transparent_model_phase_distance(&rangefinder, far.key),
        );
        assert!(
            transparent_liquid_phase_distance(&rangefinder, groups[0].key)
                > transparent_liquid_phase_distance(&rangefinder, groups[1].key)
        );
        assert_eq!(
            transparent_draw_range_args(snapshot.buffer_slot(), groups[0].ref_range.clone()),
            Some(TransparentDrawArgs {
                index_count: 6,
                instance_count: 2,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            })
        );

        let mut non_contiguous = snapshot;
        non_contiguous.refs = Arc::from([
            PackedTransparentDrawRef::new(0, far.metadata_index),
            PackedTransparentDrawRef::new(2, near.metadata_index),
            PackedTransparentDrawRef::new(1, far.metadata_index),
        ]);
        assert!(transparent_liquid_phase_groups(&non_contiguous).is_none());
        assert!(transparent_draw_range_args(0, 0..MAX_TRANSPARENT_DRAW_REFS as u32 + 1).is_none());
    }

    #[test]
    fn transparent_model_face_order_reverses_with_camera_rotation() {
        let model_refs = [PackedModelRef::new(0, 0, 0, 0b11)];
        let draw_refs = [PackedModelDrawRef::new(0, 0), PackedModelDrawRef::new(0, 1)];
        let templates = [assets::ModelTemplate {
            quad_start: 0,
            quad_count: 2,
            flags: 0,
        }];
        let quads = [
            assets::ModelQuad {
                positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            },
            assets::ModelQuad {
                positions: [[0, 0, 256], [0, 256, 256], [256, 256, 256], [256, 0, 256]],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            },
        ];
        let identity_view =
            ViewRangefinder3d::from_world_from_view(&bevy::math::Affine3A::IDENTITY);
        let reversed_view = ViewRangefinder3d::from_world_from_view(
            &bevy::math::Affine3A::from_rotation_translation(
                Quat::from_rotation_y(std::f32::consts::PI),
                Vec3::ZERO,
            ),
        );

        assert_eq!(
            sorted_transparent_model_draw_words(
                &identity_view,
                SubChunkKey::new(0, 0, 0, 0),
                &model_refs,
                &draw_refs,
                &templates,
                &quads,
                5,
            )
            .unwrap(),
            [[5, 0], [5, 1]],
        );
        assert_eq!(
            sorted_transparent_model_draw_words(
                &reversed_view,
                SubChunkKey::new(0, 0, 0, 0),
                &model_refs,
                &draw_refs,
                &templates,
                &quads,
                5,
            )
            .unwrap(),
            [[5, 1], [5, 0]],
        );

        let entity = Entity::from_bits(1);
        let candidates = Arc::from(
            draw_refs
                .iter()
                .copied()
                .enumerate()
                .map(|(stable_index, draw_ref)| {
                    let (centroid, words) = transparent_model_draw_candidate(
                        SubChunkKey::new(0, 0, 0, 0),
                        &model_refs,
                        draw_ref,
                        &templates,
                        &quads,
                        5,
                    )
                    .unwrap();
                    TransparentModelSortCandidate {
                        entity,
                        key: SubChunkKey::new(0, 0, 0, 0),
                        draw_range: 20..24,
                        stable_index: stable_index as u32,
                        centroid,
                        words,
                    }
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            sort_transparent_model_candidates(Mat4::IDENTITY, Arc::clone(&candidates))[0]
                .words
                .as_ref(),
            [[5, 0], [5, 1]],
        );
        assert_eq!(
            sort_transparent_model_candidates(
                Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI)),
                candidates,
            )[0]
            .words
            .as_ref(),
            [[5, 1], [5, 0]],
        );
    }

    #[test]
    fn transparent_model_upload_batches_respect_cap_without_splitting_subchunks() {
        let mut batches = VecDeque::from([
            TransparentModelSortBatch {
                draw_range: 0..6,
                words: vec![[0, 0]; 3].into_boxed_slice(),
            },
            TransparentModelSortBatch {
                draw_range: 6..14,
                words: vec![[1, 0]; 4].into_boxed_slice(),
            },
        ]);

        let first = take_transparent_model_upload_batches(&mut batches, 5);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].draw_range, 0..6);
        assert_eq!(batches.len(), 1);

        let second = take_transparent_model_upload_batches(&mut batches, 5);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].draw_range, 6..14);
        assert!(batches.is_empty());
    }

    #[test]
    fn transparent_model_rotation_cache_key_normalizes_quaternion_sign() {
        let rotation = Quat::from_rotation_y(0.75);
        assert_eq!(
            canonical_transparent_rotation_bits(rotation),
            canonical_transparent_rotation_bits(-rotation),
        );
        assert_ne!(
            canonical_transparent_rotation_bits(rotation),
            canonical_transparent_rotation_bits(Quat::from_rotation_y(1.0)),
        );
    }

    #[test]
    fn shared_geometry_layout_keeps_both_model_draw_routes_contiguous_and_bounded() {
        let layout = GeometryStreamCounts {
            cube: 3,
            cube_lighting: 3,
            model: 1,
            model_lighting: 2,
            model_draw: 1,
            transparent_model_draw: 2,
            liquid: 1,
            liquid_lighting: 1,
        }
        .layout()
        .expect("bounded mixed model streams");

        assert_eq!(layout.model_offset, 0);
        assert_eq!(layout.model_lighting_offset, 4);
        assert_eq!(layout.model_draw_offset, 8);
        assert_eq!(layout.transparent_model_draw_offset, 10);
        assert_eq!(layout.liquid_offset, 16);
        assert_eq!(layout.liquid_lighting_offset, 20);
        assert_eq!(layout.cube_lighting_offset, 22);
        assert_eq!(layout.word_count, 28);
        assert_eq!(layout.cube_lighting_offset % 2, 0);
    }

    #[test]
    fn cube_lighting_layout_rejects_overflow_and_origin_abi_carries_both_bases() {
        assert_eq!(std::mem::size_of::<GpuChunkOrigin>(), 32);
        assert_eq!(CHUNK_ORIGIN_BYTES, 32);
        let origin = gpu_chunk_origin([1, -2, 3], 7, 11, 24).expect("even light word base");
        assert_eq!(origin.value, [1, -2, 3, 7]);
        assert_eq!(origin.cube_bases, [11, 12, 0, 0]);
        assert!(gpu_chunk_origin([0; 3], 0, 0, 3).is_none());
        assert!(
            GeometryStreamCounts {
                cube_lighting: u32::MAX,
                ..default()
            }
            .layout()
            .is_none()
        );
    }

    #[test]
    fn direct_and_mdi_cube_lighting_addresses_resolve_identical_sentinels() {
        let allocation = GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 11..13,
            cube_lighting_range: Some(24..28),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 4,
        };
        let direct = direct_stream_addresses(&allocation);
        let mdi = mdi_stream_addresses(&allocation);
        assert_eq!(direct, mdi);
        assert_eq!(cube_lighting_record_address(&direct, 11), Some(12));
        assert_eq!(cube_lighting_record_address(&mdi, 12), Some(13));
        assert_eq!(cube_lighting_record_address(&direct, 10), None);

        let sentinel = [
            PackedQuadLighting::new([0x0011, 0x0022, 0x0033, 0x0044]),
            PackedQuadLighting::new([0x0111, 0x0222, 0x0333, 0x0444]),
        ];
        let local = cube_lighting_record_address(&direct, 12).unwrap() - 12;
        assert_eq!(sentinel[local as usize].samples()[3], 0x0444);

        let packed = packed_lighting_records(&sentinel);
        let layout = GeometryStreamCounts {
            cube: 2,
            cube_lighting: 2,
            liquid: 1,
            liquid_lighting: 1,
            ..default()
        }
        .layout()
        .unwrap();
        assert_eq!(layout.cube_lighting_offset, 6);
        let mut arena_words = vec![0_u32; layout.word_count as usize];
        let packed_words: &[u32] = bytemuck::cast_slice(&packed);
        arena_words[layout.cube_lighting_offset as usize..].copy_from_slice(packed_words);
        let record = cube_lighting_record_address(
            &StreamAddresses {
                cube: Some(11..13),
                cube_lighting: Some(6..10),
                ..default()
            },
            12,
        )
        .unwrap();
        let word = arena_words[(record * 2 + 1) as usize];
        assert_eq!(word >> 16, 0x0444);
    }

    #[test]
    fn cube_draws_reject_missing_odd_mismatched_and_overlapping_lighting_ranges() {
        let valid = GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 11..13,
            cube_lighting_range: Some(24..28),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 4,
        };
        assert!(indexed_indirect_command(&valid).is_some());

        let mut missing = valid.clone();
        missing.cube_lighting_range = None;
        assert!(indexed_indirect_command(&missing).is_none());

        let mut odd = valid.clone();
        odd.cube_lighting_range = Some(25..29);
        assert!(indexed_indirect_command(&odd).is_none());

        let mut mismatched = valid.clone();
        mismatched.cube_lighting_range = Some(24..26);
        assert!(indexed_indirect_command(&mismatched).is_none());

        let mut overlapping = valid;
        overlapping.model_range = Some(26..30);
        assert!(indexed_indirect_command(&overlapping).is_none());
    }

    #[test]
    fn model_upload_validation_enforces_exact_material_partition() {
        let templates = [assets::ModelTemplate {
            quad_start: 0,
            quad_count: 2,
            flags: 0,
        }];
        let quads = [
            assets::ModelQuad {
                positions: [[0; 3]; 4],
                uvs: [[0; 2]; 4],
                material: assets::DIAGNOSTIC_MATERIAL,
                flags: 0,
            },
            assets::ModelQuad {
                positions: [[0; 3]; 4],
                uvs: [[0; 2]; 4],
                material: 1,
                flags: 0,
            },
        ];
        let materials = [
            assets::Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            },
            assets::Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: assets::MATERIAL_FLAG_ALPHA_BLEND,
                animation: NO_ANIMATION,
            },
        ];
        let refs = [PackedModelRef::new(0x432, 0, 0, 0b11)];
        let lighting = [PackedQuadLighting::new([0; 4]); 2];
        let opaque = [PackedModelDrawRef::new(0, 0)];
        let blend = [PackedModelDrawRef::new(0, 1)];

        assert!(validate_partitioned_model_streams(
            &refs, &lighting, &opaque, &blend, &templates, &quads, &materials,
        ));
        assert!(!validate_partitioned_model_streams(
            &refs, &lighting, &blend, &opaque, &templates, &quads, &materials,
        ));
        assert!(!validate_partitioned_model_streams(
            &refs,
            &lighting,
            &opaque,
            &[],
            &templates,
            &quads,
            &materials,
        ));
    }

    fn owned_model_stream_fixture() -> (
        Vec<PackedModelRef>,
        Vec<PackedQuadLighting>,
        Vec<PackedModelDrawRef>,
        Vec<assets::ModelTemplate>,
    ) {
        (
            vec![
                PackedModelRef::new(0x111, 0, 0, 0b10_1101),
                PackedModelRef::new(0x222, 1, 6, 0b10),
            ],
            vec![PackedQuadLighting::new([0; 4]); 8],
            vec![
                PackedModelDrawRef::new(0, 0),
                PackedModelDrawRef::new(0, 2),
                PackedModelDrawRef::new(0, 3),
                PackedModelDrawRef::new(0, 5),
                PackedModelDrawRef::new(1, 1),
            ],
            vec![
                assets::ModelTemplate {
                    quad_start: 0,
                    quad_count: 6,
                    flags: 0,
                },
                assets::ModelTemplate {
                    quad_start: 6,
                    quad_count: 2,
                    flags: 0,
                },
            ],
        )
    }

    #[test]
    fn partial_masks_own_full_contiguous_template_lighting() {
        let (refs, lighting, draws, templates) = owned_model_stream_fixture();
        assert!(validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_duplicate_lighting_base() {
        let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
        refs[1] = PackedModelRef::new(0x222, 1, 0, 0b10);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_first_lighting_base_gap() {
        let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
        refs[0] = PackedModelRef::new(0x111, 0, 1, 0b10_1101);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_middle_lighting_gap() {
        let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
        refs[1] = PackedModelRef::new(0x222, 1, 7, 0b10);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_trailing_unreachable_lighting() {
        let (refs, mut lighting, draws, templates) = owned_model_stream_fixture();
        lighting.push(PackedQuadLighting::new([0; 4]));
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_cross_model_lighting_overlap() {
        let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
        refs[1] = PackedModelRef::new(0x222, 1, 5, 0b10);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_cross_model_draw_read() {
        let (refs, lighting, mut draws, templates) = owned_model_stream_fixture();
        draws[3] = PackedModelDrawRef::new(1, 1);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_upload_rejects_zero_mask_ref_beside_drawable_ref() {
        let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
        refs[0] = PackedModelRef::new(0x111, 0, 0, 0);
        assert!(!validate_local_model_streams(
            &refs, &lighting, &draws, &templates
        ));
    }

    #[test]
    fn model_draw_absolutization_changes_only_the_model_index() {
        let mut draws = vec![[0, 2], [3, 31]];
        absolutize_model_draw_refs(&mut draws, 20).expect("aligned model record base");
        assert_eq!(draws, [[5, 2], [8, 31]]);
        assert!(absolutize_model_draw_refs(&mut draws, 22).is_none());
    }

    #[test]
    fn partitioned_model_draw_absolutization_preserves_both_exact_routes() {
        let mut opaque = vec![[0, 0], [2, 7]];
        let mut blend = vec![[0, 1], [2, 8]];

        absolutize_partitioned_model_draw_refs(&mut opaque, &mut blend, 20)
            .expect("aligned shared model base");

        assert_eq!(opaque, [[5, 0], [7, 7]]);
        assert_eq!(blend, [[5, 1], [7, 8]]);
        assert!(absolutize_partitioned_model_draw_refs(&mut opaque, &mut blend, 22).is_none());
    }

    #[test]
    fn production_model_witness_counts_refs_not_exact_draw_records() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1 << 32 | 81);
        let mut allocation = retirement_test_allocation();
        allocation.gpu.key = key;
        allocation.gpu.generation = 9;
        allocation.gpu.model_range = Some(0..8);
        allocation.gpu.model_lighting_range = Some(8..20);
        allocation.gpu.model_draw_range = Some(20..40);
        allocation.model_range = allocation.gpu.model_range.clone();
        allocation.model_lighting_range = allocation.gpu.model_lighting_range.clone();
        allocation.model_draw_range = allocation.gpu.model_draw_range.clone();

        let model_ref_count = model_ref_count_for_witness(&allocation.gpu);
        let model_draw_count = allocation.gpu.model_draw_range.as_ref().unwrap().len() / 2;
        assert_eq!(model_ref_count, 2);
        assert_eq!(model_draw_count, 10);

        let identity = FrameAllocationIdentity {
            entity,
            key,
            generation: 9,
        };
        let request = ModelWitnessRequest::try_new(7, [0x81; 32], vec![key]).unwrap();
        let probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 9,
            }],
            [(identity, allocation.expected_streams(), model_ref_count)],
            request,
        );
        assert!(probe.record_direct_streams(entity, identity, ChunkStreamMask::MODEL));
        let witness = probe.complete().model_witness.unwrap();
        assert_eq!(witness.total_model_ref_count, 2);
        assert_eq!(witness.manifest[0].model_ref_count, 2);
    }

    #[test]
    fn model_workload_counts_refs_exact_draws_and_avoided_fixed_slots() {
        let mut first = retirement_test_allocation().gpu;
        first.model_range = Some(0..8);
        first.model_lighting_range = Some(8..20);
        first.model_draw_range = Some(20..40);

        let mut second = retirement_test_allocation().gpu;
        second.model_range = Some(64..68);
        second.model_lighting_range = Some(68..74);
        second.model_draw_range = Some(74..82);

        let snapshot = summarize_model_workload([&first, &second]);
        assert_eq!(snapshot.model_ref_count, 3);
        assert_eq!(snapshot.model_draw_ref_count, 14);
        assert_eq!(snapshot.legacy_fixed_slot_quad_invocations_avoided, 82);
    }

    #[test]
    fn model_workload_ignores_allocations_without_a_complete_model_stream() {
        let mut missing_draws = retirement_test_allocation().gpu;
        missing_draws.model_range = Some(0..4);
        missing_draws.model_lighting_range = Some(4..10);
        missing_draws.model_draw_range = None;

        let empty = retirement_test_allocation().gpu;
        assert_eq!(
            summarize_model_workload([&missing_draws, &empty]),
            ModelWorkloadCount::default()
        );
    }

    #[test]
    fn model_workload_counts_transparent_only_model_draws() {
        let mut allocation = retirement_test_allocation().gpu;
        allocation.model_range = Some(0..4);
        allocation.model_lighting_range = Some(4..8);
        allocation.model_draw_range = None;
        allocation.transparent_model_draw_range = Some(8..12);

        assert_eq!(
            summarize_model_workload([&allocation]),
            ModelWorkloadCount {
                model_ref_count: 1,
                model_draw_ref_count: 2,
                legacy_fixed_slot_quad_invocations_avoided: 30,
            }
        );
    }

    #[test]
    fn model_mdi_batch_emits_one_command_per_eligible_allocation() {
        let tint = ChunkBiomeTintIdentity::new(4, 5);
        let allocations = [
            (Entity::from_bits(201), 0_u32, 1_u32),
            (Entity::from_bits(202), 20_u32, 3_u32),
            (Entity::from_bits(203), 44_u32, 5_u32),
        ]
        .map(|(entity, start, draw_count)| {
            (
                entity,
                GpuChunkAllocation {
                    key: SubChunkKey::new(0, start as i32, 0, 0),
                    generation: 1,
                    tint_identity: tint,
                    quad_range: 0..0,
                    cube_lighting_range: None,
                    model_range: Some(start..start + 4),
                    model_lighting_range: Some(start + 4..start + 6),
                    model_draw_range: Some(start + 6..start + 6 + draw_count * 2),
                    transparent_model_draw_range: None,
                    liquid_range: None,
                    liquid_lighting_range: None,
                    has_depth_liquid: false,
                    has_transparent_liquid: false,
                    depth_liquid_range: None,
                    metadata_index: start / 4,
                },
            )
        });
        let frame_probe = ActiveFrameProbe::default();
        let (commands, drawn) = prepare_model_indirect_batch_draws(
            allocations
                .iter()
                .map(|(entity, allocation)| (*entity, allocation)),
            &frame_probe,
            tint,
        );

        assert_eq!(commands.len(), allocations.len());
        assert_eq!(drawn.len(), allocations.len());
        assert_eq!(
            commands
                .iter()
                .map(|command| command.instance_count)
                .collect::<Vec<_>>(),
            [1, 3, 5]
        );
        assert_eq!(
            drawn.iter().map(|(entity, _)| *entity).collect::<Vec<_>>(),
            allocations.map(|(entity, _)| entity)
        );
    }

    #[test]
    fn any_partial_model_triplet_is_expected_as_model_but_cannot_draw() {
        for (model, lighting, draw) in [
            (Some(0..4), None, None),
            (None, Some(4..6), None),
            (None, None, Some(6..8)),
            (Some(0..4), Some(4..6), None),
            (Some(0..4), None, Some(4..6)),
            (None, Some(0..2), Some(2..4)),
        ] {
            let mut allocation = retirement_test_allocation();
            allocation.model_range = model.clone();
            allocation.model_lighting_range = lighting.clone();
            allocation.model_draw_range = draw.clone();
            allocation.gpu.model_range = model;
            allocation.gpu.model_lighting_range = lighting;
            allocation.gpu.model_draw_range = draw;

            assert!(
                allocation
                    .expected_streams()
                    .contains(ChunkStreamMask::MODEL)
            );
            assert!(model_direct_draw_command(&allocation.gpu).is_none());
            assert!(model_mdi_draw_command(&allocation.gpu).is_none());
        }
    }

    #[test]
    fn liquid_lighting_index_patches_to_shared_arena_records_without_mutating_other_words() {
        let mut quads = vec![[0x432, 7, 0b101, 0], [0x765, 8, 0b110, 2]];
        absolutize_liquid_lighting_indices(&mut quads, 20);
        assert_eq!(quads, [[0x432, 7, 0b101, 10], [0x765, 8, 0b110, 12]]);
    }

    #[test]
    fn transparent_refs_require_exact_instance_identity_and_aligned_stream_ranges() {
        let key = SubChunkKey::new(0, 1, 2, 3);
        let tint = ChunkBiomeTintIdentity::new(4, 5);
        let instance = ChunkRenderInstance {
            key,
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            model_refs: Arc::from([]),
            model_lighting: Arc::from([]),
            model_draw_refs: Arc::from([]),
            transparent_model_draw_refs: Arc::from([]),
            liquid_quads: Arc::from([PackedLiquidQuad::try_pack(
                [0, 0, 0],
                crate::Face::PositiveY,
                [255; 4],
                1,
                0,
                [0, 0],
                false,
            )
            .unwrap()]),
            liquid_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_start: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity: tint,
            generation: 6,
            token: None,
            origin: [16, 32, 48],
        };
        let allocation = GpuChunkAllocation {
            key,
            generation: 6,
            tint_identity: tint,
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(8..12),
            liquid_lighting_range: Some(20..22),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            metadata_index: 7,
        };
        assert!(transparent_allocation_matches(&instance, &allocation, tint));

        let mut mismatches = Vec::new();
        let mut mismatch = allocation.clone();
        mismatch.generation += 1;
        mismatches.push(mismatch);
        let mut mismatch = allocation.clone();
        mismatch.tint_identity = ChunkBiomeTintIdentity::new(4, 6);
        mismatches.push(mismatch);
        let mut mismatch = allocation.clone();
        mismatch.liquid_range = Some(9..13);
        mismatches.push(mismatch);
        let mut mismatch = allocation.clone();
        mismatch.liquid_range = Some(8..16);
        mismatches.push(mismatch);
        let mut mismatch = allocation.clone();
        mismatch.liquid_lighting_range = Some(21..23);
        mismatches.push(mismatch);
        let mut mismatch = allocation.clone();
        mismatch.liquid_lighting_range = Some(20..24);
        mismatches.push(mismatch);
        assert!(
            mismatches.into_iter().all(|allocation| {
                !transparent_allocation_matches(&instance, &allocation, tint)
            })
        );
        assert!(!transparent_allocation_matches(
            &instance,
            &allocation,
            ChunkBiomeTintIdentity::new(9, 9),
        ));
    }

    #[test]
    fn transparent_model_refs_require_the_exact_gpu_generation_and_stream_ranges() {
        let key = SubChunkKey::new(0, 1, 2, 3);
        let instance = ChunkRenderInstance {
            key,
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            model_refs: Arc::from([PackedModelRef::new(0, 0, 0, 1)]),
            model_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
            model_draw_refs: Arc::from([PackedModelDrawRef::new(0, 0)]),
            transparent_model_draw_refs: Arc::from([PackedModelDrawRef::new(0, 0)]),
            liquid_quads: Arc::from([]),
            liquid_lighting: Arc::from([]),
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_start: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity: ChunkBiomeTintIdentity::new(4, 5),
            generation: 6,
            token: None,
            origin: [16, 32, 48],
        };
        let allocation = GpuChunkAllocation {
            key,
            generation: 6,
            tint_identity: instance.tint_identity,
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: Some(0..4),
            model_lighting_range: Some(4..6),
            model_draw_range: Some(6..8),
            transparent_model_draw_range: Some(8..10),
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 7,
        };
        assert!(transparent_model_allocation_matches(&instance, &allocation));

        let mut stale = allocation.clone();
        stale.generation += 1;
        assert!(!transparent_model_allocation_matches(&instance, &stale));
        let mut wrong_key = allocation.clone();
        wrong_key.key = SubChunkKey::new(0, 1, 2, 4);
        assert!(!transparent_model_allocation_matches(&instance, &wrong_key));
        let mut wrong_model = allocation.clone();
        wrong_model.model_range = Some(0..8);
        assert!(!transparent_model_allocation_matches(
            &instance,
            &wrong_model
        ));
        let mut wrong_draw = allocation;
        wrong_draw.transparent_model_draw_range = Some(9..11);
        assert!(!transparent_model_allocation_matches(
            &instance,
            &wrong_draw
        ));
    }

    #[test]
    fn water_and_models_share_one_transparent_upload_allowance() {
        let mut budget = TransparentUploadBudget::default();
        assert!(budget.consume(DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME - 3));
        assert_eq!(budget.remaining(), 3);
        assert!(!budget.consume(4));
        assert!(budget.consume(3));
        assert_eq!(budget.remaining(), 0);

        budget.reset();
        assert_eq!(
            budget.remaining(),
            DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME
        );
    }

    fn resident_transparent_allocation(
        identity: &TransparentAllocationIdentity,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> GpuChunkAllocation {
        GpuChunkAllocation {
            key: identity.key,
            generation: identity.mesh_generation,
            tint_identity,
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(identity.liquid_range.clone()),
            liquid_lighting_range: Some(identity.lighting_range.clone()),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            metadata_index: identity.metadata_index,
        }
    }

    fn committed_transparent_state(
        key: &ViewSortKey,
        refs: Vec<PackedTransparentDrawRef>,
    ) -> TransparentSortState {
        let mut state = TransparentSortState::with_upload_cap(64);
        let generation = state.request(key);
        assert_eq!(
            state.complete(TransparentSortResult::new(generation, key.clone(), refs).unwrap()),
            Ok(false)
        );
        assert!(state.acknowledge_upload());
        state
    }

    #[test]
    fn visibility_membership_churn_retains_resident_snapshot_until_ordered_swap() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let a =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let b =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 1, 0, 0), 4, 16..24, 36..40, 2);
        let c =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 2, 0, 0), 5, 24..32, 40..44, 3);
        let old_key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![a.clone(), b.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let old_refs = vec![
            PackedTransparentDrawRef::new(2, 2),
            PackedTransparentDrawRef::new(1, 1),
        ];
        let mut state = committed_transparent_state(&old_key, old_refs.clone());
        let old_snapshot = state.committed().unwrap().clone();
        let resident = [
            resident_transparent_allocation(&a, tint_identity),
            resident_transparent_allocation(&b, tint_identity),
            resident_transparent_allocation(&c, tint_identity),
        ];
        assert!(transparent_snapshot_addresses_are_resident(
            &old_snapshot,
            resident.iter(),
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));

        // B leaves the frustum while C enters it. B's arena allocation remains
        // resident, so every absolute reference in the old ordered snapshot is
        // still safe to draw while the replacement sort runs.
        let next_key = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![a, c],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let next_generation = state.request_retaining_resident_snapshot(&next_key, true);
        assert_eq!(state.committed(), Some(&old_snapshot));
        let retained_draw = transparent_draw_args(
            state.committed().unwrap().buffer_slot(),
            state.committed().unwrap().refs().len(),
        )
        .unwrap();
        assert_eq!(retained_draw.instance_count, old_refs.len() as u32);
        let new_refs = vec![
            PackedTransparentDrawRef::new(3, 3),
            PackedTransparentDrawRef::new(1, 1),
        ];
        assert_eq!(
            state.complete(
                TransparentSortResult::new(next_generation, next_key, new_refs.clone()).unwrap()
            ),
            Ok(false)
        );
        assert_eq!(state.committed(), Some(&old_snapshot));
        assert_eq!(state.next_upload_batch().unwrap().refs(), new_refs);
        assert!(state.acknowledge_upload());
        let replacement = state.committed().unwrap();
        assert_eq!(replacement.generation(), next_generation);
        assert_eq!(replacement.refs(), new_refs);
        assert_ne!(replacement.buffer_slot(), old_snapshot.buffer_slot());
    }

    #[test]
    fn missing_or_reallocated_snapshot_identity_clears_absolute_refs_immediately() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let identity =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let changed_key = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            texture_identity,
            tint_identity,
        )
        .unwrap();

        let exact = resident_transparent_allocation(&identity, tint_identity);
        let mut changed_liquid_range = exact.clone();
        changed_liquid_range.liquid_range = Some(12..20);
        let mut changed_metadata = exact;
        changed_metadata.metadata_index += 1;
        for resident in [
            Vec::new(),
            vec![changed_liquid_range],
            vec![changed_metadata],
        ] {
            let mut state =
                committed_transparent_state(&key, vec![PackedTransparentDrawRef::new(2, 1)]);
            assert!(!transparent_snapshot_addresses_are_resident(
                state.committed().unwrap(),
                resident.iter(),
                std::iter::empty(),
                texture_identity,
                tint_identity,
            ));
            state.request_retaining_resident_snapshot(&changed_key, false);
            assert!(state.committed().is_none());
        }
    }

    #[test]
    fn generation_only_update_retains_physically_resident_snapshot_and_draw_args() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let old_identity =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let old_key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![old_identity.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let refs = vec![
            PackedTransparentDrawRef::new(2, 1),
            PackedTransparentDrawRef::new(3, 1),
        ];
        let mut state = committed_transparent_state(&old_key, refs.clone());
        let old_snapshot = state.committed().unwrap().clone();

        let mut resident = resident_transparent_allocation(&old_identity, tint_identity);
        resident.generation += 1;
        assert!(transparent_snapshot_addresses_are_resident(
            &old_snapshot,
            [&resident],
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));

        let next_identity = TransparentAllocationIdentity::new(
            old_identity.key,
            resident.generation,
            old_identity.liquid_range.clone(),
            old_identity.lighting_range.clone(),
            old_identity.metadata_index,
        );
        let next_key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![next_identity],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let generation = state.request_retaining_resident_snapshot(&next_key, true);
        assert_eq!(state.committed(), Some(&old_snapshot));
        let retained_args = transparent_draw_args(
            state.committed().unwrap().buffer_slot(),
            state.committed().unwrap().refs().len(),
        )
        .unwrap();
        assert_eq!(retained_args.instance_count, refs.len() as u32);

        let replacement_refs = refs.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(
            state.complete(
                TransparentSortResult::new(generation, next_key, replacement_refs.clone()).unwrap()
            ),
            Ok(false)
        );
        assert_eq!(state.committed(), Some(&old_snapshot));
        assert!(state.acknowledge_upload());
        assert_eq!(state.committed().unwrap().refs(), replacement_refs);
        assert_ne!(
            state.committed().unwrap().buffer_slot(),
            old_snapshot.buffer_slot()
        );
    }

    #[test]
    fn grown_same_start_liquid_range_keeps_old_refs_physically_resident() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let identity =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let snapshot = committed_transparent_state(
            &key,
            vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
        )
        .committed()
        .unwrap()
        .clone();
        let mut resident = resident_transparent_allocation(&identity, tint_identity);
        resident.generation += 1;
        resident.liquid_range = Some(8..24);
        resident.liquid_lighting_range = Some(40..48);
        assert!(transparent_snapshot_addresses_are_resident(
            &snapshot,
            [&resident],
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));
    }

    #[test]
    fn physical_residency_rejects_moved_shrunk_or_structurally_invalid_streams() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let identity =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let snapshot = committed_transparent_state(
            &key,
            vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
        )
        .committed()
        .unwrap()
        .clone();
        let exact = resident_transparent_allocation(&identity, tint_identity);
        let mut moved = exact.clone();
        moved.liquid_range = Some(4..16);
        let mut shrunk = exact.clone();
        shrunk.liquid_range = Some(8..12);
        let mut missing_lighting = exact.clone();
        missing_lighting.liquid_lighting_range = None;
        let mut invalid_lighting_count = exact.clone();
        invalid_lighting_count.liquid_lighting_range = Some(32..34);
        let mut changed_tint = exact.clone();
        changed_tint.tint_identity = ChunkBiomeTintIdentity::new(9, 9);
        let mut changed_key = exact;
        changed_key.key = SubChunkKey::new(0, 1, 0, 0);

        for resident in [
            moved,
            shrunk,
            missing_lighting,
            invalid_lighting_count,
            changed_tint,
            changed_key,
        ] {
            assert!(!transparent_snapshot_addresses_are_resident(
                &snapshot,
                [&resident],
                std::iter::empty(),
                texture_identity,
                tint_identity,
            ));
        }
        assert!(!transparent_snapshot_addresses_are_resident(
            &snapshot,
            [&resident_transparent_allocation(&identity, tint_identity)],
            std::iter::empty(),
            ChunkTextureAssetIdentity::for_test(9, 9),
            tint_identity,
        ));
    }

    #[test]
    fn retirement_fence_uses_monotonic_epochs_and_ignores_stale_callbacks() {
        let fence = TransparentRetirementFence::default();
        let first = fence.try_reserve().expect("reserve first retirement epoch");
        assert_eq!(first, 1);
        assert!(!fence.complete(first + 9));
        assert_eq!(fence.completed_epoch(), 0);
        assert!(fence.complete(first));
        assert_eq!(fence.completed_epoch(), first);
        let second = fence
            .try_reserve()
            .expect("reserve second retirement epoch");
        assert!(second > first);
        assert!(!fence.complete(first));
        assert_eq!(fence.completed_epoch(), first);
        assert!(fence.complete(second));
        assert_eq!(fence.completed_epoch(), second);
    }

    #[test]
    fn retirement_budget_backpressures_without_overcommit_and_recovers_after_release() {
        let mut budget = TransparentRetirementBudget::with_limits(2, 64);
        assert!(budget.try_reserve(1, 32));
        assert!(budget.try_reserve(1, 32));
        assert!(!budget.try_reserve(1, 1));
        assert!(!budget.try_reserve(0, 1));
        budget.release(1, 32);
        assert!(budget.try_reserve(1, 32));
        assert_eq!(budget.items(), 2);
        assert_eq!(budget.bytes(), 64);
    }

    fn retirement_test_allocation() -> ArenaAllocation {
        ArenaAllocation {
            generation: 7,
            tint_identity: ChunkBiomeTintIdentity::new(2, 2),
            cube_range: Some(0..2),
            cube_lighting_range: Some(20..24),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(8..16),
            liquid_lighting_range: Some(16..20),
            quad_capacity: 2,
            geometry_stream_range: Some(0..24),
            geometry_stream_capacity: 30,
            biome_range: 2..6,
            biome_capacity: 4,
            gpu: GpuChunkAllocation {
                key: SubChunkKey::new(0, 0, 0, 0),
                generation: 7,
                tint_identity: ChunkBiomeTintIdentity::new(2, 2),
                quad_range: 0..2,
                cube_lighting_range: Some(20..24),
                model_range: None,
                model_lighting_range: None,
                model_draw_range: None,
                transparent_model_draw_range: None,
                liquid_range: Some(8..16),
                liquid_lighting_range: Some(16..20),
                has_depth_liquid: false,
                has_transparent_liquid: true,
                depth_liquid_range: None,
                metadata_index: 3,
            },
        }
    }

    #[test]
    fn moved_or_shrunk_liquid_contract_requires_copy_on_write_but_containment_does_not() {
        let old = retirement_test_allocation();
        let same_start_growth = GeometryStreamCounts {
            model: 2,
            liquid: 3,
            liquid_lighting: 3,
            ..default()
        };
        assert!(!transparent_geometry_update_requires_cow(
            &old,
            same_start_growth
        ));
        let moved_by_model = GeometryStreamCounts {
            model: 2,
            model_lighting: 1,
            model_draw: 2,
            liquid: 2,
            liquid_lighting: 2,
            ..default()
        };
        assert!(transparent_geometry_update_requires_cow(
            &old,
            moved_by_model
        ));
        assert!(transparent_geometry_update_requires_cow(
            &old,
            GeometryStreamCounts::default()
        ));

        let mut lava_only = old;
        lava_only.gpu.has_transparent_liquid = false;
        assert!(!transparent_geometry_update_requires_cow(
            &lava_only,
            moved_by_model,
        ));
    }

    #[test]
    fn copy_on_write_plan_keeps_old_extent_out_of_free_lists_and_growth_copy() {
        let old = retirement_test_allocation();
        let required = GeometryStreamCounts {
            model: 2,
            model_lighting: 1,
            liquid: 2,
            liquid_lighting: 2,
            ..default()
        };
        let plan = plan_chunk_range_update(
            2,
            &[],
            30,
            &[],
            6,
            &[],
            required,
            4,
            Some(&old),
            true,
            ArenaLimits {
                max_quad_items: 128,
                max_geometry_stream_words: 128,
                max_origin_items: 16,
                max_biome_words: 128,
            },
        )
        .unwrap();
        assert_eq!(plan.geometry_stream_start, 32);
        assert_eq!(plan.free_geometry_stream_words, vec![30..32]);
        assert!(plan.geometry_stream_len > old.geometry_stream_capacity as usize);
        assert_eq!(
            plan_arena_growth(1, plan.geometry_stream_len, 4, 128)
                .unwrap()
                .unwrap()
                .gpu_copy_bytes,
            4,
            "growth copies the prior high-water buffer, including retired bytes"
        );
    }

    #[test]
    fn partial_and_full_retirement_transfer_only_the_owned_spans() {
        let entity = Entity::from_bits(1 << 32 | 11);
        let old = retirement_test_allocation();
        let partial = RetiredArenaAllocation::geometry_only(entity, &old).unwrap();
        assert!(partial.quad.is_none());
        assert!(partial.geometry.is_some());
        assert!(partial.biome.is_none());
        assert!(partial.origin.is_none());

        let full = RetiredArenaAllocation::full(entity, old);
        assert!(full.quad.is_some());
        assert!(full.geometry.is_some());
        assert!(full.biome.is_some());
        assert_eq!(full.origin, Some(3));
    }

    #[test]
    fn retired_range_is_fence_delayed_then_coalesced_for_eventual_reuse() {
        let entity = Entity::from_bits(1 << 32 | 11);
        let old = retirement_test_allocation();
        let mut retirement = RetiredArenaAllocation::geometry_only(entity, &old).unwrap();
        retirement.release_epoch = Some(2);
        let mut len = 60;
        let mut free = Vec::new();
        assert!(!retirement.can_release(1));
        assert_eq!(allocate_quad_range(&mut len, &mut free, 30, 90), Some(60));
        release_quad_range(&mut len, &mut free, 60..90);

        assert!(retirement.can_release(2));
        let (range, capacity) = retirement.geometry.clone().unwrap();
        release_quad_range(&mut len, &mut free, range.start..range.start + capacity);
        assert_eq!(allocate_quad_range(&mut len, &mut free, 30, 90), Some(0));
    }

    #[test]
    fn transparent_witness_requires_the_exact_bounded_key_set() {
        let a = SubChunkKey::new(0, 0, 4, 5);
        let b = SubChunkKey::new(0, 1, 4, 5);
        let request = TransparentWitnessRequest::try_new(7, vec![b, a]).unwrap();
        assert_eq!(request.revision(), 7);
        assert_eq!(request.keys(), &[a, b]);
        assert!(TransparentWitnessRequest::try_new(0, vec![a]).is_err());
        assert!(TransparentWitnessRequest::try_new(1, Vec::new()).is_err());
        assert!(TransparentWitnessRequest::try_new(1, vec![a, a]).is_err());
        assert!(
            TransparentWitnessRequest::try_new(
                1,
                (0..=MAX_TRANSPARENT_WITNESS_KEYS)
                    .map(|x| SubChunkKey::new(0, x as i32, 4, 5))
                    .collect(),
            )
            .is_err()
        );
    }

    #[test]
    fn transparent_witness_snapshot_rejects_gen193_missing_key_then_accepts_gen194_complete() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let a = SubChunkKey::new(0, 0, 4, 5);
        let b = SubChunkKey::new(0, 1, 4, 5);
        let request = TransparentWitnessRequest::try_new(11, vec![a, b]).unwrap();
        let allocation = |key, generation, start| {
            TransparentAllocationIdentity::new(key, generation, start..start + 4, 40..42, 1)
        };
        let missing = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![allocation(a, 193, 8)],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let complete = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![allocation(a, 194, 8), allocation(b, 194, 12)],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        assert!(!transparent_view_key_satisfies_witness(&missing, &request));
        assert!(transparent_view_key_satisfies_witness(&complete, &request));
    }

    #[test]
    fn witness_publishes_two_consecutive_gpu_completions_and_resets_fail_closed() {
        let evidence = TransparentWitnessEvidence::default();
        let key = SubChunkKey::new(0, 0, 4, 5);
        let request = TransparentWitnessRequest::try_new(9, vec![key]).unwrap();
        evidence.set_authoritative_request(&request);
        let first = evidence.try_reserve(&request, 193, true).unwrap();
        assert!(evidence.complete(first));
        let second = evidence.try_reserve(&request, 193, true).unwrap();
        assert!(evidence.complete(second));
        let events = evidence.drain_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].consecutive, 1);
        assert_eq!(events[1].consecutive, 2);
        assert_eq!(events[1].revision, 9);
        assert_eq!(events[1].key_count, 1);

        let reset_evidence = TransparentWitnessEvidence::default();
        reset_evidence.set_authoritative_request(&request);
        let complete = reset_evidence.try_reserve(&request, 194, true).unwrap();
        assert!(reset_evidence.complete(complete));
        let incomplete = reset_evidence.try_reserve(&request, 194, false).unwrap();
        assert!(reset_evidence.complete(incomplete));
        let complete = reset_evidence.try_reserve(&request, 194, true).unwrap();
        assert!(reset_evidence.complete(complete));
        assert_eq!(reset_evidence.drain_events()[0].consecutive, 1);

        let stale = reset_evidence.try_reserve(&request, 194, true).unwrap();
        reset_evidence.reset();
        assert!(!reset_evidence.complete(stale));
        assert!(reset_evidence.drain_events().is_empty());
    }

    #[test]
    fn stale_extracted_witness_request_cannot_reactivate_after_authoritative_reset() {
        let evidence = TransparentWitnessEvidence::default();
        let request =
            TransparentWitnessRequest::try_new(9, vec![SubChunkKey::new(0, 0, 4, 5)]).unwrap();
        evidence.set_authoritative_request(&request);
        assert!(evidence.try_reserve(&request, 193, true).is_some());

        evidence.reset();
        assert!(evidence.try_reserve(&request, 194, true).is_none());
        assert!(evidence.drain_events().is_empty());
    }

    #[test]
    fn incomplete_witness_diagnostic_is_exact_deduplicated_and_reset_bounded() {
        let evidence = TransparentWitnessEvidence::default();
        let a = SubChunkKey::new(0, 0, 4, 5);
        let b = SubChunkKey::new(0, 1, 4, 5);
        let request = TransparentWitnessRequest::try_new(9, vec![a, b]).unwrap();
        evidence.set_authoritative_request(&request);

        for generation in [193, 194] {
            let token = evidence
                .try_reserve_missing(&request, generation, vec![b])
                .unwrap();
            assert!(evidence.complete(token));
        }
        let diagnostics = evidence.drain_incomplete_events();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].revision, 9);
        assert_eq!(diagnostics[0].generation, 193);
        assert_eq!(&*diagnostics[0].missing_keys, &[b]);

        evidence.reset();
        assert!(evidence.drain_incomplete_events().is_empty());
    }

    #[test]
    fn witness_stage_diagnostics_are_change_deduplicated_and_request_bounded() {
        let evidence = TransparentWitnessEvidence::default();
        let key = SubChunkKey::new(0, 0, 4, 5);
        let request = TransparentWitnessRequest::try_new(9, vec![key]).unwrap();
        evidence.set_authoritative_request(&request);
        let mut record = TransparentWitnessStageRecord {
            key,
            extracted_visible: false,
            instance_present: true,
            liquid_quad_count: 5,
            instance_generation: 7,
            allocation_present: false,
            liquid_range_len: 0,
            lighting_range_len: 0,
            allocation_matches: false,
            committed_member: false,
        };
        assert!(evidence.record_stage_snapshot(9, 193, vec![record]));
        assert!(!evidence.record_stage_snapshot(9, 194, vec![record]));
        for generation in 195..=203 {
            record.extracted_visible = !record.extracted_visible;
            let _ = evidence.record_stage_snapshot(9, generation, vec![record]);
        }
        let events = evidence.drain_stage_events();
        assert_eq!(events.len(), 8);
        assert_eq!(events[0].committed_generation, 193);
        assert_eq!(
            events[0].records.as_ref(),
            &[TransparentWitnessStageRecord {
                extracted_visible: false,
                ..record
            }]
        );

        evidence.reset();
        assert!(evidence.drain_stage_events().is_empty());
    }

    #[test]
    fn retired_identity_matches_exact_old_snapshot_and_not_unrelated_active_address() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let old = retirement_test_allocation();
        let identity = TransparentAllocationIdentity::new(
            old.gpu.key,
            old.gpu.generation,
            old.gpu.liquid_range.clone().unwrap(),
            old.gpu.liquid_lighting_range.clone().unwrap(),
            old.gpu.metadata_index,
        );
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let snapshot = committed_transparent_state(
            &key,
            vec![PackedTransparentDrawRef::new(2, old.gpu.metadata_index)],
        )
        .committed()
        .unwrap()
        .clone();
        assert!(transparent_snapshot_addresses_are_resident(
            &snapshot,
            std::iter::empty(),
            [&old.gpu],
            texture_identity,
            tint_identity,
        ));
        let mut unrelated = old.gpu;
        unrelated.generation += 1;
        assert!(!transparent_snapshot_addresses_are_resident(
            &snapshot,
            std::iter::empty(),
            [&unrelated],
            texture_identity,
            tint_identity,
        ));
    }

    #[test]
    fn removal_to_empty_arms_only_after_snapshot_no_longer_references_retired_identity() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let old = retirement_test_allocation();
        let identity = TransparentAllocationIdentity::new(
            old.gpu.key,
            old.gpu.generation,
            old.gpu.liquid_range.clone().unwrap(),
            old.gpu.liquid_lighting_range.clone().unwrap(),
            old.gpu.metadata_index,
        );
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let snapshot = committed_transparent_state(
            &key,
            vec![PackedTransparentDrawRef::new(2, old.gpu.metadata_index)],
        )
        .committed()
        .unwrap()
        .clone();
        assert!(!transparent_retirement_can_arm(Some(&snapshot), &old.gpu));
        assert!(transparent_retirement_can_arm(None, &old.gpu));
    }

    #[test]
    fn asset_or_tint_identity_change_clears_even_resident_snapshot() {
        let texture_identity = ChunkTextureAssetIdentity::for_test(1, 1);
        let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
        let identity =
            TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
        let old_key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity.clone()],
            texture_identity,
            tint_identity,
        )
        .unwrap();
        let resident = [resident_transparent_allocation(&identity, tint_identity)];
        for (next_texture, next_tint) in [
            (ChunkTextureAssetIdentity::for_test(9, 9), tint_identity),
            (texture_identity, ChunkBiomeTintIdentity::new(9, 9)),
        ] {
            let mut state =
                committed_transparent_state(&old_key, vec![PackedTransparentDrawRef::new(2, 1)]);
            assert!(!transparent_snapshot_addresses_are_resident(
                state.committed().unwrap(),
                resident.iter(),
                std::iter::empty(),
                next_texture,
                next_tint,
            ));
            let next_key = ViewSortKey::try_new(
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                vec![identity.clone()],
                next_texture,
                next_tint,
            )
            .unwrap();
            state.request_retaining_resident_snapshot(&next_key, false);
            assert!(state.committed().is_none());
        }
    }

    #[test]
    fn conflicting_manifest_fail_closes_every_absolute_ref_owner_and_active_metric() {
        let metrics = TransparentSortMetrics::default();
        metrics.publish_for_test(TransparentSortMetricsSnapshot {
            request_generation: 7,
            result_generation: 7,
            committed_generation: 7,
            encoded_generation: 7,
            presented_generation: 7,
            ref_count: 2,
            staged_bytes: 16,
            upload_bytes: 16,
            active_slot_age_frames: 3,
            transparent_water_distinct_tint_count: 2,
            ..Default::default()
        });
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let mut runtime = TransparentSortRuntime {
            view_entity: Some(Entity::from_bits(1)),
            ..Default::default()
        };
        let committed_generation = runtime.state.request(&key);
        assert_eq!(
            runtime.state.complete(
                TransparentSortResult::new(committed_generation, key.clone(), vec![]).unwrap()
            ),
            Ok(true)
        );
        let pending_generation = ViewSortGeneration::for_test(committed_generation.get() + 1);
        let work = TransparentSortWork {
            generation: pending_generation,
            requested_at: Instant::now(),
            key: key.clone(),
            view_from_world: Mat4::IDENTITY,
            candidates: Arc::from([]),
            distinct_tint_count: 0,
        };
        assert!(runtime.gate.submit(pending_generation, work).is_some());
        runtime
            .requested_at
            .insert(pending_generation, Instant::now());
        runtime
            .staged_distinct_tint_counts
            .insert(pending_generation, 2);

        runtime.fail_closed_conflicting_manifest(&metrics);

        assert!(runtime.state.committed().is_none());
        assert_eq!(runtime.state.staged_ref_count(), 0);
        assert_eq!(runtime.gate.in_flight_generation(), None);
        assert_eq!(runtime.gate.pending_generation(), None);
        assert!(runtime.requested_at.is_empty());
        assert!(runtime.staged_distinct_tint_counts.is_empty());
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.committed_generation, 0);
        assert_eq!(snapshot.encoded_generation, 0);
        assert_eq!(snapshot.presented_generation, 0);
        assert_eq!(snapshot.ref_count, 0);
        assert_eq!(
            snapshot.upload_bytes, 16,
            "cumulative accounting survives fail-close"
        );
        assert!(runtime.state.request(&key) > committed_generation);
    }

    #[test]
    fn invalid_camera_transform_fail_closes_committed_staged_gate_and_metadata() {
        let metrics = TransparentSortMetrics::default();
        metrics.publish_for_test(TransparentSortMetricsSnapshot {
            committed_generation: 7,
            encoded_generation: 7,
            presented_generation: 7,
            ref_count: 2,
            upload_bytes: 16,
            ..Default::default()
        });
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let mut runtime = TransparentSortRuntime {
            view_entity: Some(Entity::from_bits(1)),
            ..Default::default()
        };
        let committed = runtime.state.request(&key);
        assert_eq!(
            runtime
                .state
                .complete(TransparentSortResult::new(committed, key.clone(), vec![]).unwrap()),
            Ok(true)
        );
        let moved = ViewSortKey::try_new(
            [f32::from_bits(1), 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let staged = runtime.state.request(&moved);
        assert_eq!(
            runtime.state.complete(
                TransparentSortResult::new(
                    staged,
                    moved.clone(),
                    vec![PackedTransparentDrawRef::new(1, 2)],
                )
                .unwrap(),
            ),
            Ok(false)
        );
        let pending = ViewSortGeneration::for_test(staged.get() + 1);
        let work = TransparentSortWork {
            generation: pending,
            requested_at: Instant::now(),
            key: moved,
            view_from_world: Mat4::IDENTITY,
            candidates: Arc::from([]),
            distinct_tint_count: 0,
        };
        assert!(runtime.gate.submit(pending, work).is_some());
        runtime.requested_at.insert(staged, Instant::now());
        runtime.requested_at.insert(pending, Instant::now());
        runtime.staged_distinct_tint_counts.insert(staged, 2);

        fail_closed_transparent_sort_key_error(
            &mut runtime,
            &metrics,
            TransparentSortError::InvalidCameraTransform,
        );

        assert!(runtime.state.committed().is_none());
        assert_eq!(runtime.state.staged_ref_count(), 0);
        assert_eq!(runtime.gate.in_flight_generation(), None);
        assert!(runtime.requested_at.is_empty());
        assert!(runtime.staged_distinct_tint_counts.is_empty());
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.committed_generation, 0);
        assert_eq!(snapshot.encoded_generation, 0);
        assert_eq!(snapshot.presented_generation, 0);
        assert_eq!(snapshot.ref_count, 0);
        assert_eq!(snapshot.upload_bytes, 16);
        assert!(runtime.state.request(&key) > staged);
    }

    #[test]
    fn staged_generation_is_not_resubmitted_and_retains_causal_latency_origin() {
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let mut runtime = TransparentSortRuntime::default();
        let generation = runtime.state.request(&key);
        assert_eq!(
            runtime.state.complete(
                TransparentSortResult::new(
                    generation,
                    key,
                    vec![PackedTransparentDrawRef::new(1, 2)],
                )
                .unwrap(),
            ),
            Ok(false)
        );
        assert!(!runtime.generation_needs_sort_job(generation));

        let requested_at = Instant::now();
        assert_eq!(
            transparent_request_to_commit_latency(
                requested_at,
                requested_at + Duration::from_millis(5),
            ),
            Duration::from_millis(5)
        );
    }

    #[test]
    fn candidate_cache_reuses_camera_only_arc_rebuilds_identity_and_clears_on_failure() {
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let camera_only = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let candidate = TransparentSortCandidate::new(
            SubChunkKey::new(0, 0, 0, 0),
            0,
            4,
            5,
            [8.0; 3],
            [0.5; 3],
        );
        let mut runtime = TransparentSortRuntime::default();
        let (first, first_tints) = runtime
            .resolve_candidate_cache(&key, || Ok((vec![candidate.clone()], 2)))
            .unwrap();
        let (camera_reuse, camera_tints) = runtime
            .resolve_candidate_cache(&camera_only, || {
                panic!("camera-only key rebuilt candidates")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&first, &camera_reuse));
        assert_eq!((first_tints, camera_tints), (2, 2));

        let changed_identity = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(2, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        let (rebuilt, _) = runtime
            .resolve_candidate_cache(&changed_identity, || Ok((vec![candidate], 3)))
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &rebuilt));

        let failed_identity = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![],
            ChunkTextureAssetIdentity::for_test(3, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap();
        assert_eq!(
            runtime.resolve_candidate_cache(&failed_identity, || {
                Err(TransparentSortError::ReferenceCeiling {
                    requested: MAX_TRANSPARENT_DRAW_REFS + 1,
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                })
            }),
            Err(TransparentSortError::ReferenceCeiling {
                requested: MAX_TRANSPARENT_DRAW_REFS + 1,
                ceiling: MAX_TRANSPARENT_DRAW_REFS,
            })
        );
        assert!(runtime.candidate_cache.is_none());
    }

    #[test]
    fn encoded_liquid_draw_is_not_presented_until_submitted_work_completes() {
        let metrics = TransparentSortMetrics::default();
        metrics.publish_for_test(TransparentSortMetricsSnapshot {
            committed_generation: 12,
            ref_count: 1,
            ..Default::default()
        });
        record_encoded_transparent_generation(&metrics, ViewSortGeneration::for_test(12));
        assert_eq!(metrics.snapshot().encoded_generation, 12);
        assert_eq!(metrics.snapshot().presented_generation, 0);

        record_gpu_completed_transparent_generation(&metrics, 12);
        assert_eq!(metrics.snapshot().presented_generation, 12);
    }

    #[test]
    fn transparent_completion_fence_is_bounded_and_stale_callbacks_cannot_regress() {
        let fence = TransparentPresentationFence::default();
        assert!(fence.try_reserve(12));
        assert!(!fence.try_reserve(13));
        assert!(!fence.complete(13));
        assert!(fence.complete(12));
        assert!(fence.try_reserve(13));

        let metrics = TransparentSortMetrics::default();
        metrics.publish_for_test(TransparentSortMetricsSnapshot {
            committed_generation: 13,
            encoded_generation: 13,
            presented_generation: 11,
            ref_count: 1,
            ..Default::default()
        });
        record_gpu_completed_transparent_generation(&metrics, 12);
        assert_eq!(metrics.snapshot().presented_generation, 11);
        assert!(fence.complete(13));
        record_gpu_completed_transparent_generation(&metrics, 13);
        assert_eq!(metrics.snapshot().presented_generation, 13);
    }

    #[test]
    fn daylight_changes_only_sky_light_without_rebaking() {
        let block_only = 0x000f;
        let sky_only = 0x00f0;
        assert_eq!(
            packed_light_factor(block_only, 0.0),
            packed_light_factor(block_only, 1.0)
        );
        assert_eq!(packed_light_factor(sky_only, 0.0), 0.0);
        assert!(packed_light_factor(sky_only, 1.0) > 0.0);
        assert!(packed_light_factor(0x03f0, 1.0) < packed_light_factor(sky_only, 1.0));
    }

    fn target_expectation(
        now: Instant,
        manifest: impl IntoIterator<Item = (SubChunkKey, u64)>,
    ) -> TargetRenderExpectation {
        TargetRenderExpectation {
            cohort: RenderViewCohort::new(0, [65, 65], 16),
            source_cohort: Some(RenderViewCohort::new(0, [0, 0], 16)),
            manifest: Arc::from(manifest.into_iter().collect::<Vec<_>>()),
            view_generation: 1,
            render_ready_at: now,
        }
    }

    fn opaque_runtime_assets() -> &'static RuntimeAssets {
        static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
        ASSETS.get_or_init(|| {
            let compiled = CompiledAssets {
                visuals: vec![
                    BlockVisual {
                        faces: [0; 6],
                        flags: BlockFlags::AIR,
                        kind: VisualKind::Invisible,
                        contributor_role: assets::ContributorRole::Air,
                        model_template: NO_MODEL_TEMPLATE,
                        animation: NO_ANIMATION,
                        variant: 0,
                    },
                    BlockVisual {
                        faces: [1; 6],
                        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                        kind: VisualKind::Cube,
                        contributor_role: assets::ContributorRole::Primary,
                        model_template: NO_MODEL_TEMPLATE,
                        animation: NO_ANIMATION,
                        variant: 0,
                    },
                ]
                .into_boxed_slice(),
                light_properties: vec![assets::LightProperties::default(); 2].into_boxed_slice(),
                hashed: Box::new([]),
                materials: vec![
                    Material {
                        texture: TextureRef::DIAGNOSTIC,
                        flags: 0,
                        animation: NO_ANIMATION
                    };
                    2
                ]
                .into_boxed_slice(),
                model_templates: Box::new([]),
                model_quads: Box::new([]),
                animations: Box::new([]),
                animation_frames: Box::new([]),
                texture_pages: vec![TexturePage::new(TextureArray {
                    layers: 1,
                    mips: [16_u32, 8, 4, 2, 1]
                        .into_iter()
                        .map(|size| TextureMip {
                            size,
                            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                })]
                .into_boxed_slice(),
                biomes: CompiledBiomeAssets::diagnostic(),
            };
            let blob = encode_blob(&compiled).expect("encode opaque plugin test assets");
            RuntimeAssets::decode(&blob).expect("decode opaque plugin test assets")
        })
    }

    fn solid_test_mesh() -> ChunkMesh {
        let sub_chunk = SubChunk::decode(&[9, 1, 0, 1, 2]).expect("uniform test sub-chunk");
        crate::mesh_sub_chunk(
            &crate::BlockClassifier::new(0),
            opaque_runtime_assets(),
            NetworkIdMode::Sequential,
            &crate::Neighbourhood::empty(),
            &sub_chunk,
        )
    }

    #[test]
    fn gpu_write_does_not_complete_a_frame_probe() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let expectation = target_expectation(now, [(key, 7)]);
        let probe = CompletedFrameProbe {
            frame_sequence: 1,
            allocation_manifest: Arc::clone(&expectation.manifest),
            visible_allocation_manifest: Arc::clone(&expectation.manifest),
            drawn_manifest: Arc::clone(&expectation.manifest),
            expectation,
            missing_target_instances: 0,
            unexpected_target_instances: 0,
            source_instances: 0,
            foreign_instances: 0,
            stale_generation_instances: 0,
            orphan_allocations: 0,
            transparent_sort_generation: 0,
            model_witness: None,
        };

        assert!(
            build_presented_frame_ack(probe, FrameCompletionEvidence::default()).is_none(),
            "a PrepareResources write is not post-present GPU completion evidence"
        );
    }

    #[test]
    fn allocated_but_undrawn_target_manifest_is_not_exact_presented_evidence() {
        let render_ready_at = Instant::now();
        let present_returned_at = render_ready_at + std::time::Duration::from_millis(1);
        let gpu_completed_at = present_returned_at + std::time::Duration::from_millis(1);
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let acknowledgement = build_presented_frame_ack(
            FrameProbe::begin(
                target_expectation(render_ready_at, [(key, 7)]),
                [FrameInstanceIdentity {
                    entity,
                    key,
                    generation: 7,
                }],
                [allocation],
            )
            .complete(),
            FrameCompletionEvidence {
                present_returned_at: Some(present_returned_at),
                submitted_work_done_at: Some(gpu_completed_at),
            },
        )
        .expect("post-present GPU completion should publish diagnostic frame evidence");

        assert_eq!(acknowledgement.allocation_manifest.as_ref(), &[(key, 7)]);
        assert!(acknowledgement.drawn_manifest.is_empty());
        assert!(
            !acknowledgement.is_exact(),
            "an allocated but undrawn target generation satisfied the presented-frame gate"
        );
    }

    #[test]
    fn frame_probe_rejects_stale_allocation_generation() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let instance = FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        };
        let stale_allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let probe = FrameProbe::begin(
            target_expectation(now, [(key, 8)]),
            [instance],
            [stale_allocation],
        );

        assert!(probe.record_direct_draw(entity, stale_allocation));
        let completed = probe.complete();
        assert_eq!(completed.stale_generation_instances, 1);
        assert_eq!(completed.missing_target_instances, 1);
        assert_eq!(completed.unexpected_target_instances, 1);
    }

    #[test]
    fn frame_probe_rejects_source_foreign_and_orphan_allocations() {
        let now = Instant::now();
        let target_key = SubChunkKey::new(0, 65, 0, 65);
        let source_key = SubChunkKey::new(0, 0, 0, 0);
        let foreign_key = SubChunkKey::new(1, 65, 0, 65);
        let target = Entity::from_bits(1);
        let source = Entity::from_bits(2);
        let foreign = Entity::from_bits(3);
        let orphan = Entity::from_bits(4);
        let instances = [
            FrameInstanceIdentity {
                entity: target,
                key: target_key,
                generation: 7,
            },
            FrameInstanceIdentity {
                entity: source,
                key: source_key,
                generation: 1,
            },
            FrameInstanceIdentity {
                entity: foreign,
                key: foreign_key,
                generation: 1,
            },
        ];
        let allocations = [
            FrameAllocationIdentity {
                entity: target,
                key: target_key,
                generation: 7,
            },
            FrameAllocationIdentity {
                entity: source,
                key: source_key,
                generation: 1,
            },
            FrameAllocationIdentity {
                entity: foreign,
                key: foreign_key,
                generation: 1,
            },
            FrameAllocationIdentity {
                entity: orphan,
                key: target_key,
                generation: 7,
            },
        ];

        let completed = FrameProbe::begin(
            target_expectation(now, [(target_key, 7)]),
            instances,
            allocations,
        )
        .complete();

        assert_eq!(completed.source_instances, 1);
        assert_eq!(completed.foreign_instances, 1);
        assert_eq!(completed.orphan_allocations, 1);
        assert_eq!(
            completed.missing_target_instances, 0,
            "the hidden target allocation belongs to the complete allocation manifest"
        );
    }

    #[test]
    fn newly_prepared_generation_is_not_acknowledged_until_a_later_draw_frame() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let instance = FrameInstanceIdentity {
            entity,
            key,
            generation: 9,
        };
        let prepared = FrameAllocationIdentity {
            entity,
            key,
            generation: 9,
        };

        let same_frame = FrameProbe::begin(
            target_expectation(now, [(key, 9)]),
            [instance],
            std::iter::empty::<FrameAllocationIdentity>(),
        );
        assert!(
            !same_frame.record_direct_draw(entity, prepared),
            "an allocation created after Queue was eligible in its PrepareResources frame"
        );
        assert_eq!(same_frame.complete().missing_target_instances, 1);

        let later_frame =
            FrameProbe::begin(target_expectation(now, [(key, 9)]), [instance], [prepared]);
        assert!(later_frame.record_direct_draw(entity, prepared));
        assert_eq!(later_frame.complete().missing_target_instances, 0);
    }

    #[test]
    fn direct_and_mdi_draws_publish_the_same_exact_manifest() {
        let now = Instant::now();
        let key_a = SubChunkKey::new(0, 64, 0, 65);
        let key_b = SubChunkKey::new(0, 65, 0, 65);
        let entity_a = Entity::from_bits(1);
        let entity_b = Entity::from_bits(2);
        let instances = [
            FrameInstanceIdentity {
                entity: entity_b,
                key: key_b,
                generation: 8,
            },
            FrameInstanceIdentity {
                entity: entity_a,
                key: key_a,
                generation: 7,
            },
        ];
        let allocations = [
            FrameAllocationIdentity {
                entity: entity_b,
                key: key_b,
                generation: 8,
            },
            FrameAllocationIdentity {
                entity: entity_a,
                key: key_a,
                generation: 7,
            },
        ];
        let expectation = target_expectation(now, [(key_a, 7), (key_b, 8)]);

        let direct = FrameProbe::begin(expectation.clone(), instances, allocations);
        assert!(direct.record_direct_draw(entity_b, allocations[0]));
        assert!(direct.record_direct_draw(entity_a, allocations[1]));
        let direct_manifest = direct.complete().drawn_manifest;

        let mdi = FrameProbe::begin(expectation, instances, allocations);
        assert_eq!(
            mdi.record_mdi_draws([(entity_b, allocations[0]), (entity_a, allocations[1]),]),
            2
        );
        let mdi_manifest = mdi.complete().drawn_manifest;

        assert_eq!(direct_manifest.as_ref(), &[(key_a, 7), (key_b, 8)]);
        assert_eq!(mdi_manifest, direct_manifest);
    }

    #[test]
    fn frame_ack_requires_present_return_and_submitted_work_done_callback() {
        let render_ready_at = Instant::now();
        let present_returned_at = render_ready_at + std::time::Duration::from_millis(1);
        let gpu_completed_at = present_returned_at + std::time::Duration::from_millis(1);
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let probe = FrameProbe::begin(
            target_expectation(render_ready_at, [(key, 7)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [allocation],
        );
        assert!(probe.record_direct_draw(entity, allocation));
        let completed = probe.complete();

        assert!(
            build_presented_frame_ack(
                completed.clone(),
                FrameCompletionEvidence {
                    present_returned_at: Some(present_returned_at),
                    submitted_work_done_at: None,
                },
            )
            .is_none()
        );
        assert!(
            build_presented_frame_ack(
                completed.clone(),
                FrameCompletionEvidence {
                    present_returned_at: None,
                    submitted_work_done_at: Some(gpu_completed_at),
                },
            )
            .is_none()
        );
        assert!(
            build_presented_frame_ack(
                completed.clone(),
                FrameCompletionEvidence {
                    present_returned_at: Some(gpu_completed_at),
                    submitted_work_done_at: Some(present_returned_at),
                },
            )
            .is_none(),
            "GPU completion cannot precede present return"
        );
        let acknowledgement = build_presented_frame_ack(
            completed,
            FrameCompletionEvidence {
                present_returned_at: Some(present_returned_at),
                submitted_work_done_at: Some(gpu_completed_at),
            },
        )
        .expect("both post-render signals should publish the frame");
        assert_eq!(acknowledgement.render_ready_at, render_ready_at);
        assert_eq!(acknowledgement.present_returned_at, present_returned_at);
        assert_eq!(acknowledgement.gpu_completed_at, gpu_completed_at);
    }

    #[test]
    fn transparent_generation_is_published_only_from_actual_liquid_draw_evidence() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let probe = FrameProbe::begin(
            target_expectation(now, [(key, 7)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [(allocation, ChunkStreamMask::LIQUID)],
        );
        assert_eq!(
            probe.record_transparent_draw(ViewSortGeneration::for_test(23), [(entity, allocation)]),
            1
        );
        let completed = probe.complete();
        assert_eq!(completed.transparent_sort_generation, 23);
        assert_eq!(completed.drawn_manifest.as_ref(), &[(key, 7)]);
    }

    #[test]
    fn retired_backed_liquid_draw_still_attributes_the_encoded_sort_generation() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let current = FrameAllocationIdentity {
            entity,
            key,
            generation: 8,
        };
        let retired = FrameAllocationIdentity {
            generation: 7,
            ..current
        };
        let probe = FrameProbe::begin(
            target_expectation(now, [(key, 8)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 8,
            }],
            [(current, ChunkStreamMask::LIQUID)],
        );
        assert_eq!(
            probe.record_transparent_draw(ViewSortGeneration::for_test(24), [(entity, retired)]),
            0,
            "retired geometry is not the current opaque allocation manifest"
        );
        assert_eq!(probe.complete().transparent_sort_generation, 24);
    }

    #[test]
    fn shared_frame_gate_publishes_only_the_current_expectation_callback() {
        let render_ready_at = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let expectation = target_expectation(render_ready_at, [(key, 7)]);
        let gate = PresentedFrameGate::default();
        gate.set_expectation(expectation.clone());
        assert_eq!(gate.expectation(), Some(expectation.clone()));

        let probe = FrameProbe::begin(
            expectation.clone(),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [allocation],
        );
        assert!(probe.record_direct_draw(entity, allocation));
        let completed = probe.complete();
        assert!(gate.try_reserve_callback(&expectation));
        assert!(gate.publish_reserved_probe(
            completed.clone(),
            render_ready_at + std::time::Duration::from_millis(1),
            render_ready_at + std::time::Duration::from_millis(2),
        ));
        assert_eq!(gate.drain().len(), 1);

        let mut replacement = expectation;
        replacement.view_generation = 2;
        assert!(gate.try_reserve_callback(&completed.expectation));
        gate.set_expectation(replacement);
        assert!(!gate.publish_reserved_probe(
            completed,
            render_ready_at + std::time::Duration::from_millis(3),
            render_ready_at + std::time::Duration::from_millis(4),
        ));
        assert!(gate.drain().is_empty());
    }

    #[test]
    fn frame_gate_bounds_in_flight_gpu_callbacks() {
        let render_ready_at = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let expectation = target_expectation(render_ready_at, [(key, 7)]);
        let gate = PresentedFrameGate::default();
        gate.set_expectation(expectation.clone());
        let completed = FrameProbe::begin(
            expectation.clone(),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [allocation],
        )
        .complete();

        for _ in 0..DEFAULT_PRESENTED_FRAME_ACK_CAPACITY {
            assert!(gate.try_reserve_callback(&expectation));
        }
        assert!(
            !gate.try_reserve_callback(&expectation),
            "a stalled GPU allowed an unbounded callback reservation"
        );

        assert!(gate.publish_reserved_probe(
            completed,
            render_ready_at + std::time::Duration::from_millis(1),
            render_ready_at + std::time::Duration::from_millis(2),
        ));
        assert!(gate.try_reserve_callback(&expectation));
    }

    #[test]
    fn duplicate_target_allocations_cannot_satisfy_exact_manifest() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let first = Entity::from_bits(1);
        let duplicate = Entity::from_bits(2);
        let completed = FrameProbe::begin(
            target_expectation(now, [(key, 7)]),
            [
                FrameInstanceIdentity {
                    entity: first,
                    key,
                    generation: 7,
                },
                FrameInstanceIdentity {
                    entity: duplicate,
                    key,
                    generation: 7,
                },
            ],
            [
                FrameAllocationIdentity {
                    entity: first,
                    key,
                    generation: 7,
                },
                FrameAllocationIdentity {
                    entity: duplicate,
                    key,
                    generation: 7,
                },
            ],
        )
        .complete();

        assert_eq!(completed.allocation_manifest.as_ref(), &[(key, 7)]);
        assert_eq!(completed.missing_target_instances, 0);
        assert_eq!(
            completed.unexpected_target_instances, 1,
            "duplicate target allocation multiplicity was collapsed into an exact set"
        );
    }

    #[test]
    fn two_identical_partial_manifests_do_not_satisfy_the_expected_target_manifest() {
        let render_ready_at = Instant::now();
        let key_a = SubChunkKey::new(0, 64, 0, 65);
        let key_b = SubChunkKey::new(0, 65, 0, 65);
        let entity_a = Entity::from_bits(1);
        let allocation_a = FrameAllocationIdentity {
            entity: entity_a,
            key: key_a,
            generation: 7,
        };
        let expectation = target_expectation(render_ready_at, [(key_a, 7), (key_b, 7)]);
        let probe = FrameProbe::begin(
            expectation,
            [FrameInstanceIdentity {
                entity: entity_a,
                key: key_a,
                generation: 7,
            }],
            [allocation_a],
        );
        assert!(probe.record_direct_draw(entity_a, allocation_a));
        let completed = probe.complete();
        let first = build_presented_frame_ack(
            completed.clone(),
            FrameCompletionEvidence {
                present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(1)),
                submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(2)),
            },
        )
        .unwrap();
        let second = build_presented_frame_ack(
            completed,
            FrameCompletionEvidence {
                present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(3)),
                submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(4)),
            },
        )
        .unwrap();

        assert_eq!(first.allocation_manifest.as_ref(), &[(key_a, 7)]);
        assert_eq!(second.allocation_manifest, first.allocation_manifest);
        assert_eq!(first.missing_target_instances, 1);
        assert_eq!(second.missing_target_instances, 1);
        assert!(!first.is_exact());
        assert!(!first.forms_stable_exact_pair_with(&second));
    }

    #[test]
    fn skipped_render_frame_sequence_cannot_form_a_stable_pair() {
        let render_ready_at = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let entity = Entity::from_bits(1);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 7,
        };
        let frame_probe = ActiveFrameProbe::default();
        let completed_frame = || {
            frame_probe.begin(FrameProbe::begin(
                target_expectation(render_ready_at, [(key, 7)]),
                [FrameInstanceIdentity {
                    entity,
                    key,
                    generation: 7,
                }],
                [allocation],
            ));
            assert!(frame_probe.record_direct_draw(entity, allocation));
            frame_probe.take_completed().unwrap()
        };
        let acknowledgement = |probe, present_offset, gpu_offset| {
            build_presented_frame_ack(
                probe,
                FrameCompletionEvidence {
                    present_returned_at: Some(
                        render_ready_at + std::time::Duration::from_millis(present_offset),
                    ),
                    submitted_work_done_at: Some(
                        render_ready_at + std::time::Duration::from_millis(gpu_offset),
                    ),
                },
            )
            .unwrap()
        };
        let first_probe = completed_frame();
        let adjacent_probe = completed_frame();
        let skipped_probe = completed_frame();
        assert_eq!(first_probe.frame_sequence, 1);
        assert_eq!(adjacent_probe.frame_sequence, 2);
        assert_eq!(skipped_probe.frame_sequence, 3);
        let first = acknowledgement(first_probe, 1, 2);
        let adjacent = acknowledgement(adjacent_probe, 3, 4);
        let skipped = acknowledgement(skipped_probe, 5, 6);

        assert!(first.forms_stable_exact_pair_with(&adjacent));
        assert!(
            !first.forms_stable_exact_pair_with(&skipped),
            "non-adjacent target render frames formed a consecutive stable pair"
        );
    }

    #[test]
    fn target_expectation_freezes_a_sorted_independent_complete_manifest() {
        let now = Instant::now();
        let target_a = SubChunkKey::new(0, 64, 0, 65);
        let target_b = SubChunkKey::new(0, 65, 0, 65);
        let foreign = SubChunkKey::new(0, 100, 0, 100);
        let mut queue = ChunkRenderQueue::default();
        queue
            .render_manifest
            .extend([(target_b, 8), (foreign, 99), (target_a, 7)]);

        let expectation = queue.freeze_target_expectation(
            RenderViewCohort::new(0, [65, 65], 16),
            Some(RenderViewCohort::new(0, [0, 0], 16)),
            4,
            now,
        );

        assert_eq!(
            expectation.manifest.as_ref(),
            &[(target_a, 7), (target_b, 8)]
        );
        assert_eq!(expectation.view_generation, 4);
        assert_eq!(expectation.render_ready_at, now);
    }

    #[test]
    fn requested_key_expectation_is_sorted_and_ignores_unrelated_manifest_churn() {
        let now = Instant::now();
        let target_a = SubChunkKey::new(0, 64, 0, 65);
        let target_b = SubChunkKey::new(0, 65, 0, 65);
        let unrelated = SubChunkKey::new(0, 66, 0, 65);
        let cohort = RenderViewCohort::new(0, [65, 65], 16);
        let mut queue = ChunkRenderQueue::default();
        queue
            .render_manifest
            .extend([(target_b, 8), (unrelated, 99), (target_a, 7)]);

        let first = queue
            .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
            .expect("requested keys belong to the active cohort");
        queue.render_manifest.insert(unrelated, 100);
        let after_unrelated_churn = queue
            .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
            .expect("requested keys still belong to the active cohort");
        queue.render_manifest.insert(target_b, 9);
        let after_requested_generation_change = queue
            .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
            .expect("updated requested keys still belong to the active cohort");

        assert_eq!(first.manifest.as_ref(), &[(target_a, 7), (target_b, 8)]);
        assert_eq!(after_unrelated_churn, first);
        assert_eq!(
            after_requested_generation_change.manifest.as_ref(),
            &[(target_a, 7), (target_b, 9)]
        );
        assert_ne!(after_requested_generation_change, first);
    }

    #[test]
    fn requested_key_expectation_rejects_a_key_outside_the_active_cohort() {
        let now = Instant::now();
        let target = SubChunkKey::new(0, 65, 0, 65);
        let outside = SubChunkKey::new(0, 100, 0, 100);
        let cohort = RenderViewCohort::new(0, [65, 65], 16);
        let mut queue = ChunkRenderQueue::default();
        queue.render_manifest.extend([(target, 7), (outside, 8)]);

        assert!(
            queue
                .freeze_target_expectation_for_keys(cohort, None, [target, outside], 4, now)
                .is_none(),
            "an out-of-cohort request must fail closed"
        );
    }

    #[test]
    fn accepted_tracked_queue_changes_maintain_the_expected_generation_manifest() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 0, 65);
        let cohort = RenderViewCohort::new(0, [65, 65], 16);
        let mut queue = ChunkRenderQueue::default();
        queue
            .try_update_tracked(
                key,
                solid_test_mesh(),
                ChunkUploadPriority::new(0.0),
                ChunkUploadToken {
                    generation: 7,
                    dirty_since: now,
                },
            )
            .unwrap();

        assert_eq!(
            queue
                .freeze_target_expectation(cohort, None, 1, now)
                .manifest
                .as_ref(),
            &[(key, 7)]
        );

        queue
            .try_remove_tracked(
                key,
                ChunkUploadPriority::new(0.0),
                ChunkUploadToken {
                    generation: 8,
                    dirty_since: now,
                },
            )
            .unwrap();
        assert!(
            queue
                .freeze_target_expectation(cohort, None, 1, now)
                .manifest
                .is_empty()
        );
    }

    #[test]
    fn indexed_indirect_commands_preserve_order_and_encode_quad_and_origin_ranges() {
        let allocations = [
            GpuChunkAllocation {
                key: SubChunkKey::new(0, 0, 0, 0),
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
                quad_range: 17..23,
                cube_lighting_range: Some(100..112),
                model_range: None,
                model_lighting_range: None,
                model_draw_range: None,
                transparent_model_draw_range: None,
                liquid_range: None,
                liquid_lighting_range: None,
                has_depth_liquid: false,
                has_transparent_liquid: false,
                depth_liquid_range: None,
                metadata_index: 4,
            },
            GpuChunkAllocation {
                key: SubChunkKey::new(0, 1, 0, 0),
                generation: 2,
                tint_identity: ChunkBiomeTintIdentity::default(),
                quad_range: 4..9,
                cube_lighting_range: Some(120..130),
                model_range: None,
                model_lighting_range: None,
                model_draw_range: None,
                transparent_model_draw_range: None,
                liquid_range: None,
                liquid_lighting_range: None,
                has_depth_liquid: false,
                has_transparent_liquid: false,
                depth_liquid_range: None,
                metadata_index: 1,
            },
        ];

        let commands = build_indexed_indirect_commands(allocations.iter());

        assert_eq!(size_of::<DrawIndexedIndirectArgs>(), 20);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].index_count, 6);
        assert_eq!(commands[0].instance_count, 6);
        assert_eq!(commands[0].first_index, 0);
        assert_eq!(commands[0].base_vertex, 16);
        assert_eq!(commands[0].first_instance, 17);
        assert_eq!(commands[1].index_count, 6);
        assert_eq!(commands[1].instance_count, 5);
        assert_eq!(commands[1].first_index, 0);
        assert_eq!(commands[1].base_vertex, 4);
        assert_eq!(commands[1].first_instance, 4);
        assert_eq!(
            bytemuck::cast_slice::<DrawIndexedIndirectArgs, u32>(&commands),
            &[6, 6, 0, 16, 17, 6, 5, 0, 4, 4],
        );
        assert_eq!(metadata_base_vertex(4), Some(commands[0].base_vertex));
        assert_eq!(metadata_base_vertex(1), Some(commands[1].base_vertex));
    }

    #[test]
    fn origin_metadata_preserves_the_palette_record_pointer() {
        assert_eq!(
            gpu_chunk_origin([16, -64, 32], 27, 7, 22),
            Some(GpuChunkOrigin {
                value: [16, -64, 32, 27],
                cube_bases: [7, 11, 0, 0],
            })
        );
        assert_eq!(
            gpu_chunk_origin([0, 0, 0], 0, 0, 0),
            Some(GpuChunkOrigin {
                value: [0, 0, 0, 0],
                cube_bases: [0, 0, 0, 0],
            })
        );
    }

    #[test]
    fn multi_draw_requires_indirect_execution_and_indirect_first_instance() {
        let indirect = DownlevelFlags::INDIRECT_EXECUTION | DownlevelFlags::BASE_VERTEX;
        let first_instance = WgpuFeatures::INDIRECT_FIRST_INSTANCE
            | WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

        assert_eq!(
            select_chunk_draw_mode(indirect, first_instance, false, true),
            ChunkDrawMode::MultiDrawIndirect,
        );
        assert_eq!(
            select_chunk_draw_mode(DownlevelFlags::BASE_VERTEX, first_instance, false, true,),
            ChunkDrawMode::Direct,
        );
        assert_eq!(
            select_chunk_draw_mode(
                indirect,
                WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                false,
                true,
            ),
            ChunkDrawMode::Direct,
        );
        assert_eq!(
            select_chunk_draw_mode(DownlevelFlags::empty(), WgpuFeatures::empty(), false, true,),
            ChunkDrawMode::Unsupported,
        );
        assert_eq!(
            select_chunk_draw_mode(
                DownlevelFlags::INDIRECT_EXECUTION,
                WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                false,
                true,
            ),
            ChunkDrawMode::Unsupported,
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_build_uses_direct_draws_when_indirect_validation_needs_extended_commands() {
        let indirect = DownlevelFlags::INDIRECT_EXECUTION | DownlevelFlags::BASE_VERTEX;
        let first_instance = WgpuFeatures::INDIRECT_FIRST_INSTANCE;

        assert_eq!(
            select_chunk_draw_mode(indirect, first_instance, true, true),
            ChunkDrawMode::Direct,
            "debug DX12 validation expands indexed commands from 20 to 32 bytes, so wgpu 27 cannot batch them safely"
        );
        assert_eq!(
            select_chunk_draw_mode(indirect, first_instance, true, false),
            ChunkDrawMode::MultiDrawIndirect,
            "release DX12 keeps the required multi-draw path after debug validation is compiled out"
        );
    }

    #[test]
    fn indirect_batch_collection_keeps_entities_before_gpu_allocation() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();

        let visible = sorted_visible_entities([(second, ()), (first, ())]);
        let mut expected = vec![first, second];
        expected.sort_unstable();

        assert_eq!(
            visible
                .into_iter()
                .map(|(render_entity, ())| render_entity)
                .collect::<Vec<_>>(),
            expected,
        );
    }

    fn assert_indirect_view_query_is_static<C>()
    where
        C: RenderCommand<Opaque3d, ViewQuery = Read<ViewUniformOffset>>,
    {
    }

    #[test]
    fn indirect_batch_staging_cannot_invalidate_the_view_query() {
        assert_indirect_view_query_is_static::<DrawPackedChunksIndirect>();
    }

    #[test]
    fn missing_or_empty_indirect_batch_has_no_draw_arguments() {
        let mut world = World::new();
        let view = world.spawn_empty().id();
        let mut batches = ChunkIndirectBatches::default();

        assert_eq!(indirect_batch_draw_args(&batches, view), None);

        batches.0.insert(
            view,
            ChunkIndirectBatch {
                visible_entities: Vec::new(),
                drawn_allocations: Vec::new(),
                indirect_offset: 40,
                command_count: 0,
            },
        );
        assert_eq!(indirect_batch_draw_args(&batches, view), None);

        batches.0.get_mut(&view).unwrap().command_count = 3;
        assert_eq!(indirect_batch_draw_args(&batches, view), Some((40, 3)));
    }

    #[test]
    fn adjacent_quad_frees_coalesce_and_reuse_the_lowest_range_under_churn() {
        let mut free = Vec::new();

        insert_free_quad_range(&mut free, 12..16);
        insert_free_quad_range(&mut free, 0..4);
        insert_free_quad_range(&mut free, 8..12);
        insert_free_quad_range(&mut free, 4..8);
        assert_eq!(free.len(), 1);
        assert_eq!(free[0], 0..16);

        assert_eq!(take_free_quad_range(&mut free, 3), Some(0));
        assert_eq!(take_free_quad_range(&mut free, 5), Some(3));
        assert_eq!(free.len(), 1);
        assert_eq!(free[0], 8..16);

        insert_free_quad_range(&mut free, 0..3);
        insert_free_quad_range(&mut free, 3..8);
        assert_eq!(free.len(), 1);
        assert_eq!(free[0], 0..16);
        assert_eq!(take_free_quad_range(&mut free, 16), Some(0));
        assert!(free.is_empty());
    }

    #[test]
    fn bind_group_cache_rebuilds_only_when_a_buffer_identity_changes() {
        let cached = [11_u64, 12, 13];

        assert!(!bind_group_needs_rebuild(true, Some(&cached), &cached));
        assert!(bind_group_needs_rebuild(false, Some(&cached), &cached));
        assert!(bind_group_needs_rebuild(true, None, &cached));
        assert!(bind_group_needs_rebuild(true, Some(&cached), &[11, 99, 13],));
        assert!(bind_group_needs_rebuild(true, Some(&cached), &[11, 12, 99],));
    }

    #[test]
    fn biome_tint_table_is_revisioned_and_keeps_a_fallback_entry() {
        let fallback = ChunkBiomeTints::default();
        assert_eq!(fallback.entries().len(), 1);
        assert_eq!(fallback.revision(), 0);
        assert_eq!(prepare_biome_tint_entries(fallback.entries()).len(), 1);

        let empty = ChunkBiomeTints::with_revision(Arc::from([]), 7);
        assert_eq!(empty.entries().len(), 1);
        assert_eq!(empty.revision(), 7);

        let shared_entries = Arc::from([BiomeTint::default()]);
        let first = ChunkBiomeTints::with_revision(Arc::clone(&shared_entries), 7);
        let replacement = ChunkBiomeTints::with_revision(shared_entries, 8);
        assert_ne!(first.resource_identity(), replacement.resource_identity());

        assert_eq!(pack_linear_rgb10([0.0, 0.0, 0.0]), 0);
        assert_eq!(pack_linear_rgb10([1.0, 1.0, 1.0]), 0x3fff_ffff);
    }

    #[test]
    fn biome_gpu_entries_pack_all_six_tint_classes_and_flags() {
        let entry = BiomeTint {
            grass: [0.1, 0.2, 0.3],
            foliage: [0.2, 0.3, 0.4],
            birch: [0.3, 0.4, 0.5],
            evergreen: [0.4, 0.5, 0.6],
            dry_foliage: [0.5, 0.6, 0.7],
            water: [0.6, 0.7, 0.8],
            flags: 0x5a,
        };
        let gpu = prepare_biome_tint_entries(&[entry])[0];

        assert_eq!(gpu.grass, pack_linear_rgb10(entry.grass));
        assert_eq!(gpu.foliage, pack_linear_rgb10(entry.foliage));
        assert_eq!(gpu.birch, pack_linear_rgb10(entry.birch));
        assert_eq!(gpu.evergreen, pack_linear_rgb10(entry.evergreen));
        assert_eq!(gpu.dry_foliage, pack_linear_rgb10(entry.dry_foliage));
        assert_eq!(gpu.water, pack_linear_rgb10(entry.water));
        assert_eq!(gpu.flags, entry.flags);
    }

    #[test]
    fn tint_table_identity_rebuilds_the_gpu_buffer_and_shared_bind_group() {
        let entries = Arc::from([BiomeTint::default()]);
        let first =
            ChunkBiomeTints::with_identity(Arc::clone(&entries), ChunkBiomeTintIdentity::new(4, 7));
        let replacement =
            ChunkBiomeTints::with_identity(entries, ChunkBiomeTintIdentity::new(5, 7));
        let first_identity = first.resource_identity();
        let replacement_identity = replacement.resource_identity();

        assert!(!biome_tint_gpu_buffer_needs_rebuild(
            Some(first_identity),
            first_identity,
        ));
        assert!(biome_tint_gpu_buffer_needs_rebuild(
            Some(first_identity),
            replacement_identity,
        ));
        assert!(biome_tint_bind_group_needs_rebuild(
            Some(first_identity),
            replacement_identity,
        ));
    }

    #[test]
    fn matching_identity_uploads_acks_and_queues_direct_and_mdi_draws() {
        fn solid_sub_chunk() -> world::SubChunk {
            world::SubChunk::decode(&[9, 1, 0, 1, 2]).expect("uniform solid sub-chunk")
        }

        let active = ChunkBiomeTintIdentity::new(4, 7);
        let mismatched = ChunkBiomeTintIdentity::new(5, 7);
        let matching_key = SubChunkKey::new(0, 0, 0, 0);
        let mismatched_key = SubChunkKey::new(0, 1, 0, 0);
        let now = Instant::now();
        let matching_token = ChunkUploadToken {
            generation: 1,
            dirty_since: now,
        };
        let mismatched_token = ChunkUploadToken {
            generation: 2,
            dirty_since: now,
        };
        let solid = solid_sub_chunk();
        let mesh = || {
            crate::mesh_sub_chunk(
                &crate::BlockClassifier::new(0),
                opaque_runtime_assets(),
                assets::NetworkIdMode::Sequential,
                &crate::Neighbourhood::empty(),
                &solid,
            )
        };
        let acknowledgements = ChunkUploadAcknowledgements::default();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(acknowledgements.clone())
            .insert_resource(ChunkBiomeTints::with_identity(
                Arc::from([BiomeTint::default()]),
                active,
            ))
            .add_plugins(DebugWorldPlugin::new(2));
        {
            let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
            queue
                .try_update_tracked_with_biome_identity(
                    matching_key,
                    mesh(),
                    PackedBiomeRecord::fallback(),
                    active,
                    ChunkUploadPriority::new(0.0),
                    matching_token,
                )
                .unwrap();
            queue
                .try_update_tracked_with_biome_identity(
                    mismatched_key,
                    mesh(),
                    PackedBiomeRecord::fallback(),
                    mismatched,
                    ChunkUploadPriority::new(1.0),
                    mismatched_token,
                )
                .unwrap();
        }
        app.update();
        let instances = app
            .world_mut()
            .query::<(Entity, &ChunkRenderInstance)>()
            .iter(app.world())
            .map(|(entity, instance)| (entity, instance.clone()))
            .collect::<HashMap<_, _>>();
        let candidates = instances
            .iter()
            .map(|(&entity, instance)| GpuUpdateCandidate {
                entity,
                key: instance.key,
                generation: instance.generation,
                tint_identity: instance.tint_identity,
            })
            .collect::<Vec<_>>();
        let selected = plan_gpu_chunk_updates(
            candidates,
            &HashMap::new(),
            Vec3::ZERO,
            active,
            &GpuUpdateFairness::default(),
        );
        assert_eq!(selected.len(), 1);
        let selected_entity = selected[0];
        let selected_instance = &instances[&selected_entity];
        assert_eq!(selected_instance.key, matching_key);
        assert!(acknowledgements.try_reserve(matching_key, matching_token));
        assert!(acknowledgements.complete_with_bytes(matching_key, matching_token, now, 64,));
        let acked = acknowledgements.drain();
        assert_eq!(acked.len(), 1);
        assert_eq!(acked[0].key, matching_key);

        let allocations = instances
            .iter()
            .enumerate()
            .map(|(index, (&entity, instance))| {
                (
                    entity,
                    GpuChunkAllocation {
                        key: instance.key,
                        generation: instance.generation,
                        tint_identity: instance.tint_identity,
                        quad_range: (index as u32 * 6)..(index as u32 * 6 + 6),
                        cube_lighting_range: Some(
                            (200 + index as u32 * 12)..(212 + index as u32 * 12),
                        ),
                        model_range: None,
                        model_lighting_range: None,
                        model_draw_range: None,
                        transparent_model_draw_range: None,
                        liquid_range: None,
                        liquid_lighting_range: None,
                        has_depth_liquid: false,
                        has_transparent_liquid: false,
                        depth_liquid_range: None,
                        metadata_index: index as u32,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let frame_probe = ActiveFrameProbe::default();
        let direct = allocations
            .iter()
            .filter_map(|(&entity, allocation)| {
                drawable_allocation_identity(&frame_probe, entity, allocation, active)
            })
            .collect::<Vec<_>>();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].key, matching_key);

        let (commands, drawn) = prepare_indirect_batch_draws(
            allocations
                .iter()
                .map(|(&entity, allocation)| (entity, allocation)),
            &frame_probe,
            active,
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].1.key, matching_key);
        assert!(acknowledgements.drain().is_empty());
    }

    #[test]
    fn gpu_growth_plan_copies_the_old_allocation_without_a_host_shadow_upload() {
        let growth = plan_arena_growth(8, 9, PACKED_QUAD_BYTES, 16)
            .unwrap()
            .unwrap();
        assert_eq!(growth.new_capacity, 16);
        assert_eq!(growth.gpu_copy_bytes, 64);

        let stats = account_chunk_gpu_uploads(
            ChunkUploadBudget { max_per_frame: 2 },
            2,
            40,
            32,
            0,
            growth.gpu_copy_bytes,
            0,
            0,
        );

        assert_eq!(stats.chunk_updates, 2);
        assert_eq!(stats.chunk_budget, 2);
        assert_eq!(stats.incremental_bytes, 72);
        assert_eq!(stats.gpu_copy_bytes, 64);
        assert_eq!(stats.full_shadow_bytes, 0);
        assert_eq!(stats.total_bytes, 72);
    }

    #[test]
    fn render_world_update_plan_is_capped_before_arena_mutation() {
        let mut world = World::new();
        let candidates = (0..5)
            .map(|index| GpuUpdateCandidate {
                entity: world.spawn_empty().id(),
                key: SubChunkKey::new(0, index, 0, 0),
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
            })
            .collect::<Vec<_>>();
        let allocations = HashMap::new();

        let selected = plan_gpu_chunk_updates(
            candidates,
            &allocations,
            Vec3::ZERO,
            ChunkBiomeTintIdentity::default(),
            &GpuUpdateFairness::default(),
        );

        assert_eq!(selected.into_iter().take(2).count(), 2);
        assert!(allocations.is_empty());
    }

    #[test]
    fn failing_candidates_do_not_starve_a_later_fitting_candidate() {
        let mut world = World::new();
        let failing = world.spawn_empty().id();
        let fitting = world.spawn_empty().id();
        let candidates = vec![
            GpuUpdateCandidate {
                entity: failing,
                key: SubChunkKey::new(0, -10, 0, 0),
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
            },
            GpuUpdateCandidate {
                entity: fitting,
                key: SubChunkKey::new(0, 10, 0, 0),
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
            },
        ];
        let selected = plan_gpu_chunk_updates(
            candidates,
            &HashMap::new(),
            Vec3::ZERO,
            ChunkBiomeTintIdentity::default(),
            &GpuUpdateFairness::default(),
        );
        let mut len = 2;
        let mut free = std::iter::once(0..2).collect::<Vec<_>>();
        let successful = selected
            .into_iter()
            .filter(|entity| {
                let required = if *entity == failing { 3 } else { 2 };
                allocate_quad_range(&mut len, &mut free, required, 2).is_some()
            })
            .collect::<Vec<_>>();

        assert_eq!(successful, [fitting]);
    }

    #[test]
    fn recovery_planner_prefers_near_high_key_over_far_low_key() {
        let mut world = World::new();
        let far = world.spawn_empty().id();
        let near = world.spawn_empty().id();
        let far_key = SubChunkKey::new(0, -100, 0, 0);
        let near_key = SubChunkKey::new(0, 100, 0, 0);
        let candidates = vec![
            GpuUpdateCandidate {
                entity: far,
                key: far_key,
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
            },
            GpuUpdateCandidate {
                entity: near,
                key: near_key,
                generation: 1,
                tint_identity: ChunkBiomeTintIdentity::default(),
            },
        ];

        let selected = plan_gpu_chunk_updates(
            candidates,
            &HashMap::new(),
            Vec3::new(1_608.0, 8.0, 8.0),
            ChunkBiomeTintIdentity::default(),
            &GpuUpdateFairness::default(),
        );

        assert_eq!(selected[0], near);
        assert!(
            ChunkUploadPriority::from_camera(near_key, Vec3::new(1_608.0, 8.0, 8.0))
                < ChunkUploadPriority::from_camera(far_key, Vec3::new(1_608.0, 8.0, 8.0))
        );
    }

    #[test]
    fn recurring_near_replacements_do_not_starve_an_older_far_gpu_update() {
        let mut world = World::new();
        let near = world.spawn_empty().id();
        let far = world.spawn_empty().id();
        let tint = ChunkBiomeTintIdentity::default();
        let near_key = SubChunkKey::new(0, 0, 4, 0);
        let far_key = SubChunkKey::new(0, 0, 4, 5);
        let mut allocations = HashMap::new();
        let mut fairness = GpuUpdateFairness::default();
        for (entity, key) in [(near, near_key), (far, far_key)] {
            let mut allocation = retirement_test_allocation();
            allocation.generation = 0;
            allocation.tint_identity = tint;
            allocation.gpu.key = key;
            allocation.gpu.generation = 0;
            allocation.gpu.tint_identity = tint;
            allocations.insert(entity, allocation);
        }

        let mut far_selected = false;
        for near_generation in 1..=8 {
            let candidates = vec![
                GpuUpdateCandidate {
                    entity: near,
                    key: near_key,
                    generation: near_generation,
                    tint_identity: tint,
                },
                GpuUpdateCandidate {
                    entity: far,
                    key: far_key,
                    generation: 1,
                    tint_identity: tint,
                },
            ];
            let selected =
                plan_gpu_chunk_updates(candidates, &allocations, Vec3::ZERO, tint, &fairness);
            let chosen = selected[0];
            fairness.finish_frame(&selected, &[chosen]);
            if chosen == far {
                far_selected = true;
                break;
            }
            allocations.get_mut(&near).unwrap().generation = near_generation;
        }

        assert!(
            far_selected,
            "recurring nearer remeshes consumed every one-item frame budget"
        );
    }

    #[test]
    fn gpu_update_fairness_is_bounded_prunes_inactive_and_clears_success_or_reset() {
        let mut world = World::new();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let c = world.spawn_empty().id();
        let mut fairness = GpuUpdateFairness::with_limit(2);

        fairness.finish_frame(&[a, b, c], &[]);
        assert_eq!(fairness.len(), 2);
        assert_eq!(fairness.wait_age(a), 1);
        assert_eq!(fairness.wait_age(b), 1);
        assert_eq!(fairness.wait_age(c), 0);

        fairness.finish_frame(&[b, c], &[]);
        assert_eq!(fairness.len(), 2);
        assert_eq!(fairness.wait_age(a), 0);
        assert_eq!(fairness.wait_age(b), 2);
        assert_eq!(fairness.wait_age(c), 1);

        fairness.finish_frame(&[b, c], &[b]);
        assert_eq!(fairness.wait_age(b), 0);
        assert_eq!(fairness.wait_age(c), 2);
        fairness.reset();
        assert!(fairness.is_empty());

        for _ in 0..70_000 {
            fairness.finish_frame(&[c], &[]);
        }
        assert_eq!(fairness.wait_age(c), 70_000);
    }

    #[test]
    fn tracked_empty_mesh_acknowledges_only_after_bounded_application() {
        let key = SubChunkKey::new(0, 1, 2, 3);
        let token = ChunkUploadToken {
            generation: 7,
            dirty_since: Instant::now(),
        };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(DebugWorldPlugin::new(1));
        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_update_tracked(
                key,
                ChunkMesh::default(),
                ChunkUploadPriority::new(0.0),
                token,
            )
            .unwrap();

        assert!(acknowledgements.drain().is_empty());
        app.update();
        let applied = acknowledgements.drain();

        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].key, key);
        assert_eq!(applied[0].token, token);
    }

    #[test]
    fn acknowledgement_surface_is_bounded_and_coalesces_same_key() {
        let acknowledgements = ChunkUploadAcknowledgements::default();
        let now = Instant::now();
        let repeated = SubChunkKey::new(0, 0, 0, 0);
        for generation in 1..=2 {
            acknowledgements.record(ChunkUploadAcknowledgement {
                key: repeated,
                token: ChunkUploadToken {
                    generation,
                    dirty_since: now,
                },
                applied_at: now,
                uploaded_bytes: 0,
            });
        }
        for index in 1..=DEFAULT_RENDER_QUEUE_ITEMS {
            acknowledgements.record(ChunkUploadAcknowledgement {
                key: SubChunkKey::new(0, index as i32, 0, 0),
                token: ChunkUploadToken {
                    generation: 1,
                    dirty_since: now,
                },
                applied_at: now,
                uploaded_bytes: 0,
            });
        }

        let pending = acknowledgements.drain();

        assert!(pending.len() <= DEFAULT_RENDER_QUEUE_ITEMS);
        assert_eq!(
            pending
                .iter()
                .filter(|acknowledgement| acknowledgement.key == repeated)
                .count(),
            1
        );
        assert_eq!(
            pending
                .iter()
                .find(|acknowledgement| acknowledgement.key == repeated)
                .unwrap()
                .token
                .generation,
            2
        );
    }

    #[test]
    fn acknowledgement_reservation_defers_when_full_and_retries_after_drain() {
        let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
        let first = SubChunkKey::new(0, 1, 0, 0);
        let second = SubChunkKey::new(0, 2, 0, 0);
        let now = Instant::now();
        let first_token = ChunkUploadToken {
            generation: 1,
            dirty_since: now,
        };
        let second_token = ChunkUploadToken {
            generation: 2,
            dirty_since: now,
        };

        assert!(acknowledgements.is_empty());
        assert!(acknowledgements.try_reserve(first, first_token));
        assert!(!acknowledgements.is_empty());
        assert!(!acknowledgements.try_reserve(second, second_token));
        assert!(!acknowledgements.complete(first, second_token, now));
        assert!(acknowledgements.complete(first, first_token, now));
        assert_eq!(acknowledgements.drain().len(), 1);
        assert!(acknowledgements.is_empty());
        assert!(acknowledgements.try_reserve(second, second_token));
    }

    #[test]
    fn adapter_failure_releases_capacity_for_later_fitting_extracted_instance() {
        fn encode_zig_zag_i32(value: i32) -> Vec<u8> {
            let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
            let mut encoded = Vec::new();
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                encoded.push(byte);
                if value == 0 {
                    return encoded;
                }
            }
        }

        fn solid_sub_chunk(runtime_id: u32) -> world::SubChunk {
            let mut encoded = vec![9, 1, 0, 1];
            encoded.extend(encode_zig_zag_i32(runtime_id as i32));
            world::SubChunk::decode(&encoded).expect("uniform solid sub-chunk")
        }

        let impossible_key = SubChunkKey::new(0, 0, 0, 0);
        let fitting_key = SubChunkKey::new(0, 10, 0, 0);
        let now = Instant::now();
        let impossible_token = ChunkUploadToken {
            generation: 1,
            dirty_since: now,
        };
        let fitting_token = ChunkUploadToken {
            generation: 2,
            dirty_since: now,
        };
        let solid = solid_sub_chunk(1);
        let classifier = crate::BlockClassifier::new(0);
        let impossible_mesh = crate::mesh_sub_chunk(
            &classifier,
            opaque_runtime_assets(),
            assets::NetworkIdMode::Sequential,
            &crate::Neighbourhood::empty(),
            &solid,
        );
        let fitting_mesh = crate::mesh_sub_chunk(
            &classifier,
            opaque_runtime_assets(),
            assets::NetworkIdMode::Sequential,
            &crate::Neighbourhood::empty()
                .with_negative_x(&solid)
                .with_positive_x(&solid)
                .with_negative_y(&solid)
                .with_positive_y(&solid)
                .with_negative_z(&solid),
            &solid,
        );
        assert_eq!(impossible_mesh.quad_count(), 6);
        assert_eq!(fitting_mesh.quad_count(), 1);

        let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(acknowledgements.clone())
            .add_plugins(DebugWorldPlugin::new(2));
        {
            let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
            queue
                .try_update_tracked(
                    impossible_key,
                    impossible_mesh,
                    ChunkUploadPriority::new(0.0),
                    impossible_token,
                )
                .unwrap();
            queue
                .try_update_tracked(
                    fitting_key,
                    fitting_mesh,
                    ChunkUploadPriority::new(1.0),
                    fitting_token,
                )
                .unwrap();
        }
        app.update();

        let extracted = app
            .world_mut()
            .query::<(Entity, &ChunkRenderInstance)>()
            .iter(app.world())
            .map(|(entity, instance)| (entity, instance.clone()))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            extracted.len(),
            2,
            "acknowledgement capacity must not block main-to-render extraction"
        );

        let candidates = extracted
            .iter()
            .map(|(&entity, instance)| GpuUpdateCandidate {
                entity,
                key: instance.key,
                generation: instance.generation,
                tint_identity: instance.tint_identity,
            })
            .collect::<Vec<_>>();
        let selected = plan_gpu_chunk_updates(
            candidates,
            &HashMap::new(),
            Vec3::ZERO,
            ChunkBiomeTintIdentity::default(),
            &GpuUpdateFairness::default(),
        );
        let mut quad_len = 0;
        let mut free_quads = Vec::new();
        let mut failed = Vec::new();
        let mut successful = Vec::new();
        for entity in selected {
            let instance = &extracted[&entity];
            let required = u32::try_from(instance.quads().len()).unwrap();
            let token = instance.token.expect("tracked upload token");
            assert!(acknowledgements.try_reserve(instance.key, token));
            if allocate_quad_range(&mut quad_len, &mut free_quads, required, 5).is_none() {
                assert!(acknowledgements.cancel(instance.key, token));
                failed.push(instance.key);
                continue;
            }
            let uploaded_bytes = buffer_byte_len(instance.quads().len(), PACKED_QUAD_BYTES)
                .saturating_add(CHUNK_ORIGIN_BYTES);
            assert!(
                acknowledgements.complete_with_bytes(instance.key, token, now, uploaded_bytes,)
            );
            successful.push(instance.key);
        }

        assert_eq!(failed, [impossible_key]);
        assert_eq!(successful, [fitting_key]);
        let applied = acknowledgements.drain();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].key, fitting_key);
        assert_eq!(applied[0].token, fitting_token);
        assert_eq!(
            applied[0].uploaded_bytes,
            PACKED_QUAD_BYTES + CHUNK_ORIGIN_BYTES
        );
        assert!(
            extracted
                .values()
                .any(|instance| instance.key == impossible_key)
        );
    }

    #[test]
    fn same_key_ready_supersession_preserves_bytes_and_latest_token() {
        let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
        let key = SubChunkKey::new(0, 1, 2, 3);
        let now = Instant::now();
        let first = ChunkUploadToken {
            generation: 1,
            dirty_since: now,
        };
        let latest = ChunkUploadToken {
            generation: 2,
            dirty_since: now,
        };

        assert!(acknowledgements.try_reserve(key, first));
        assert!(acknowledgements.complete_with_bytes(key, first, now, 40));
        assert!(acknowledgements.try_reserve(key, latest));
        assert!(acknowledgements.complete_with_bytes(key, latest, now, 24));

        let drained = acknowledgements.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].key, key);
        assert_eq!(drained[0].token, latest);
        assert_eq!(drained[0].uploaded_bytes, 64);
        assert!(acknowledgements.drain().is_empty());
    }

    #[test]
    fn arena_growth_clamps_to_adapter_limits_and_rejects_one_past() {
        let limits = arena_limits_from_device_limits(64, 32);
        assert_eq!(limits.max_quad_items, 4);
        assert_eq!(limits.max_geometry_stream_words, 8);
        assert_eq!(limits.max_origin_items, 1);
        assert_eq!(limits.max_biome_words, 8);

        assert_eq!(
            plan_arena_growth(1, 4, PACKED_QUAD_BYTES, 4).unwrap(),
            Some(ArenaGrowthPlan {
                new_capacity: 4,
                gpu_copy_bytes: 8,
            })
        );
        assert_eq!(
            plan_arena_growth(1, 3, PACKED_QUAD_BYTES, 3).unwrap(),
            Some(ArenaGrowthPlan {
                new_capacity: 3,
                gpu_copy_bytes: 8,
            })
        );
        assert!(plan_arena_growth(1, 5, PACKED_QUAD_BYTES, 4).is_err());
    }

    #[test]
    fn quad_allocator_reuses_and_trims_high_water_without_a_cpu_shadow() {
        let mut len = 0;
        let mut free = Vec::new();
        let first = allocate_quad_range(&mut len, &mut free, 4, 16).unwrap();
        let second = allocate_quad_range(&mut len, &mut free, 6, 16).unwrap();
        assert_eq!((first, second, len), (0, 4, 10));

        release_quad_range(&mut len, &mut free, 0..4);
        assert_eq!(len, 10);
        assert_eq!(free.len(), 1);
        assert_eq!(free[0], 0..4);
        release_quad_range(&mut len, &mut free, 4..10);
        assert_eq!(len, 0);
        assert!(free.is_empty());
        assert_eq!(allocate_quad_range(&mut len, &mut free, 16, 16), Some(0));
        assert_eq!(allocate_quad_range(&mut len, &mut free, 1, 16), None);
    }

    #[test]
    fn biome_range_planning_reserves_zero_and_rolls_back_as_one_transaction() {
        let limits = ArenaLimits {
            max_quad_items: 8,
            max_geometry_stream_words: 8,
            max_origin_items: 8,
            max_biome_words: 8,
        };
        let plan = |quad_len, biome_len, quad_required, biome_required, limits| {
            plan_chunk_range_update(
                quad_len,
                &[],
                0,
                &[],
                biome_len,
                &[],
                GeometryStreamCounts {
                    cube: quad_required,
                    ..Default::default()
                },
                biome_required,
                None,
                false,
                limits,
            )
        };
        let fallback = plan(0, FALLBACK_BIOME_WORDS, 1, 0, limits).unwrap();
        assert_eq!(fallback.biome_start, 0);
        assert_eq!(fallback.biome_capacity, 0);
        assert_eq!(fallback.biome_len, FALLBACK_BIOME_WORDS);

        let real = plan(0, FALLBACK_BIOME_WORDS, 1, 2, limits).unwrap();
        assert_eq!(real.biome_start, FALLBACK_BIOME_WORDS as u32);
        assert_eq!(real.biome_len, FALLBACK_BIOME_WORDS + 2);

        assert!(
            plan(
                4,
                FALLBACK_BIOME_WORDS,
                1,
                1,
                ArenaLimits {
                    max_quad_items: 8,
                    max_geometry_stream_words: 8,
                    max_origin_items: 8,
                    max_biome_words: FALLBACK_BIOME_WORDS,
                },
            )
            .is_none(),
            "a successful temporary quad allocation must not escape when biome allocation fails"
        );

        let mut len = real.biome_len;
        let mut free = real.free_biomes;
        release_quad_range(
            &mut len,
            &mut free,
            real.biome_start..real.biome_start + real.biome_capacity,
        );
        assert_eq!(len, FALLBACK_BIOME_WORDS);
        assert!(free.is_empty());
    }

    #[derive(Component)]
    struct RemovalProbe;

    #[derive(Resource, Default)]
    struct RemovalDeltas(Vec<Entity>);

    fn record_removal_deltas(
        mut removed: RemovedComponents<RemovalProbe>,
        mut deltas: ResMut<RemovalDeltas>,
    ) {
        deltas.0.extend(removed.read());
    }

    #[test]
    fn removed_components_are_reported_once_without_a_presence_scan() {
        let mut app = App::new();
        app.init_resource::<RemovalDeltas>()
            .add_systems(Update, record_removal_deltas);
        let retained = app.world_mut().spawn(RemovalProbe).id();
        let removed = app.world_mut().spawn(RemovalProbe).id();
        let despawned = app.world_mut().spawn(RemovalProbe).id();

        app.update();
        assert!(app.world().resource::<RemovalDeltas>().0.is_empty());

        app.world_mut().entity_mut(removed).remove::<RemovalProbe>();
        app.world_mut().entity_mut(despawned).despawn();
        app.update();
        let mut actual = app.world().resource::<RemovalDeltas>().0.clone();
        actual.sort_unstable();
        let mut expected = vec![removed, despawned];
        expected.sort_unstable();
        assert_eq!(actual, expected);
        assert!(app.world().get::<RemovalProbe>(retained).is_some());

        app.update();
        let mut actual = app.world().resource::<RemovalDeltas>().0.clone();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }

    #[test]
    fn model_witness_request_is_exact_bounded_sorted_and_hashed() {
        let a = SubChunkKey::new(0, 1, 4, 5);
        let b = SubChunkKey::new(0, 2, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0xab; 32], vec![b, a]).unwrap();
        assert_eq!(request.revision(), 7);
        assert_eq!(request.request_hash(), &[0xab; 32]);
        assert_eq!(request.keys(), &[a, b]);
        assert!(ModelWitnessRequest::try_new(0, [0; 32], vec![a]).is_err());
        assert!(ModelWitnessRequest::try_new(1, [0; 32], Vec::new()).is_err());
        assert!(ModelWitnessRequest::try_new(1, [0; 32], vec![a, a]).is_err());
        assert!(
            ModelWitnessRequest::try_new(
                1,
                [0; 32],
                (0..=MAX_MODEL_WITNESS_KEYS)
                    .map(|x| SubChunkKey::new(0, x as i32, 0, 0))
                    .collect(),
            )
            .is_err()
        );
    }

    #[test]
    fn model_witness_rejects_missing_stale_wrong_stream_zero_ref_and_draw_mismatch() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0x11; 32], vec![key]).unwrap();
        let expected = [(key, 9)];

        let missing = evaluate_model_witness_frame(&request, 20, 3, &[], &[], &[]);
        assert_eq!(missing.missing_key_count, 1);
        let stale = evaluate_model_witness_frame(
            &request,
            20,
            3,
            &expected,
            &[(key, 8, ChunkStreamMask::MODEL, 2)],
            &[(key, 8, ChunkStreamMask::MODEL)],
        );
        assert_eq!(stale.stale_generation_count, 1);
        let cube_only = evaluate_model_witness_frame(
            &request,
            20,
            3,
            &expected,
            &[(key, 9, ChunkStreamMask::CUBE, 2)],
            &[(key, 9, ChunkStreamMask::CUBE)],
        );
        assert_eq!(cube_only.wrong_stream_count, 1);
        let zero_ref = evaluate_model_witness_frame(
            &request,
            20,
            3,
            &expected,
            &[(key, 9, ChunkStreamMask::MODEL, 0)],
            &[(key, 9, ChunkStreamMask::MODEL)],
        );
        assert_eq!(zero_ref.zero_model_ref_count, 1);
        let draw_mismatch = evaluate_model_witness_frame(
            &request,
            20,
            3,
            &expected,
            &[(key, 9, ChunkStreamMask::MODEL, 2)],
            &[],
        );
        assert_eq!(draw_mismatch.draw_mismatch_count, 1);
        assert!(!missing.is_exact());
        assert!(!stale.is_exact());
        assert!(!cube_only.is_exact());
        assert!(!zero_ref.is_exact());
        assert!(!draw_mismatch.is_exact());
    }

    #[test]
    fn model_witness_accepts_direct_and_mdi_model_stream_evidence() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0x22; 32], vec![key]).unwrap();
        let expected = [(key, 9)];
        let allocations = [(key, 9, ChunkStreamMask::CUBE | ChunkStreamMask::MODEL, 3)];
        for drawn in [
            vec![(key, 9, ChunkStreamMask::MODEL)],
            vec![(key, 9, ChunkStreamMask::CUBE | ChunkStreamMask::MODEL)],
        ] {
            let frame =
                evaluate_model_witness_frame(&request, 20, 3, &expected, &allocations, &drawn);
            assert!(frame.is_exact());
            assert_eq!(frame.total_model_ref_count, 3);
            assert_eq!(frame.manifest.len(), 1);
        }
    }

    #[test]
    fn model_witness_pair_requires_adjacent_identical_gpu_completed_frames() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 1, 4, 5);
        let manifest: Arc<[ModelWitnessManifestRecord]> = Arc::from([ModelWitnessManifestRecord {
            key,
            generation: 9,
            model_ref_count: 3,
        }]);
        let first =
            ModelWitnessFrameAck::exact_for_test(7, [0x33; 32], 40, 3, Arc::clone(&manifest), now);
        let adjacent =
            ModelWitnessFrameAck::exact_for_test(7, [0x33; 32], 41, 3, Arc::clone(&manifest), now);
        let skipped = ModelWitnessFrameAck::exact_for_test(7, [0x33; 32], 42, 3, manifest, now);
        assert!(first.forms_stable_exact_pair_with(&adjacent));
        assert!(!first.forms_stable_exact_pair_with(&skipped));
    }

    fn presented_model_witness_ack(
        request: &ModelWitnessRequest,
        key: SubChunkKey,
        frame_sequence: u64,
        stale_generation_instances: usize,
        unexpected_target_instances: usize,
    ) -> PresentedFrameAck {
        let now = Instant::now();
        let manifest: Arc<[ModelWitnessManifestRecord]> = Arc::from([ModelWitnessManifestRecord {
            key,
            generation: 9,
            model_ref_count: 3,
        }]);
        PresentedFrameAck {
            cohort: RenderViewCohort::new(key.dimension, [key.x, key.z], 0),
            frame_sequence,
            allocation_manifest: Arc::from([(key, 9)]),
            visible_allocation_manifest: Arc::from([(key, 9)]),
            drawn_manifest: Arc::from([(key, 9)]),
            view_generation: 3,
            render_ready_at: now,
            present_returned_at: now,
            gpu_completed_at: now,
            missing_target_instances: 0,
            unexpected_target_instances,
            source_instances: 0,
            foreign_instances: 0,
            stale_generation_instances,
            orphan_allocations: 0,
            transparent_sort_generation: 0,
            model_witness: Some(ModelWitnessFrameAck::exact_for_test(
                request.revision(),
                *request.request_hash(),
                frame_sequence,
                3,
                manifest,
                now,
            )),
        }
    }

    fn probed_model_witness_ack(
        request: &ModelWitnessRequest,
        render_ready_at: Instant,
        frame_sequence: u64,
        unrelated_visible: bool,
    ) -> PresentedFrameAck {
        let requested_key = request.keys()[0];
        let unrelated_key =
            SubChunkKey::new(0, requested_key.y + 1, requested_key.x + 1, requested_key.z);
        let requested_entity = Entity::from_bits(91);
        let unrelated_entity = Entity::from_bits(92);
        let requested_allocation = FrameAllocationIdentity {
            entity: requested_entity,
            key: requested_key,
            generation: 9,
        };
        let unrelated_allocation = FrameAllocationIdentity {
            entity: unrelated_entity,
            key: unrelated_key,
            generation: 4,
        };
        let mut probe = FrameProbe::begin_with_model_witness(
            target_expectation(render_ready_at, [(requested_key, 9)]),
            [
                FrameInstanceIdentity {
                    entity: requested_entity,
                    key: requested_key,
                    generation: 9,
                },
                FrameInstanceIdentity {
                    entity: unrelated_entity,
                    key: unrelated_key,
                    generation: 4,
                },
            ],
            [
                (
                    requested_allocation,
                    ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                    3,
                ),
                (
                    unrelated_allocation,
                    ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                    2,
                ),
            ],
            request.clone(),
        );
        probe.frame_sequence = frame_sequence;
        assert!(probe.record_visible(requested_entity, requested_allocation));
        if unrelated_visible {
            assert!(probe.record_visible(unrelated_entity, unrelated_allocation));
        }
        assert!(probe.record_direct_streams(
            requested_entity,
            requested_allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
        ));
        build_presented_frame_ack(
            probe.complete(),
            FrameCompletionEvidence {
                present_returned_at: Some(
                    render_ready_at + std::time::Duration::from_millis(frame_sequence),
                ),
                submitted_work_done_at: Some(
                    render_ready_at + std::time::Duration::from_millis(frame_sequence + 1),
                ),
            },
        )
        .unwrap()
    }

    #[test]
    fn exact_model_manifest_pairs_with_unrelated_non_visible_allocation_undrawn() {
        let render_ready_at = Instant::now();
        let key = SubChunkKey::new(0, 65, 65, 65);
        let request = ModelWitnessRequest::try_new(7, [0x43; 32], vec![key]).unwrap();
        let evidence = ModelWitnessEvidence::default();
        evidence.set_authoritative_request(&request);

        for frame_sequence in [40, 41] {
            let acknowledgement =
                probed_model_witness_ack(&request, render_ready_at, frame_sequence, false);
            assert!(acknowledgement.is_exact());
            assert_eq!(acknowledgement.allocation_manifest.len(), 1);
            assert_eq!(acknowledgement.visible_allocation_manifest.len(), 1);
            assert_eq!(
                acknowledgement.visible_allocation_manifest,
                acknowledgement.drawn_manifest
            );
            assert!(
                acknowledgement
                    .model_witness
                    .as_ref()
                    .is_some_and(ModelWitnessFrameAck::is_exact)
            );
            evidence.observe_presented_frame(&request, &acknowledgement);
        }

        let events = evidence.drain_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].consecutive, 1);
        assert_eq!(events[1].consecutive, 2);
        assert!(evidence.is_complete_for(&request));

        let next = ModelWitnessRequest::try_new(8, [0x44; 32], vec![key]).unwrap();
        evidence.set_authoritative_request(&next);
        assert!(!evidence.is_complete_for(&request));
        assert!(!evidence.is_complete_for(&next));
    }

    #[test]
    fn exact_model_manifest_ignores_unrelated_visible_allocation() {
        let render_ready_at = Instant::now();
        let key = SubChunkKey::new(0, 65, 65, 65);
        let request = ModelWitnessRequest::try_new(7, [0x45; 32], vec![key]).unwrap();
        let evidence = ModelWitnessEvidence::default();
        evidence.set_authoritative_request(&request);

        for frame_sequence in [40, 41] {
            let acknowledgement =
                probed_model_witness_ack(&request, render_ready_at, frame_sequence, true);
            assert_eq!(acknowledgement.visible_allocation_manifest.len(), 1);
            assert_eq!(acknowledgement.drawn_manifest.len(), 1);
            assert!(acknowledgement.is_model_witness_compatible());
            assert!(
                acknowledgement
                    .model_witness
                    .as_ref()
                    .is_some_and(ModelWitnessFrameAck::is_exact)
            );
            evidence.observe_presented_frame(&request, &acknowledgement);
        }

        let events = evidence.drain_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].consecutive, 1);
        assert_eq!(events[1].consecutive, 2);
    }

    #[test]
    fn model_frame_probe_scopes_manifests_and_contamination_to_requested_keys() {
        let now = Instant::now();
        let requested_key = SubChunkKey::new(0, 65, 65, 65);
        let unrelated_key = SubChunkKey::new(0, 66, 66, 65);
        let request = ModelWitnessRequest::try_new(7, [0x46; 32], vec![requested_key]).unwrap();
        let requested_entity = Entity::from_bits(93);
        let unrelated_entity = Entity::from_bits(94);
        let requested_allocation = FrameAllocationIdentity {
            entity: requested_entity,
            key: requested_key,
            generation: 9,
        };
        let unrelated_allocation = FrameAllocationIdentity {
            entity: unrelated_entity,
            key: unrelated_key,
            generation: 4,
        };
        let probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(requested_key, 9)]),
            [
                FrameInstanceIdentity {
                    entity: requested_entity,
                    key: requested_key,
                    generation: 9,
                },
                FrameInstanceIdentity {
                    entity: unrelated_entity,
                    key: unrelated_key,
                    generation: 4,
                },
            ],
            [
                (
                    requested_allocation,
                    ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                    3,
                ),
                (
                    unrelated_allocation,
                    ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                    2,
                ),
            ],
            request,
        );

        assert!(probe.record_visible(requested_entity, requested_allocation));
        assert!(probe.record_visible(unrelated_entity, unrelated_allocation));
        assert!(probe.record_direct_streams(
            requested_entity,
            requested_allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
        ));
        assert!(probe.record_direct_streams(
            unrelated_entity,
            unrelated_allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
        ));
        let completed = probe.complete();

        assert_eq!(
            completed.allocation_manifest.as_ref(),
            &[(requested_key, 9)]
        );
        assert_eq!(
            completed.visible_allocation_manifest.as_ref(),
            &[(requested_key, 9)]
        );
        assert_eq!(completed.drawn_manifest.as_ref(), &[(requested_key, 9)]);
        assert_eq!(completed.missing_target_instances, 0);
        assert_eq!(completed.unexpected_target_instances, 0);
        assert_eq!(completed.source_instances, 0);
        assert_eq!(completed.foreign_instances, 0);
        assert_eq!(completed.stale_generation_instances, 0);
        assert_eq!(completed.orphan_allocations, 0);
        assert!(
            completed
                .model_witness
                .is_some_and(|model| model.is_exact())
        );
    }

    #[test]
    fn model_frame_probe_still_rejects_duplicate_and_stale_requested_targets() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 65, 65);
        let request = ModelWitnessRequest::try_new(7, [0x47; 32], vec![key]).unwrap();
        let first = Entity::from_bits(95);
        let duplicate = Entity::from_bits(96);
        let duplicate_probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [
                FrameInstanceIdentity {
                    entity: first,
                    key,
                    generation: 9,
                },
                FrameInstanceIdentity {
                    entity: duplicate,
                    key,
                    generation: 9,
                },
            ],
            [
                (
                    FrameAllocationIdentity {
                        entity: first,
                        key,
                        generation: 9,
                    },
                    ChunkStreamMask::MODEL,
                    3,
                ),
                (
                    FrameAllocationIdentity {
                        entity: duplicate,
                        key,
                        generation: 9,
                    },
                    ChunkStreamMask::MODEL,
                    3,
                ),
            ],
            request.clone(),
        )
        .complete();
        assert_eq!(duplicate_probe.unexpected_target_instances, 1);

        let stale_entity = Entity::from_bits(97);
        let stale_probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [FrameInstanceIdentity {
                entity: stale_entity,
                key,
                generation: 8,
            }],
            [(
                FrameAllocationIdentity {
                    entity: stale_entity,
                    key,
                    generation: 8,
                },
                ChunkStreamMask::MODEL,
                3,
            )],
            request,
        )
        .complete();
        assert_eq!(stale_probe.stale_generation_instances, 1);
        assert_eq!(stale_probe.missing_target_instances, 1);
        assert_eq!(stale_probe.unexpected_target_instances, 1);
        assert_eq!(
            stale_probe
                .model_witness
                .as_ref()
                .expect("model evaluation must be retained")
                .stale_generation_count,
            1
        );
    }

    #[test]
    fn model_frame_probe_rejects_visible_requested_allocation_missing_an_expected_stream_draw() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 65, 65);
        let request = ModelWitnessRequest::try_new(7, [0x48; 32], vec![key]).unwrap();
        let entity = Entity::from_bits(98);
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 9,
        };
        let mut probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 9,
            }],
            [(
                allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                3,
            )],
            request,
        );
        probe.frame_sequence = 40;
        assert!(probe.record_visible(entity, allocation));
        assert!(probe.record_direct_streams(entity, allocation, ChunkStreamMask::MODEL));

        let acknowledgement = build_presented_frame_ack(
            probe.complete(),
            FrameCompletionEvidence {
                present_returned_at: Some(now + std::time::Duration::from_millis(1)),
                submitted_work_done_at: Some(now + std::time::Duration::from_millis(2)),
            },
        )
        .expect("post-present GPU completion should publish model frame evidence");

        assert_eq!(
            acknowledgement.visible_allocation_manifest.as_ref(),
            &[(key, 9)]
        );
        assert!(acknowledgement.drawn_manifest.is_empty());
        assert!(
            acknowledgement
                .model_witness
                .as_ref()
                .is_some_and(ModelWitnessFrameAck::is_exact),
            "the requested model stream itself was drawn exactly"
        );
        assert!(
            !acknowledgement.is_model_witness_compatible(),
            "a visible requested allocation missing its expected cube draw passed outer evidence"
        );
    }

    #[test]
    fn model_frame_probe_counts_only_requested_orphan_allocations() {
        let now = Instant::now();
        let requested_key = SubChunkKey::new(0, 65, 65, 65);
        let unrelated_key = SubChunkKey::new(0, 66, 66, 65);
        let request = ModelWitnessRequest::try_new(7, [0x49; 32], vec![requested_key]).unwrap();
        let probe = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(requested_key, 9)]),
            std::iter::empty::<FrameInstanceIdentity>(),
            [
                (
                    FrameAllocationIdentity {
                        entity: Entity::from_bits(99),
                        key: requested_key,
                        generation: 9,
                    },
                    ChunkStreamMask::MODEL,
                    3,
                ),
                (
                    FrameAllocationIdentity {
                        entity: Entity::from_bits(100),
                        key: unrelated_key,
                        generation: 4,
                    },
                    ChunkStreamMask::MODEL,
                    2,
                ),
            ],
            request,
        );
        let acknowledgement = build_presented_frame_ack(
            probe.complete(),
            FrameCompletionEvidence {
                present_returned_at: Some(now + std::time::Duration::from_millis(1)),
                submitted_work_done_at: Some(now + std::time::Duration::from_millis(2)),
            },
        )
        .expect("post-present GPU completion should publish orphan diagnostics");

        assert_eq!(acknowledgement.orphan_allocations, 1);
        assert!(
            !acknowledgement.is_model_witness_compatible(),
            "a requested orphan allocation passed outer model evidence"
        );
    }

    #[test]
    fn model_witness_outer_compatibility_rejects_every_contamination_counter() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0x44; 32], vec![key]).unwrap();
        let clean = presented_model_witness_ack(&request, key, 40, 0, 0);
        assert!(clean.is_model_witness_compatible());

        let contaminate: [fn(&mut PresentedFrameAck); 6] = [
            |acknowledgement| acknowledgement.missing_target_instances = 1,
            |acknowledgement| acknowledgement.unexpected_target_instances = 1,
            |acknowledgement| acknowledgement.source_instances = 1,
            |acknowledgement| acknowledgement.foreign_instances = 1,
            |acknowledgement| acknowledgement.stale_generation_instances = 1,
            |acknowledgement| acknowledgement.orphan_allocations = 1,
        ];
        for contaminate in contaminate {
            let mut acknowledgement = clean.clone();
            contaminate(&mut acknowledgement);
            assert!(!acknowledgement.is_model_witness_compatible());
        }
    }

    #[test]
    fn exact_model_manifest_cannot_pair_across_stale_outer_frame_contamination() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0x44; 32], vec![key]).unwrap();
        let evidence = ModelWitnessEvidence::default();
        evidence.set_authoritative_request(&request);

        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 40, 0, 0),
        );
        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 41, 1, 0),
        );
        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 42, 0, 0),
        );

        assert!(evidence.drain_events().is_empty());
    }

    #[test]
    fn exact_model_manifest_cannot_pair_across_duplicate_outer_frame_contamination() {
        let key = SubChunkKey::new(0, 1, 4, 5);
        let request = ModelWitnessRequest::try_new(7, [0x55; 32], vec![key]).unwrap();
        let evidence = ModelWitnessEvidence::default();
        evidence.set_authoritative_request(&request);

        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 50, 0, 0),
        );
        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 51, 0, 1),
        );
        evidence.observe_presented_frame(
            &request,
            &presented_model_witness_ack(&request, key, 52, 0, 0),
        );

        assert!(evidence.drain_events().is_empty());
    }

    #[test]
    fn model_witness_uses_actual_direct_and_mdi_frame_probe_recording_paths() {
        let now = Instant::now();
        let key = SubChunkKey::new(0, 65, 4, 65);
        let entity = Entity::from_bits(91);
        let instance = FrameInstanceIdentity {
            entity,
            key,
            generation: 9,
        };
        let allocation = FrameAllocationIdentity {
            entity,
            key,
            generation: 9,
        };
        let request = ModelWitnessRequest::try_new(7, [0x66; 32], vec![key]).unwrap();

        let direct = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [instance],
            [(
                allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                3,
            )],
            request.clone(),
        );
        assert!(direct.record_direct_streams(entity, allocation, ChunkStreamMask::MODEL));
        let direct = direct.complete().model_witness.unwrap();
        assert!(direct.is_exact());
        assert_eq!(direct.total_model_ref_count, 3);

        let mdi = FrameProbe::begin_with_model_witness(
            target_expectation(now, [(key, 9)]),
            [instance],
            [(
                allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                3,
            )],
            request,
        );
        assert_eq!(
            mdi.record_mdi_streams([(entity, allocation)], ChunkStreamMask::MODEL),
            1
        );
        let mdi = mdi.complete().model_witness.unwrap();
        assert!(mdi.is_exact());
        assert_eq!(mdi.manifest, direct.manifest);
    }

    #[test]
    fn depth_liquid_direct_and_mdi_draws_share_exact_addresses() {
        let allocation = GpuChunkAllocation {
            key: SubChunkKey::new(0, 1, 2, 3),
            generation: 4,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(40..64),
            liquid_lighting_range: Some(64..76),
            has_depth_liquid: true,
            has_transparent_liquid: true,
            depth_liquid_range: Some(10..16),
            metadata_index: 7,
        };
        let direct = depth_liquid_direct_draw_command(&allocation).unwrap();
        let mdi = depth_liquid_mdi_draw_command(&allocation).unwrap();
        assert_eq!(direct.index_count, mdi.index_count);
        assert_eq!(direct.instance_count, mdi.instance_count);
        assert_eq!(direct.first_index, mdi.first_index);
        assert_eq!(direct.base_vertex, mdi.base_vertex);
        assert_eq!(direct.first_instance, mdi.first_instance);
        assert_eq!(direct.index_count, 6);
        assert_eq!(direct.instance_count, 6);
        assert_eq!(direct.base_vertex, 28);
        assert_eq!(direct.first_instance, 10);

        let mut water_only = allocation;
        water_only.has_depth_liquid = false;
        assert!(depth_liquid_direct_draw_command(&water_only).is_none());
        assert!(depth_liquid_mdi_draw_command(&water_only).is_none());
    }
}
