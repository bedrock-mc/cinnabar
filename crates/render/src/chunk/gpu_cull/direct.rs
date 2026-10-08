//! Occlusion for direct draws, without indirect submission. While the camera holds still and a
//! verdict could change, solid terrain draws in its own pass ahead of every other opaque draw,
//! so its depth holds only static, fully opaque geometry; a pyramid of it tests every resident
//! slot, and the bits come back asynchronously for later frames to skip what stays occluded.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    render::{
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_phase::DrawFunctionId,
        render_resource::{CachedRenderPipelineId, RenderPassDescriptor, StoreOp, TextureViewId},
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};

use crate::chunk::*;
use crate::gpu_timing::readback::{ReadbackRing, SLOTS};

use super::{
    kernels::{CullKernels, OcclusionStorage, occlusion_bytes},
    model::{CullPhase, CullViewUniform},
    occlusion::{OcclusionBasis, OcclusionHistory, VerdictTag},
    prepare::{
        ChunkHiddenEntities, CullViewComponents, PreparedPyramid, cull_view_input, prepare_pyramid,
        sampleable_depth, write_dirty_records,
    },
    slots::{CullSlots, cull_record},
};

const MIN_CAPACITY: u32 = 1024;
const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

pub(in crate::chunk) fn direct_occlusion_supported(
    draw_mode: ChunkDrawMode,
    downlevel: DownlevelFlags,
    forced_cpu: bool,
) -> bool {
    !forced_cpu
        && draw_mode == ChunkDrawMode::Direct
        && downlevel.contains(DownlevelFlags::COMPUTE_SHADERS)
}

/// The direct-drawn view chosen for occlusion this frame, filled while queueing.
#[derive(Resource, Default)]
pub(in crate::chunk) struct DirectOcclusionFrame {
    view: Option<QueuedView>,
    /// Render entities offered by the view, and whether each has solid cube faces.
    candidates: Vec<(Entity, bool)>,
    last_pose: Option<(Entity, Mat4, Mat4)>,
}

#[derive(Clone, Copy)]
struct QueuedView {
    entity: Entity,
    solid_pipeline: CachedRenderPipelineId,
    solid_draw: DrawFunctionId,
    terrain_pass: bool,
}

impl DirectOcclusionFrame {
    pub(in crate::chunk) fn clear(&mut self) {
        self.view = None;
        self.candidates.clear();
    }

    /// Starts the view's frame; `true` routes its solid terrain to the terrain pass, which
    /// runs only while the pose matches the previous frame's and a verdict is wanted.
    pub(in crate::chunk) fn begin(
        &mut self,
        entity: Entity,
        view: &ExtractedView,
        (solid_pipeline, solid_draw): (CachedRenderPipelineId, DrawFunctionId),
        wants_verdict: bool,
    ) -> bool {
        let pose = (
            entity,
            view.world_from_view.to_matrix(),
            view.clip_from_view,
        );
        let terrain_pass = wants_verdict && self.last_pose == Some(pose);
        self.last_pose = Some(pose);
        self.view = Some(QueuedView {
            entity,
            solid_pipeline,
            solid_draw,
            terrain_pass,
        });
        terrain_pass
    }

    pub(in crate::chunk) fn push(&mut self, entity: Entity, solid: bool) {
        self.candidates.push((entity, solid));
    }
}

/// Per-frame work counters of the direct occlusion path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct DirectOcclusionStats {
    /// Frustum- and cave-visible sub-chunks offered by the view.
    pub(in crate::chunk) candidates: u32,
    pub(in crate::chunk) skipped: u32,
    /// Sub-chunks whose solid faces the terrain pass drew.
    pub(in crate::chunk) solid_drawn: u32,
    pub(in crate::chunk) verdicts_applied: u64,
    /// Frames whose verdict was dropped because every readback slot was in flight.
    pub(in crate::chunk) verdicts_dropped: u64,
    /// Times every verdict was voided because resident geometry changed.
    pub(in crate::chunk) world_invalidations: u64,
}

/// Readback slot bookkeeping; a full ring drops that frame's verdict rather than wait.
#[derive(Default)]
pub(in crate::chunk) struct VerdictQueue {
    ring: ReadbackRing,
    tags: [Option<VerdictTag>; SLOTS],
    states: [Arc<AtomicU8>; SLOTS],
}

