use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque, hash_map::Entry},
    ops::Range,
    sync::{Arc, Mutex},
    time::Instant,
};

use assets::{RuntimeAssets, TextureArray};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    camera::{
        primitives::Aabb,
        visibility::{self, VisibilityClass},
    },
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
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
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
            ViewBinnedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, DownlevelFlags, DrawIndexedIndirectArgs, Extent3d, Face as CullFace,
            FilterMode, FragmentState, IndexFormat, Origin3d, PipelineCache, PollType,
            PrimitiveState, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState, WgpuFeatures,
        },
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
};
use world::SubChunkKey;

use crate::{ChunkMesh, PackedBiomeRecord, PackedQuad};

const CHUNK_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b5664c91-763f-4e5c-9310-d12659f70cd4");
const STATIC_QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
const PACKED_QUAD_BYTES: u64 = 8;
const CHUNK_ORIGIN_BYTES: u64 = 16;
const BIOME_WORD_BYTES: u64 = 4;
const FALLBACK_BIOME_WORDS: usize = 2;
const FALLBACK_BIOME_RECORD: [u32; FALLBACK_BIOME_WORDS] = [1 << 8, 0];
const INDEXED_INDIRECT_BYTES: u64 = 20;
const DEFAULT_RENDER_QUEUE_ITEMS: usize = 256;
const DEFAULT_RENDER_QUEUE_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_ACKNOWLEDGEMENT_CAPACITY: usize = DEFAULT_RENDER_QUEUE_ITEMS;
const DEFAULT_PRESENTED_FRAME_ACK_CAPACITY: usize = 8;

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
    pub water: [f32; 3],
}

impl Default for BiomeTint {
    fn default() -> Self {
        Self {
            grass: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            foliage: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            water: [1.0; 3],
        }
    }
}

/// Immutable dense biome tint table. Palette-native chunk records reference
/// these entries by index; entry zero is always a deterministic fallback.
#[derive(Resource, Clone)]
pub struct ChunkBiomeTints {
    entries: Arc<[BiomeTint]>,
    revision: u64,
}

impl Default for ChunkBiomeTints {
    fn default() -> Self {
        Self {
            entries: Arc::from([BiomeTint::default()]),
            revision: 0,
        }
    }
}

impl ChunkBiomeTints {
    /// Replaces tint colours while retaining the dense index contract used by
    /// queued [`PackedBiomeRecord`] palettes. Callers that change index
    /// assignments must enqueue replacement records with the same revision.
    #[must_use]
    pub fn with_revision(entries: Arc<[BiomeTint]>, revision: u64) -> Self {
        let entries = if entries.is_empty() {
            Arc::from([BiomeTint::default()])
        } else {
            entries
        };
        Self { entries, revision }
    }

