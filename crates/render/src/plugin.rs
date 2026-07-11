use std::{
    collections::{HashMap, hash_map::Entry},
    ops::Range,
    sync::{Arc, Mutex},
    time::Instant,
};

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
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
            ViewBinnedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferInitDescriptor, BufferUsages, Canonical, ColorTargetState, ColorWrites,
            CommandEncoderDescriptor, CompareFunction, DepthStencilState, DownlevelFlags,
            DrawIndexedIndirectArgs, Face as CullFace, FragmentState, IndexFormat, PipelineCache,
            PrimitiveState, RenderPipeline, RenderPipelineDescriptor, ShaderStages, ShaderType,
            Specializer, SpecializerKey, TextureFormat, Variants, VertexState, WgpuFeatures,
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

use crate::{ChunkMesh, PackedQuad};

const CHUNK_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b5664c91-763f-4e5c-9310-d12659f70cd4");
const STATIC_QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
const PACKED_QUAD_BYTES: u64 = 8;
const CHUNK_ORIGIN_BYTES: u64 = 16;
const INDEXED_INDIRECT_BYTES: u64 = 20;
const DEFAULT_RENDER_QUEUE_ITEMS: usize = 256;
const DEFAULT_RENDER_QUEUE_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_ACKNOWLEDGEMENT_CAPACITY: usize = DEFAULT_RENDER_QUEUE_ITEMS;

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
        self.try_enqueue(key, mesh, priority, None)
    }

    pub fn try_update(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(key, mesh, priority, None)
    }

    pub fn try_update_tracked(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
        token: ChunkUploadToken,
    ) -> Result<(), ChunkMesh> {
        self.try_enqueue(key, mesh, priority, Some(token))
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
                .saturating_sub(mesh_byte_len(&pending.mesh));
        }
        self.removals
            .insert(key, PendingRemoval { priority, token });
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

    fn try_enqueue(
        &mut self,
        key: SubChunkKey,
        mesh: ChunkMesh,
        priority: ChunkUploadPriority,
        token: Option<ChunkUploadToken>,
    ) -> Result<(), ChunkMesh> {
        let old_bytes = self
            .pending
            .get(&key)
            .map_or(0, |pending| mesh_byte_len(&pending.mesh));
        let replaces_existing = self.pending.contains_key(&key) || self.removals.contains_key(&key);
        let next_items = self
            .retained_len()
            .saturating_add(usize::from(!replaces_existing));
        let next_bytes = self
            .pending_bytes
            .saturating_sub(old_bytes)
            .saturating_add(mesh_byte_len(&mesh));
        if next_items > self.limits.max_items || next_bytes > self.limits.max_bytes {
            return Err(mesh);
        }
        self.removals.remove(&key);
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.pending_bytes = next_bytes;
        self.pending.insert(
            key,
            PendingUpload {
                mesh,
                priority,
                generation: self.next_generation,
                token,
            },
        );
        Ok(())
    }
}

fn mesh_byte_len(mesh: &ChunkMesh) -> u64 {
    buffer_byte_len(mesh.quad_count(), PACKED_QUAD_BYTES)
}

/// Extracted packed geometry for one visible, frustum-cullable sub-chunk.
#[derive(Component, Clone, ExtractComponent)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<ChunkRenderInstance>)]
pub struct ChunkRenderInstance {
    key: SubChunkKey,
    quads: Arc<[PackedQuad]>,
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
            .init_resource::<ChunkEntities>()
            .insert_resource(self.upload_budget)
            .add_systems(Update, apply_chunk_render_queue);

        if app.get_sub_app(RenderApp).is_none() {
            return;
        }

        app.add_plugins(ExtractComponentPlugin::<ChunkRenderInstance>::default());

        load_internal_asset!(app, CHUNK_SHADER_HANDLE, "chunk.wgsl", Shader::from_wgsl);

        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();

        app.sub_app_mut(RenderApp)
            .insert_resource(self.upload_budget)
            .insert_resource(acknowledgements)
            .init_resource::<ChunkPipeline>()
            .init_resource::<ChunkGpuUploadStats>()
            .init_resource::<ChunkIndirectBatches>()
            .add_render_command::<Opaque3d, DrawChunkCommands>()
            .add_render_command::<Opaque3d, DrawChunkIndirectCommands>()
            .add_systems(RenderStartup, init_chunk_gpu_arena)
            .add_systems(
                Render,
                (
                    queue_chunks.in_set(RenderSystems::Queue),
                    prepare_gpu_chunks.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_indirect_batches
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_chunk_bind_group.in_set(RenderSystems::PrepareBindGroups),
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
            .saturating_sub(mesh_byte_len(&pending.mesh));
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
    let base_vertex = allocation
        .metadata_index
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())?;
    Some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex,
        first_instance: allocation.quad_range.start,
    })
}

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
    indirect_offset: u64,
    command_count: u32,
}

#[derive(Resource, Default)]
struct ChunkIndirectBatches(HashMap<Entity, ChunkIndirectBatch>);

#[derive(Clone)]
struct ArenaAllocation {
    generation: u64,
    quad_capacity: u32,
    gpu: GpuChunkAllocation,
}

#[derive(Clone, PartialEq, Eq)]
struct ChunkBindGroupBuffers {
    view: BufferId,
    quads: BufferId,
    origins: BufferId,
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
    ArenaLimits {
        max_quad_items,
        max_origin_items,
    }
}