impl VerdictQueue {
    pub(in crate::chunk) fn acquire(&mut self, tag: VerdictTag) -> Option<usize> {
        let slot = self.ring.acquire()?;
        self.tags[slot] = Some(tag);
        Some(slot)
    }

    pub(in crate::chunk) fn release(&mut self, slot: usize) {
        self.tags[slot] = None;
        self.ring.release(slot);
    }

    /// Marks `slot` in flight; the returned state is for its map callback.
    pub(in crate::chunk) fn submit(&mut self, slot: usize) -> Arc<AtomicU8> {
        self.states[slot].store(PENDING, Ordering::Relaxed);
        self.ring.submit(slot);
        self.states[slot].clone()
    }

    /// Hands completed slots to `sink` oldest first, with `None` for a failed map.
    pub(in crate::chunk) fn drain(&mut self, mut sink: impl FnMut(usize, Option<&VerdictTag>)) {
        while let Some(slot) = self.ring.oldest_in_flight() {
            match self.states[slot].load(Ordering::Acquire) {
                PENDING => break,
                MAPPED => sink(slot, self.tags[slot].as_ref()),
                _ => sink(slot, None),
            }
            self.release(slot);
        }
    }

    /// Forgets every slot; callbacks still in flight land on detached states.
    fn reset(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(in crate::chunk) fn complete(&self, slot: usize, ok: bool) {
        self.states[slot].store(if ok { MAPPED } else { FAILED }, Ordering::Release);
    }
}

struct TerrainPassPlan {
    view: Entity,
    solid_pipeline: CachedRenderPipelineId,
    solid_draw: DrawFunctionId,
    solid: Vec<(Entity, MainEntity)>,
    verdict: Option<usize>,
    slots: u32,
    encoded: AtomicBool,
}

/// Slot records, the pyramid, verdict readbacks and the skip set of the direct-drawn view.
#[derive(Resource)]
pub(in crate::chunk) struct DirectOcclusion {
    kernels: CullKernels,
    storage: Option<OcclusionStorage>,
    readbacks: Vec<wgpu::Buffer>,
    table: CullSlots,
    generations: HashMap<Entity, u64>,
    tint: Option<ChunkBiomeTintIdentity>,
    pyramid: Option<PreparedPyramid>,
    bind_group: Option<(wgpu::BindGroup, TextureViewId)>,
    verdicts: VerdictQueue,
    history: OcclusionHistory,
    frame: u64,
    skip_view: Option<Entity>,
    skipped: HashSet<Entity>,
    /// The view as last prepared, which decides whether the next frame wants a verdict.
    last_view: Option<(Entity, OcclusionBasis)>,
    plan: Option<TerrainPassPlan>,
    pub(in crate::chunk) stats: DirectOcclusionStats,
}

impl DirectOcclusion {
    fn new(device: &RenderDevice) -> Self {
        Self {
            kernels: CullKernels::new(device.wgpu_device()),
            storage: None,
            readbacks: Vec::new(),
            table: CullSlots::default(),
            generations: HashMap::new(),
            tint: None,
            pyramid: None,
            bind_group: None,
            verdicts: VerdictQueue::default(),
            history: OcclusionHistory::default(),
            frame: 0,
            skip_view: None,
            skipped: HashSet::new(),
            last_view: None,
            plan: None,
            stats: DirectOcclusionStats::default(),
        }
    }

    /// Whether a verdict for `view` could change what it skips; a settled still view needs none.
    pub(in crate::chunk) fn wants_verdict(
        &self,
        entity: Entity,
        view: &ExtractedView,
        msaa: Msaa,
    ) -> bool {
        !self.last_view.is_some_and(|(last_entity, last)| {
            last_entity == entity
                && OcclusionBasis {
                    depth_size: last.depth_size,
                    world: last.world,
                    ..view_basis(view, msaa.samples())
                } == last
                && self.history.settled(&last)
        })
    }

    /// Whether `view` skips `entity`'s opaque terrain this frame.
    pub(in crate::chunk) fn skips(&self, view: Entity, entity: Entity) -> bool {
        self.skip_view == Some(view) && self.skipped.contains(&entity)
    }