    #[must_use]
    pub fn entries(&self) -> &[BiomeTint] {
        &self.entries
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    fn identity(&self) -> ChunkBiomeTintIdentity {
        ChunkBiomeTintIdentity {
            pointer: Arc::as_ptr(&self.entries) as *const BiomeTint as usize,
            revision: self.revision,
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
struct ChunkBiomeTintIdentity {
    pointer: usize,
    revision: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
struct CompletedFrameProbe {
    expectation: TargetRenderExpectation,
    frame_sequence: u64,
    allocation_manifest: Arc<[(SubChunkKey, u64)]>,
    drawn_manifest: Arc<[(SubChunkKey, u64)]>,
    missing_target_instances: usize,
    unexpected_target_instances: usize,
    source_instances: usize,
    foreign_instances: usize,
    stale_generation_instances: usize,
    orphan_allocations: usize,
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
}

impl PresentedFrameAck {
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
    Some(PresentedFrameAck {
        cohort: probe.expectation.cohort,
        frame_sequence: probe.frame_sequence,
        allocation_manifest: probe.allocation_manifest,
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

struct FrameProbe {
    expectation: TargetRenderExpectation,
    frame_sequence: u64,
    eligible: HashMap<Entity, FrameAllocationIdentity>,
    allocation_manifest: BTreeSet<(SubChunkKey, u64)>,
    target_allocation_count: usize,
    duplicate_target_instances: usize,
    drawn: Mutex<BTreeSet<(SubChunkKey, u64)>>,
    source_instances: usize,
    foreign_instances: usize,
    stale_generation_instances: usize,
    orphan_allocations: usize,
}

impl FrameProbe {
    fn begin(
        expectation: TargetRenderExpectation,
        instances: impl IntoIterator<Item = FrameInstanceIdentity>,
        allocations: impl IntoIterator<Item = FrameAllocationIdentity>,
    ) -> Self {
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
                expectation
                    .source_cohort
                    .is_some_and(|source| source.contains(instance.key))
            })
            .count();
        let foreign_instances = instances
            .values()
            .filter(|instance| {
                !expectation.cohort.contains(instance.key)
                    && expectation
                        .source_cohort
                        .is_none_or(|source| !source.contains(instance.key))
            })
            .count();
        let target_instance_count = instances
            .values()
            .filter(|instance| expectation.cohort.contains(instance.key))
            .count();
        let unique_target_instance_keys = instances
            .values()
            .filter(|instance| expectation.cohort.contains(instance.key))
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
        let mut allocation_manifest = BTreeSet::new();
        let mut target_allocation_count = 0;
        let mut orphan_allocations = 0;
        for allocation in allocations {
            let Some(instance) = instances.get(&allocation.entity) else {
                orphan_allocations += 1;
                continue;
            };
            if instance.key != allocation.key || instance.generation != allocation.generation {
                stale_entities.insert(allocation.entity);
                continue;
            }
            if expectation.cohort.contains(allocation.key) {
                target_allocation_count += 1;
                allocation_manifest.insert((allocation.key, allocation.generation));
            }
            eligible.insert(allocation.entity, allocation);
        }
        Self {
            expectation,
            frame_sequence: 0,
            eligible,
            allocation_manifest,
            target_allocation_count,
            duplicate_target_instances,
            drawn: Mutex::new(BTreeSet::new()),
            source_instances,
            foreign_instances,
            stale_generation_instances: stale_entities.len(),
            orphan_allocations,
        }
    }

    fn record_direct_draw(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        if self.eligible.get(&entity) != Some(&allocation) {
            return false;
        }
        let identity = (allocation.key, allocation.generation);
        if self.allocation_manifest.contains(&identity) {
            self.drawn
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(identity);
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

    fn complete(self) -> CompletedFrameProbe {
        let expected = self
            .expectation
            .manifest
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let drawn = self
            .drawn
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
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
            drawn_manifest: Arc::from(drawn.into_iter().collect::<Vec<_>>()),
            missing_target_instances,
            unexpected_target_instances,
            source_instances: self.source_instances,
            foreign_instances: self.foreign_instances,
            stale_generation_instances: self.stale_generation_instances,
            orphan_allocations: self.orphan_allocations,
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
            .is_none_or(|probe| probe.eligible.get(&entity) == Some(&allocation))
    }

    fn record_direct_draw(&self, entity: Entity, allocation: FrameAllocationIdentity) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_direct_draw(entity, allocation))
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
        self.try_enqueue(key, mesh, PackedBiomeRecord::fallback(), priority, None)
            .map_err(|(mesh, _)| mesh)
    }

    pub fn try_insert_with_biome(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(key, mesh, biome, priority, None)
    }

    pub fn try_update(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(key, mesh, PackedBiomeRecord::fallback(), priority, None)
            .map_err(|(mesh, _)| mesh)
    }

    pub fn try_update_with_biome(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        priority: ChunkUploadPriority,
    ) -> Result<(), (ChunkMesh, PackedBiomeRecord)> {
        self.try_enqueue(key, mesh, biome, priority, None)
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
        self.try_enqueue(key, mesh, biome, priority, Some(token))
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

    fn try_enqueue(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
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
                priority,
                generation,
                token,
            },
        );
        Ok(())
    }
}

fn mesh_byte_len(mesh: &ChunkMesh) -> u64 {
    buffer_byte_len(mesh.quad_count(), PACKED_QUAD_BYTES)
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
    quads: Arc<[PackedQuad]>,
    biome: PackedBiomeRecord,
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
        self.quads.len()
    }

    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        &self.quads
    }

    #[must_use]
    pub const fn biome_record(&self) -> &PackedBiomeRecord {
        &self.biome
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
        app.init_resource::<ChunkRenderQueue>()
            .init_resource::<ChunkUploadAcknowledgements>()
            .init_resource::<PresentedFrameGate>()
            .init_resource::<ChunkEntities>()
            .init_resource::<ChunkTextureAssets>()
            .init_resource::<ChunkBiomeTints>()
            .insert_resource(self.upload_budget)
            .add_systems(Update, apply_chunk_render_queue);

        if app.get_sub_app(RenderApp).is_none() {
            return;
        }

        app.add_plugins((
            ExtractComponentPlugin::<ChunkRenderInstance>::default(),
            ExtractResourcePlugin::<ChunkTextureAssets>::default(),
            ExtractResourcePlugin::<ChunkBiomeTints>::default(),
        ));

        load_internal_asset!(app, CHUNK_SHADER_HANDLE, "chunk.wgsl", Shader::from_wgsl);

        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        let presented_frame_gate = app.world().resource::<PresentedFrameGate>().clone();

        app.sub_app_mut(RenderApp)
            .insert_resource(self.upload_budget)
            .insert_resource(acknowledgements)
            .insert_resource(presented_frame_gate)
            .init_resource::<ChunkPipeline>()
            .init_resource::<ChunkGpuUploadStats>()
            .init_resource::<ChunkGpuTextureAssets>()
            .init_resource::<ChunkGpuBiomeTints>()
            .init_resource::<ChunkTextureUploadStats>()
            .init_resource::<ChunkIndirectBatches>()
            .init_resource::<ActiveFrameProbe>()
            .add_render_command::<Opaque3d, DrawChunkCommands>()
            .add_render_command::<Opaque3d, DrawChunkIndirectCommands>()
            .add_systems(RenderStartup, init_chunk_gpu_arena)
            .add_systems(
                Render,
                (
                    queue_chunks.in_set(RenderSystems::Queue),
                    prepare_chunk_texture_assets.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_biome_tints.in_set(RenderSystems::PrepareResources),
                    prepare_gpu_chunks.in_set(RenderSystems::PrepareResources),
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
        let instance = ChunkRenderInstance {
            key,
            quads: Arc::from(pending.mesh.into_quads()),
            biome: pending.biome,
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
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ChunkPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "chunk vertex-pulling bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
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
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        Self {
            variants: Variants::new(ChunkPipelineSpecializer, descriptor),
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
    quad_range: Range<u32>,
    metadata_index: u32,
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
) -> ChunkDrawMode {
    if !downlevel_flags.contains(DownlevelFlags::BASE_VERTEX) {
        ChunkDrawMode::Unsupported
    } else if downlevel_flags.contains(DownlevelFlags::INDIRECT_EXECUTION)
        && features.contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE)
    {
        ChunkDrawMode::MultiDrawIndirect
    } else {
        ChunkDrawMode::Direct
    }
}

fn indexed_indirect_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    let instance_count = allocation
        .quad_range
        .end
        .checked_sub(allocation.quad_range.start)?;
    if instance_count == 0 {
        return None;
    }
    let base_vertex = metadata_base_vertex(allocation.metadata_index)?;
    Some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex,
        first_instance: allocation.quad_range.start,
    })
}

fn metadata_base_vertex(metadata_index: u32) -> Option<i32> {
    metadata_index
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
}

fn gpu_chunk_origin(origin: [i32; 3], biome_start: u32) -> [i32; 4] {
    [
        origin[0],
        origin[1],
        origin[2],
        i32::try_from(biome_start).expect("biome arena is limited to i32 offsets"),
    ]
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

#[derive(Clone)]
struct ArenaAllocation {
    generation: u64,
    quad_capacity: u32,
    biome_range: Range<u32>,
    biome_capacity: u32,
    gpu: GpuChunkAllocation,
}

#[derive(Clone, PartialEq, Eq)]
struct ChunkBindGroupBuffers {
    view: BufferId,
    quads: BufferId,
    origins: BufferId,
    biomes: BufferId,
    materials: BufferId,
    biome_tints: BufferId,
    textures: ChunkTextureAssetIdentity,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialGpu {
    layer: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BiomeTintGpu {
    grass: u32,
    foliage: u32,
    water: u32,
    flags: u32,
}

const _: () = assert!(std::mem::size_of::<BiomeTintGpu>() == 16);

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
            water: pack_linear_rgb10(entry.water),
            flags: 0,
        })
        .collect()
}

struct PreparedChunkBiomeTints {
    identity: ChunkBiomeTintIdentity,
    buffer: Buffer,
}

#[derive(Resource, Default)]
struct ChunkGpuBiomeTints {
    prepared: Option<PreparedChunkBiomeTints>,
    _retained_entries: Option<Arc<[BiomeTint]>>,
}

fn prepare_chunk_biome_tints(
    render_device: Res<RenderDevice>,
    source: Res<ChunkBiomeTints>,
    mut gpu: ResMut<ChunkGpuBiomeTints>,
) {
    let identity = source.identity();
    if gpu
        .prepared
        .as_ref()
        .is_some_and(|prepared| prepared.identity == identity)
    {
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
    _texture: Texture,
    view: TextureView,
    sampler: Sampler,
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
    gpu_assets.prepared = None;
    *stats = ChunkTextureUploadStats::default();

    let texture_array = assets.assets().texture_array();
    let device_limits = render_device.limits();
    let limits = TextureArrayLimits {
        max_layers: device_limits.max_texture_array_layers,
        max_dimension_2d: device_limits.max_texture_dimension_2d,
    };
    if let Err(error) = limits.validate(texture_array.layers, assets::TILE_SIZE) {
        bevy::log::error!(?error, "chunk texture array exceeds adapter limits");
        return;
    }
    let upload_plans =
        match plan_texture_mip_uploads(texture_array, RenderDevice::align_copy_bytes_per_row(1)) {
            Ok(plans) => plans,
            Err(error) => {
                bevy::log::error!(?error, "invalid chunk texture upload layout");
                return;
            }
        };

    let material_words = assets
        .assets()
        .materials()
        .iter()
        .map(|material| MaterialGpu {
            layer: material.layer,
            flags: material.flags,
        })
        .collect::<Vec<_>>();
    let material_bytes = material_words
        .len()
        .saturating_mul(std::mem::size_of::<MaterialGpu>());
    if u64::try_from(material_bytes).map_or(true, |bytes| {
        bytes > device_limits.max_buffer_size
            || bytes > u64::from(device_limits.max_storage_buffer_binding_size)
    }) {
        bevy::log::error!(
            material_bytes,
            "chunk material table exceeds adapter limits"
        );
        return;
    }
    let material_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk materials"),
        contents: bytemuck::cast_slice(&material_words),
        usage: BufferUsages::STORAGE,
    });
    let mip_level_count = match u32::try_from(texture_array.mips.len()) {
        Ok(count) => count,
        Err(_) => {
            bevy::log::error!("chunk texture array has too many mip levels");
            return;
        }
    };
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some("global chunk texture array"),
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
    for (mip, plan) in texture_array.mips.iter().zip(&upload_plans) {
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
        label: Some("global chunk texture array view"),
        dimension: Some(TextureViewDimension::D2Array),
        mip_level_count: Some(mip_level_count),
        array_layer_count: Some(texture_array.layers),
        ..Default::default()
    });
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("global chunk repeat sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: FilterMode::Linear,
        ..Default::default()
    });

    stats.upload_count = 1;
    stats.material_bytes = material_bytes as u64;
    stats.texture_bytes_including_mips = texture_array
        .mips
        .iter()
        .map(|mip| mip.rgba8.len() as u64)
        .sum();
    stats.padded_upload_bytes = padded_upload_bytes;
    gpu_assets.prepared = Some(PreparedChunkTextureAssets {
        identity,
        material_buffer,
        _texture: texture,
        view,
        sampler,
    });
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
        max_origin_items,
        max_biome_words,
    }
}