#[derive(Resource)]
struct ChunkGpuArena {
    quad_buffer: Buffer,
    origin_buffer: Buffer,
    index_buffer: Buffer,
    indirect_buffer: Buffer,
    bind_group: Option<BindGroup>,
    bind_group_buffers: Option<ChunkBindGroupBuffers>,
    quad_capacity: usize,
    origin_capacity: usize,
    indirect_capacity: usize,
    quad_len: usize,
    origin_len: usize,
    limits: ArenaLimits,
    free_quads: Vec<Range<u32>>,
    free_origins: Vec<u32>,
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
            indirect_capacity: 1,
            quad_len: 0,
            origin_len: 0,
            limits,
            free_quads: Vec::new(),
            free_origins: Vec::new(),
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
        let Some((quad_start, quad_capacity)) =
            allocate_for_chunk_update(&mut arena, required, old.as_ref())
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
        let origin = [
            instance.origin[0],
            instance.origin[1],
            instance.origin[2],
            0,
        ];
        quad_writes.push((quad_start, words));
        origin_writes.push((metadata_index, origin));
        let gpu = GpuChunkAllocation {
            quad_range: quad_start..quad_end,
            metadata_index,
        };
        commands.entity(entity).insert(gpu.clone());
        arena.allocations.insert(
            entity,
            ArenaAllocation {
                generation: instance.generation,
                quad_capacity,
                gpu,
            },
        );
        if let Some(token) = instance.token {
            let uploaded_bytes = buffer_byte_len(instance.quads.len(), PACKED_QUAD_BYTES)
                .saturating_add(CHUNK_ORIGIN_BYTES);
            applied_tokens.push((instance.key, token, uploaded_bytes));
        }
        chunk_updates += 1;
    }

    let quad_incremental_bytes = quad_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_BYTES))
    });
    let origin_incremental_bytes = buffer_byte_len(origin_writes.len(), CHUNK_ORIGIN_BYTES);
    let quad_gpu_copy_bytes = ensure_quad_capacity(&mut arena, &render_device, &render_queue);
    let origin_gpu_copy_bytes = ensure_origin_capacity(&mut arena, &render_device, &render_queue);
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
    let applied_at = Instant::now();
    for (key, token, uploaded_bytes) in applied_tokens {
        acknowledgements.complete_with_bytes(key, token, applied_at, uploaded_bytes);
    }

    *upload_stats = account_chunk_gpu_uploads(
        *budget,
        chunk_updates,
        quad_incremental_bytes,
        origin_incremental_bytes,
        quad_gpu_copy_bytes,
        origin_gpu_copy_bytes,
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
    required: u32,
    old: Option<&ArenaAllocation>,
) -> Option<(u32, u32)> {
    if let Some(old) = old
        && required <= old.quad_capacity
    {
        return Some((old.gpu.quad_range.start, old.quad_capacity));
    }

    let mut next_len = arena.quad_len;
    let mut next_free = arena.free_quads.clone();
    if let Some(old) = old {
        let freed =
            old.gpu.quad_range.start..old.gpu.quad_range.start.saturating_add(old.quad_capacity);
        release_quad_range(&mut next_len, &mut next_free, freed);
    }
    let start = allocate_quad_range(
        &mut next_len,
        &mut next_free,
        required,
        arena.limits.max_quad_items,
    )?;
    arena.quad_len = next_len;
    arena.free_quads = next_free;
    Some((start, required))
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
    quad_gpu_copy_bytes: u64,
    origin_gpu_copy_bytes: u64,
) -> ChunkGpuUploadStats {
    let incremental_bytes = quad_incremental_bytes.saturating_add(origin_incremental_bytes);
    let gpu_copy_bytes = quad_gpu_copy_bytes.saturating_add(origin_gpu_copy_bytes);
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
    mut arena: ResMut<ChunkGpuArena>,
) {
    let Some(view_buffer) = view_uniforms.uniforms.buffer() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let buffers = ChunkBindGroupBuffers {
        view: view_buffer.id(),
        quads: arena.quad_buffer.id(),
        origins: arena.origin_buffer.id(),
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
        ],
    );
    arena.bind_group = Some(bind_group);
    arena.bind_group_buffers = Some(buffers);
}

fn prepare_chunk_indirect_batches(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    allocations: Query<&GpuChunkAllocation>,
    mut batches: ResMut<ChunkIndirectBatches>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let mut all_commands = Vec::new();
    for batch in batches.0.values_mut() {
        let indirect_commands = build_indexed_indirect_commands(
            batch
                .visible_entities
                .iter()
                .filter_map(|entity| allocations.get(*entity).ok()),
        );
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
    allocations: Query<(), With<GpuChunkAllocation>>,
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
        if !*unsupported_reported {
            bevy::log::error!(
                "packed chunk renderer requires DownlevelFlags::BASE_VERTEX; this adapter is unsupported"
            );
            *unsupported_reported = true;
        }
        return;
    }
    *unsupported_reported = false;
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
            );

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
            if allocations.get(render_entity).is_err() {
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
    type Param = SRes<ChunkGpuArena>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        _item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        arena: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let Some(base_vertex) = allocation
            .metadata_index
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            0..STATIC_QUAD_INDICES.len() as u32,
            base_vertex,
            allocation.quad_range.clone(),
        );
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
    type Param = (SRes<ChunkGpuArena>, SRes<ChunkIndirectBatches>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
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
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use bevy::{
        prelude::*,
        render::render_resource::{DownlevelFlags, DrawIndexedIndirectArgs, WgpuFeatures},
    };

    use super::*;

    #[test]
    fn indexed_indirect_commands_preserve_order_and_encode_quad_and_origin_ranges() {
        let allocations = [
            GpuChunkAllocation {
                quad_range: 17..23,
                metadata_index: 4,
            },
            GpuChunkAllocation {
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
            growth.gpu_copy_bytes,
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
        let impossible_mesh =
            crate::mesh_sub_chunk(&classifier, &crate::Neighbourhood::empty(), &solid);
        let fitting_mesh = crate::mesh_sub_chunk(
            &classifier,
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