    fn sync_records(
        &mut self,
        changed: &ChangedAllocations,
        removed: impl Iterator<Item = Entity>,
        tint: ChunkBiomeTintIdentity,
        hidden: &mut ChunkHiddenEntities,
    ) {
        let none = HashSet::new();
        // One void per batch: clearing every run per removal is quadratic on a session reset.
        let mut uncovered = false;
        for entity in removed {
            if self.table.contains(entity) {
                self.table.remove(entity);
                self.generations.remove(&entity);
                uncovered = true;
            }
        }
        uncovered |= self.tint.replace(tint).is_some_and(|old| old != tint);
        for (entity, allocation, instance) in changed {
            let record = cull_record(allocation, instance);
            let slot = allocation.metadata_index;
            let fresh = !self.table.contains(entity);
            let same = self.generations.insert(entity, allocation.generation)
                == Some(allocation.generation)
                && self.table.records().get(slot as usize) == Some(&record);
            if fresh || !same {
                uncovered |= !fresh;
                self.history.assign(slot, self.frame);
            }
            self.table
                .update(entity, slot, allocation.tint_identity, record, &none);
        }
        // Cave culling hid or revealed something; what it hid may have occluded others.
        if !hidden.changed.is_empty() {
            hidden.changed.clear();
            uncovered = true;
        }
        if uncovered {
            self.history.invalidate_world();
            self.stats.world_invalidations += 1;
        }
        self.table.trim();
    }

    fn upload(&mut self, device: &RenderDevice, queue: &RenderQueue) {
        let slots = self.table.slot_count();
        if self
            .storage
            .as_ref()
            .is_none_or(|storage| storage.capacity < slots)
        {
            let capacity = slots.max(MIN_CAPACITY).next_power_of_two();
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.occlusion_allocate",
                capacity,
                readback_bytes = occlusion_bytes(capacity) * SLOTS as u64
            )
            .entered();
            let device = device.wgpu_device();
            self.storage = Some(OcclusionStorage::new(device, capacity));
            self.readbacks = (0..SLOTS)
                .map(|_| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("terrain occlusion readback"),
                        size: occlusion_bytes(capacity),
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })
                .collect();
            self.verdicts.reset();
            self.bind_group = None;
            self.table.mark_all_dirty();
        }
        let storage = self.storage.as_ref().expect("storage was just ensured");
        write_dirty_records(&mut self.table, queue, &storage.records);
    }

    fn apply_verdicts(&mut self) {
        let Self {
            verdicts,
            readbacks,
            history,
            stats,
            ..
        } = self;
        verdicts.drain(|slot, tag| {
            let Some(tag) = tag else {
                return;
            };
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.occlusion_readback_apply",
                slot,
                frame = tag.frame,
                records = tag.slots,
                bytes = readbacks[slot].size(),
            )
            .entered();
            let buffer = &readbacks[slot];
            history.apply(
                tag,
                bytemuck::cast_slice(&buffer.slice(..).get_mapped_range()),
            );
            buffer.unmap();
            stats.verdicts_applied += 1;
        });
    }
}

type ChangedAllocations<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GpuChunkAllocation,
        Option<&'static ChunkRenderInstance>,
    ),
    Changed<GpuChunkAllocation>,