#[derive(Resource)]
struct ChunkGpuArena {
    quad_buffer: Buffer,
    origin_buffer: Buffer,
    biome_buffer: Buffer,
    index_buffer: Buffer,
    indirect_buffer: Buffer,
    bind_group: Option<BindGroup>,
    bind_group_buffers: Option<ChunkBindGroupBuffers>,
    quad_capacity: usize,
    origin_capacity: usize,
    biome_capacity: usize,
    indirect_capacity: usize,
    quad_len: usize,
    origin_len: usize,
    biome_len: usize,
    limits: ArenaLimits,
    free_quads: Vec<Range<u32>>,
    free_origins: Vec<u32>,
    free_biomes: Vec<Range<u32>>,
    allocations: HashMap<Entity, ArenaAllocation>,
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
            indirect_buffer: create_indirect_buffer(render_device, 1),
            bind_group: None,
            bind_group_buffers: None,
            quad_capacity: 1,
            origin_capacity: 1,
            biome_capacity: FALLBACK_BIOME_WORDS,
            indirect_capacity: 1,
            quad_len: 0,
            origin_len: 0,
            biome_len: FALLBACK_BIOME_WORDS,
            limits,
            free_quads: Vec::new(),
            free_origins: Vec::new(),
            free_biomes: Vec::new(),
            allocations: HashMap::new(),
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
}

fn plan_gpu_chunk_updates(
    mut candidates: Vec<GpuUpdateCandidate>,
    allocations: &HashMap<Entity, ArenaAllocation>,
    camera_position: Vec3,
) -> Vec<Entity> {
    candidates.retain(|candidate| {
        allocations
            .get(&candidate.entity)
            .is_none_or(|allocation| allocation.generation != candidate.generation)
    });
    candidates.sort_by(|left, right| {
        ChunkUploadPriority::from_camera(left.key, camera_position)
            .distance_squared()
            .total_cmp(
                &ChunkUploadPriority::from_camera(right.key, camera_position).distance_squared(),
            )
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
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let candidates = instances
        .iter()
        .map(|(entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
        })
        .collect();
    let camera_position = views
        .iter()
        .next()
        .map(|view| view.world_from_view.translation())
        .unwrap_or(Vec3::ZERO);
    let selected = plan_gpu_chunk_updates(candidates, &arena.allocations, camera_position);

    for entity in removed_instances.read() {
        free_allocation(&mut arena, entity);
    }

    let mut quad_writes = Vec::new();
    let mut biome_writes = Vec::new();
    let mut origin_writes = Vec::new();
    let mut applied_tokens = Vec::new();
    let mut chunk_updates = 0;
    for entity in selected {
        if chunk_updates >= budget.max_per_frame {
            break;
        }
        let Ok((_, instance)) = instances.get(entity) else {
            continue;
        };
        let old = arena.allocations.get(&entity).cloned();
        let required = match u32::try_from(instance.quads.len()) {
            Ok(required) => required,
            Err(_) => {
                bevy::log::error!("sub-chunk mesh exceeds the u32 instance range");
                continue;
            }
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
        let Some((quad_start, quad_capacity, biome_start, biome_capacity)) =
            allocate_for_chunk_update(&mut arena, required, biome_required, old.as_ref())
        else {
            if let Some(token) = instance.token {
                acknowledgements.cancel(instance.key, token);
            }
            bevy::log::warn!("chunk quad arena is at the adapter storage-buffer limit");
            continue;
        };
        let metadata_index = match old {
            Some(old) => old.gpu.metadata_index,
            None => allocate_origin(&mut arena)
                .expect("origin capacity was checked before quad allocation"),
        };
        let quad_end = quad_start + required;
        let words = instance
            .quads
            .iter()
            .map(PackedQuad::words)
            .collect::<Vec<_>>();
        let origin = gpu_chunk_origin(instance.origin, biome_start);
        quad_writes.push((quad_start, words));
        if !biome_words.is_empty() {
            biome_writes.push((biome_start, biome_words));
        }
        origin_writes.push((metadata_index, origin));
        let gpu = GpuChunkAllocation {
            key: instance.key,
            generation: instance.generation,
            quad_range: quad_start..quad_end,
            metadata_index,
        };
        commands.entity(entity).insert(gpu.clone());
        arena.allocations.insert(
            entity,
            ArenaAllocation {
                generation: instance.generation,
                quad_capacity,
                biome_range: biome_start..biome_start + biome_required,
                biome_capacity,
                gpu,
            },
        );
        if let Some(token) = instance.token {
            let uploaded_bytes = buffer_byte_len(instance.quads.len(), PACKED_QUAD_BYTES)
                .saturating_add(CHUNK_ORIGIN_BYTES)
                .saturating_add(biome_record_byte_len(&instance.biome));
            applied_tokens.push((instance.key, token, uploaded_bytes));
        }
        chunk_updates += 1;
    }

    let quad_incremental_bytes = quad_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_BYTES))
    });
    let origin_incremental_bytes = buffer_byte_len(origin_writes.len(), CHUNK_ORIGIN_BYTES);
    let biome_incremental_bytes = biome_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), BIOME_WORD_BYTES))
    });
    let quad_gpu_copy_bytes = ensure_quad_capacity(&mut arena, &render_device, &render_queue);
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
        quad_incremental_bytes,
        origin_incremental_bytes,
        biome_incremental_bytes,
        quad_gpu_copy_bytes,
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