>;

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_direct_occlusion(
    mut occlusion: ResMut<DirectOcclusion>,
    mut hidden: ResMut<ChunkHiddenEntities>,
    frame: Res<DirectOcclusionFrame>,
    changed: ChangedAllocations,
    allocations: Query<(&GpuChunkAllocation, &MainEntity)>,
    mut removed: RemovedComponents<GpuChunkAllocation>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    biome_tints: Res<ChunkBiomeTints>,
    views: Query<CullViewComponents>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::IndirectPreparation));
    let occlusion = &mut *occlusion;
    occlusion.frame += 1;
    occlusion.plan = None;
    occlusion.skip_view = None;
    occlusion.last_view = None;
    occlusion.skipped.clear();
    for entity in removed_instances.read() {
        hidden.hidden.remove(&entity);
    }
    occlusion.sync_records(
        &changed,
        removed.read(),
        biome_tints.table_identity(),
        &mut hidden,
    );
    occlusion.upload(&device, &queue);
    // Readbacks mapped by the previous frame's device poll.
    occlusion.apply_verdicts();

    let Some(queued) = frame.view else {
        return;
    };
    let Ok((extracted, frustum, depth, msaa, resolution_override)) = views.get(queued.entity)
    else {
        return;
    };
    let depth = sampleable_depth(depth, resolution_override);
    let size = depth.map(|depth| depth.texture.size());
    let current = OcclusionBasis {
        depth_size: size.map_or([0; 2], |size| [size.width, size.height]),
        world: occlusion.history.world(),
        ..view_basis(extracted, msaa.samples())
    };
    occlusion.skip_view = Some(queued.entity);
    occlusion.last_view = Some((queued.entity, current));
    let mut solid = Vec::new();
    for &(entity, has_solid) in &frame.candidates {
        let Ok((allocation, _)) = allocations.get(entity) else {
            continue;
        };
        if occlusion.history.skips(allocation.metadata_index, &current) {
            occlusion.skipped.insert(entity);
        } else if has_solid && queued.terrain_pass {
            solid.push((entity, allocation.key));
        }
    }
    occlusion.stats.candidates = frame.candidates.len() as u32;
    occlusion.stats.skipped = occlusion.skipped.len() as u32;
    occlusion.stats.solid_drawn = solid.len() as u32;
    if !queued.terrain_pass {
        return;
    }
    let solid = front_to_back_cube_entities(solid, &extracted.rangefinder3d())
        .into_iter()
        .filter_map(|entity| Some((entity, *allocations.get(entity).ok()?.1)))
        .collect();
    prepare_pyramid(
        &mut occlusion.pyramid,
        &occlusion.kernels,
        &device,
        depth,
        *msaa,
    );
    let slots = occlusion.table.slot_count();
    let verdict = occlusion.pyramid.as_ref().and_then(|prepared| {
        let storage = occlusion.storage.as_ref()?;
        let input = cull_view_input(
            extracted,
            frustum,
            prepared.pyramid.depth_size,
            prepared.pyramid.mip_count(),
        );
        let uniform = CullViewUniform::new(&input, CullPhase::Late, slots, storage.capacity);
        {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.occlusion_uniform_write", frame = occlusion.frame)
                    .entered();
            queue.write_buffer(&storage.uniform, 0, bytemuck::bytes_of(&uniform));
        }
        if occlusion
            .bind_group
            .as_ref()
            .is_none_or(|(_, depth)| *depth != prepared.depth)
        {
            let group = occlusion.kernels.occlusion_bind_group(
                device.wgpu_device(),
                storage,
                &prepared.pyramid,
            );
            occlusion.bind_group = Some((group, prepared.depth));
        }
        let tag = VerdictTag {
            frame: occlusion.frame,
            basis: current,
            slots,
        };
        let slot = occlusion.verdicts.acquire(tag);
        if slot.is_none() {
            occlusion.stats.verdicts_dropped += 1;
        }
        slot
    });
    occlusion.plan = Some(TerrainPassPlan {
        view: queued.entity,
        solid_pipeline: queued.solid_pipeline,
        solid_draw: queued.solid_draw,
        solid,
        verdict,
        slots,
        encoded: AtomicBool::new(false),
    });
}

/// Maps the verdict the terrain pass copied out; a frame that never encoded it frees the slot.
pub(super) fn submit_direct_occlusion(mut occlusion: ResMut<DirectOcclusion>) {
    let occlusion = &mut *occlusion;
    let Some(plan) = occlusion.plan.take() else {
        return;
    };
    let Some(slot) = plan.verdict else {
        return;
    };
    if !plan.encoded.load(Ordering::Acquire) {
        occlusion.verdicts.release(slot);
        return;
    }
    let state = occlusion.verdicts.submit(slot);
    #[cfg(feature = "tracy")]
    let frame = occlusion.frame;
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("terrain.occlusion_map_request", slot, frame).entered();
    occlusion.readbacks[slot]
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.occlusion_map_callback",
                slot,
                frame,
                ok = result.is_ok()
            )
            .entered();
            state.store(
                if result.is_ok() { MAPPED } else { FAILED },
                Ordering::Release,
            );
        });
}

/// The view's pose half of a verdict basis; depth size and world come from prepare.
pub(super) fn view_basis(view: &ExtractedView, depth_samples: u32) -> OcclusionBasis {
    OcclusionBasis {
        eye: view.world_from_view.translation().to_array(),
        view_rotation: view.world_from_view.affine().matrix3.to_cols_array(),
        clip_from_view: view.clip_from_view.to_cols_array(),
        viewport: view.viewport.to_array(),
        depth_size: [0; 2],
        depth_samples,
        world: 0,
    }
}