fn allocate_for_chunk_update(
    arena: &mut ChunkGpuArena,
    quad_required: u32,
    biome_required: u32,
    old: Option<&ArenaAllocation>,
) -> Option<(u32, u32, u32, u32)> {
    let plan = plan_chunk_range_update(
        arena.quad_len,
        &arena.free_quads,
        arena.biome_len,
        &arena.free_biomes,
        quad_required,
        biome_required,
        old,
        arena.limits,
    )?;
    arena.quad_len = plan.quad_len;
    arena.free_quads = plan.free_quads;
    arena.biome_len = plan.biome_len;
    arena.free_biomes = plan.free_biomes;
    Some((
        plan.quad_start,
        plan.quad_capacity,
        plan.biome_start,
        plan.biome_capacity,
    ))
}

struct ChunkRangePlan {
    quad_start: u32,
    quad_capacity: u32,
    biome_start: u32,
    biome_capacity: u32,
    quad_len: usize,
    free_quads: Vec<Range<u32>>,
    biome_len: usize,
    free_biomes: Vec<Range<u32>>,
}

#[allow(clippy::too_many_arguments)]
fn plan_chunk_range_update(
    mut quad_len: usize,
    current_free_quads: &[Range<u32>],
    mut biome_len: usize,
    current_free_biomes: &[Range<u32>],
    quad_required: u32,
    biome_required: u32,
    old: Option<&ArenaAllocation>,
    limits: ArenaLimits,
) -> Option<ChunkRangePlan> {
    let mut free_quads = current_free_quads.to_vec();
    let (quad_start, quad_capacity) = allocate_range_for_update(
        &mut quad_len,
        &mut free_quads,
        quad_required,
        old.map(|old| (old.gpu.quad_range.start, old.quad_capacity)),
        limits.max_quad_items,
        0,
    )?;

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
        biome_start,
        biome_capacity,
        quad_len,
        free_quads,
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
        let freed = allocation.gpu.quad_range.start
            ..allocation.gpu.quad_range.start + allocation.quad_capacity;
        release_quad_range(&mut arena.quad_len, &mut arena.free_quads, freed);
        if allocation.biome_capacity != 0 {
            let freed = allocation.biome_range.start
                ..allocation.biome_range.start + allocation.biome_capacity;
            release_quad_range(&mut arena.biome_len, &mut arena.free_biomes, freed);
        }
        release_origin(arena, allocation.gpu.metadata_index);
    }
}

fn buffer_byte_len(item_count: usize, item_bytes: u64) -> u64 {
    u64::try_from(item_count)
        .unwrap_or(u64::MAX)
        .saturating_mul(item_bytes)
}

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

fn prepare_chunk_bind_group(
    pipeline: Res<ChunkPipeline>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    render_device: Res<RenderDevice>,
    texture_assets: Res<ChunkGpuTextureAssets>,
    biome_tints: Res<ChunkGpuBiomeTints>,
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
        biome_tints: biome_tints.buffer.id(),
        textures: texture_assets.identity,
    };
    if !bind_group_needs_rebuild(
        arena.bind_group.is_some(),
        arena.bind_group_buffers.as_ref(),
        &buffers,
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
                resource: BindingResource::TextureView(&texture_assets.view),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::Sampler(&texture_assets.sampler),
            },
            BindGroupEntry {
                binding: 6,
                resource: arena.biome_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 7,
                resource: biome_tints.buffer.as_entire_binding(),
            },
        ],
    );
    arena.bind_group = Some(bind_group);
    arena.bind_group_buffers = Some(buffers);
}

fn prepare_chunk_indirect_batches(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    allocations: Query<&GpuChunkAllocation>,
    frame_probe: Res<ActiveFrameProbe>,
    mut batches: ResMut<ChunkIndirectBatches>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let mut all_commands = Vec::new();
    for batch in batches.0.values_mut() {
        let mut indirect_commands = Vec::new();
        batch.drawn_allocations.clear();
        for &entity in &batch.visible_entities {
            let Ok(allocation) = allocations.get(entity) else {
                continue;
            };
            let identity = FrameAllocationIdentity {
                entity,
                key: allocation.key,
                generation: allocation.generation,
            };
            if !frame_probe.accepts(entity, identity) {
                continue;
            }
            let Some(command) = indexed_indirect_command(allocation) else {
                continue;
            };
            indirect_commands.push(command);
            batch.drawn_allocations.push((entity, identity));
        }
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
    presented_frame_gate: Res<PresentedFrameGate>,
    frame_probe: Res<ActiveFrameProbe>,
    mut indirect_batches: ResMut<ChunkIndirectBatches>,
    mut next_tick: Local<Tick>,
    mut unsupported_reported: Local<bool>,
) {
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
    );
    let draw_functions = draw_functions.read();
    let direct_draw = draw_functions.id::<DrawChunkCommands>();
    let indirect_draw = draw_functions.id::<DrawChunkIndirectCommands>();
    indirect_batches.0.clear();
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
    if let Some(expectation) = presented_frame_gate.expectation() {
        frame_probe.begin(FrameProbe::begin(
            expectation,
            instances
                .iter()
                .map(|(entity, instance)| FrameInstanceIdentity {
                    entity,
                    key: instance.key,
                    generation: instance.generation,
                }),
            arena
                .allocations
                .iter()
                .map(|(&entity, allocation)| FrameAllocationIdentity {
                    entity,
                    key: allocation.gpu.key,
                    generation: allocation.gpu.generation,
                }),
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

        if draw_mode == ChunkDrawMode::MultiDrawIndirect {
            let visible = sorted_visible_entities(
                visible_entities
                    .get::<ChunkRenderInstance>()
                    .iter()
                    .copied(),
            )
            .into_iter()
            .filter(|(entity, _)| {
                allocations.get(*entity).is_ok_and(|allocation| {
                    frame_probe.accepts(
                        *entity,
                        FrameAllocationIdentity {
                            entity: *entity,
                            key: allocation.key,
                            generation: allocation.generation,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();

            if visible.is_empty() {
                continue;
            }
            indirect_batches.0.insert(
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
            continue;
        }

        for &(render_entity, main_entity) in visible_entities.get::<ChunkRenderInstance>() {
            let Ok(allocation) = allocations.get(render_entity) else {
                continue;
            };
            if !frame_probe.accepts(
                render_entity,
                FrameAllocationIdentity {
                    entity: render_entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            ) {
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
        }
    }
}

type DrawChunkCommands = (SetItemPipeline, DrawPackedChunk);
type DrawChunkIndirectCommands = (SetItemPipeline, DrawPackedChunksIndirect);

// Both supported paths use `first_instance` to select packed quad records and
// `base_vertex / 4` to select the per-draw origin. Direct drawing is the
// fallback only on adapters that expose BASE_VERTEX.
struct DrawPackedChunk;

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
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            0..STATIC_QUAD_INDICES.len() as u32,
            base_vertex,
            allocation.quad_range.clone(),
        );
        frame_probe.record_direct_draw(item.entity(), identity);
        RenderCommandResult::Success
    }
}

struct DrawPackedChunksIndirect;

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

fn submit_presented_frame_probe(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    frame_probe: Res<ActiveFrameProbe>,
    presented_frame_gate: Res<PresentedFrameGate>,
) {
    let Some(completed_probe) = frame_probe.take_completed() else {
        if let Err(error) = render_device.poll(PollType::Poll) {
            bevy::log::warn!(
                ?error,
                "could not nonblockingly poll presented-frame fences"
            );
        }
        return;
    };
    if !presented_frame_gate.try_reserve_callback(&completed_probe.expectation) {
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
    command_buffer.on_submitted_work_done(move || {
        callback_gate.publish_reserved_probe(completed_probe, present_returned_at, Instant::now());
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
        BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NetworkIdMode,
        TextureMip, encode_blob,
    };
    use bevy::{
        prelude::*,
        render::render_resource::{DownlevelFlags, DrawIndexedIndirectArgs, WgpuFeatures},
    };
    use world::SubChunk;

    use super::*;

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
                    },
                    BlockVisual {
                        faces: [1; 6],
                        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                    },
                ]
                .into_boxed_slice(),
                hashed: Box::new([]),
                materials: vec![Material { layer: 0, flags: 0 }; 2].into_boxed_slice(),
                textures: TextureArray {
                    layers: 1,
                    mips: [16_u32, 8, 4, 2, 1]
                        .into_iter()
                        .map(|size| TextureMip {
                            size,
                            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                },
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
            drawn_manifest: Arc::clone(&expectation.manifest),
            expectation,
            missing_target_instances: 0,
            unexpected_target_instances: 0,
            source_instances: 0,
            foreign_instances: 0,
            stale_generation_instances: 0,
            orphan_allocations: 0,
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

        let same_frame = FrameProbe::begin(target_expectation(now, [(key, 9)]), [instance], []);
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
                quad_range: 17..23,
                metadata_index: 4,
            },
            GpuChunkAllocation {
                key: SubChunkKey::new(0, 1, 0, 0),
                generation: 2,
                quad_range: 4..9,
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
        assert_eq!(gpu_chunk_origin([16, -64, 32], 27), [16, -64, 32, 27]);
        assert_eq!(gpu_chunk_origin([0, 0, 0], 0), [0, 0, 0, 0]);
    }

    #[test]
    fn multi_draw_requires_indirect_execution_and_indirect_first_instance() {
        let indirect = DownlevelFlags::INDIRECT_EXECUTION | DownlevelFlags::BASE_VERTEX;
        let first_instance = WgpuFeatures::INDIRECT_FIRST_INSTANCE
            | WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

        assert_eq!(
            select_chunk_draw_mode(indirect, first_instance),
            ChunkDrawMode::MultiDrawIndirect,
        );
        assert_eq!(
            select_chunk_draw_mode(DownlevelFlags::BASE_VERTEX, first_instance),
            ChunkDrawMode::Direct,
        );
        assert_eq!(
            select_chunk_draw_mode(
                indirect,
                WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            ),
            ChunkDrawMode::Direct,
        );
        assert_eq!(
            select_chunk_draw_mode(DownlevelFlags::empty(), WgpuFeatures::empty()),
            ChunkDrawMode::Unsupported,
        );
        assert_eq!(
            select_chunk_draw_mode(
                DownlevelFlags::INDIRECT_EXECUTION,
                WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            ),
            ChunkDrawMode::Unsupported,
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

        assert_eq!(pack_linear_rgb10([0.0, 0.0, 0.0]), 0);
        assert_eq!(pack_linear_rgb10([1.0, 1.0, 1.0]), 0x3fff_ffff);
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
            })
            .collect::<Vec<_>>();
        let allocations = HashMap::new();

        let selected = plan_gpu_chunk_updates(candidates, &allocations, Vec3::ZERO);

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
            },
            GpuUpdateCandidate {
                entity: fitting,
                key: SubChunkKey::new(0, 10, 0, 0),
                generation: 1,
            },
        ];
        let selected = plan_gpu_chunk_updates(candidates, &HashMap::new(), Vec3::ZERO);
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
            },
            GpuUpdateCandidate {
                entity: near,
                key: near_key,
                generation: 1,
            },
        ];

        let selected =
            plan_gpu_chunk_updates(candidates, &HashMap::new(), Vec3::new(1_608.0, 8.0, 8.0));

        assert_eq!(selected[0], near);
        assert!(
            ChunkUploadPriority::from_camera(near_key, Vec3::new(1_608.0, 8.0, 8.0))
                < ChunkUploadPriority::from_camera(far_key, Vec3::new(1_608.0, 8.0, 8.0))
        );
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
            })
            .collect::<Vec<_>>();
        let selected = plan_gpu_chunk_updates(candidates, &HashMap::new(), Vec3::ZERO);
        let mut quad_len = 0;
        let mut free_quads = Vec::new();
        let mut failed = Vec::new();
        let mut successful = Vec::new();
        for entity in selected {
            let instance = &extracted[&entity];
            let required = u32::try_from(instance.quads.len()).unwrap();
            let token = instance.token.expect("tracked upload token");
            assert!(acknowledgements.try_reserve(instance.key, token));
            if allocate_quad_range(&mut quad_len, &mut free_quads, required, 5).is_none() {
                assert!(acknowledgements.cancel(instance.key, token));
                failed.push(instance.key);
                continue;
            }
            let uploaded_bytes = buffer_byte_len(instance.quads.len(), PACKED_QUAD_BYTES)
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
        assert_eq!(limits.max_origin_items, 2);
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
            max_origin_items: 8,
            max_biome_words: 8,
        };
        let fallback =
            plan_chunk_range_update(0, &[], FALLBACK_BIOME_WORDS, &[], 1, 0, None, limits).unwrap();
        assert_eq!(fallback.biome_start, 0);
        assert_eq!(fallback.biome_capacity, 0);
        assert_eq!(fallback.biome_len, FALLBACK_BIOME_WORDS);

        let real =
            plan_chunk_range_update(0, &[], FALLBACK_BIOME_WORDS, &[], 1, 2, None, limits).unwrap();
        assert_eq!(real.biome_start, FALLBACK_BIOME_WORDS as u32);
        assert_eq!(real.biome_len, FALLBACK_BIOME_WORDS + 2);

        assert!(
            plan_chunk_range_update(
                4,
                &[],
                FALLBACK_BIOME_WORDS,
                &[],
                1,
                1,
                None,
                ArenaLimits {
                    max_quad_items: 8,
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
}