pub(super) fn reset_direct_occlusion_frame(mut frame: ResMut<DirectOcclusionFrame>) {
    frame.clear();
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct TerrainPassLabel;

pub(super) fn install_graph(world: &mut World) {
    let node = ViewNodeRunner::new(TerrainPassNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(TerrainPassLabel, node);
    graph.add_node_edges((
        Node3d::StartMainPass,
        TerrainPassLabel,
        Node3d::MainOpaquePass,
    ));
}

/// Draws solid terrain front to back, then tests every slot against the depth it left.
#[derive(Default)]
struct TerrainPassNode;

impl ViewNode for TerrainPassNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, target, scene_target, depth, resolution_override): (
            &'w ExtractedCamera,
            &'w ViewTarget,
            &'w crate::scene_target::SceneTarget,
            &'w ViewDepthTexture,
            Option<&'w MainPassResolutionOverride>,
        ),
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph.view_entity();
        let occlusion = world.resource::<DirectOcclusion>();
        let Some(plan) = occlusion
            .plan
            .as_ref()
            .filter(|plan| plan.view == view_entity)
        else {
            return Ok(());
        };
        {
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("terrain solid pass"),
                color_attachments: &[Some(scene_target.color_attachment(target, false))],
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuTerrainOpaque,
                ),
                occlusion_query_set: None,
            });
            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
            {
                pass.set_camera_viewport(&viewport);
            }
            let draw_functions = world.resource::<DrawFunctions<Opaque3d>>();
            let mut draw_functions = draw_functions.write();
            draw_functions.prepare(world);
            if let Some(draw) = draw_functions.get_mut(plan.solid_draw) {
                for &(entity, main) in &plan.solid {
                    let item = <Opaque3d as bevy::render::render_phase::BinnedPhaseItem>::new(
                        Opaque3dBatchSetKey {
                            draw_function: plan.solid_draw,
                            pipeline: plan.solid_pipeline,
                            material_bind_group_index: None,
                            lightmap_slab: None,
                            vertex_slab: default(),
                            index_slab: None,
                        },
                        Opaque3dBinKey {
                            asset_id: AssetId::<Mesh>::invalid().untyped(),
                        },
                        (entity, main),
                        0..1,
                        PhaseItemExtraIndex::None,
                    );
                    if let Err(error) = draw.draw(world, &mut pass, view_entity, &item) {
                        bevy::log::error!("terrain solid pass draw failed: {error:?}");
                    }
                }
            }
        }
        let (Some(slot), Some(prepared), Some((group, _)), Some(storage)) = (
            plan.verdict,
            occlusion.pyramid.as_ref(),
            occlusion.bind_group.as_ref(),
            occlusion.storage.as_ref(),
        ) else {
            return Ok(());
        };
        let encoder = render_context.command_encoder();
        occlusion
            .kernels
            .encode_pyramid(encoder, &prepared.pyramid, &prepared.bindings);
        occlusion
            .kernels
            .encode_occlusion(encoder, group, plan.slots);
        encoder.copy_buffer_to_buffer(
            &storage.occluded,
            0,
            &occlusion.readbacks[slot],
            0,
            occlusion_bytes(storage.capacity),
        );
        plan.encoded.store(true, Ordering::Release);
        Ok(())
    }
}

/// Skips a direct terrain draw the occlusion verdicts hid from this view.
pub(in crate::chunk) struct SkipOccludedTerrain;

impl<P: PhaseItem> RenderCommand<P> for SkipOccludedTerrain {
    type Param = Option<SRes<DirectOcclusion>>;
    type ViewQuery = Entity;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view: Entity,
        _: Option<()>,
        occlusion: SystemParamItem<'w, '_, Self::Param>,
        _: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        if occlusion.is_some_and(|occlusion| occlusion.into_inner().skips(view, item.entity())) {
            RenderCommandResult::Skip
        } else {
            RenderCommandResult::Success
        }
    }
}

/// Inserts the direct occlusion path's resources, systems and graph node.
pub(super) fn install(render_app: &mut SubApp, device: &RenderDevice) {
    render_app
        .insert_resource(DirectOcclusion::new(device))
        .init_resource::<ChunkHiddenEntities>()
        .add_systems(ExtractSchedule, super::prepare::extract_hidden_chunks)
        .add_systems(
            Render,
            (
                prepare_direct_occlusion
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare_gpu_chunks)
                    .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
                submit_direct_occlusion.in_set(crate::device_poll::FrameSubmissions),
            ),
        );
    install_graph(render_app.world_mut());
}
