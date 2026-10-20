//! The render-world geometry resource: preparing expansion work and draw lists each frame, and
//! recording the expansion before any camera draws.

use std::collections::HashMap;

use bevy::log::{debug, warn};
use bevy::math::{DVec3, IVec3, Vec3};
use bevy::prelude::{Entity, FromWorld, Has, Query, Res, ResMut, Resource, World, default};
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_phase::TrackedRenderPass;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntry, Buffer, BufferDescriptor, BufferId, BufferInitDescriptor,
    BufferUsages, ComputePassDescriptor, PipelineCache,
};
use bevy::render::renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedView, RenderVisibleEntities};
use render_scene::{ChunkBiomeTintIdentity, ChunkRenderInstance, ChunkTextureAssets};

use super::block_ids::{ShaderpackBlockIds, material_attribute_words};
use super::draw::{DrawInputs, ViewDraws, face_indices};
use super::expansion_buffers::{ExpansionBuffers, PARAM_WORDS, frame_job_capacity};
use super::jobs::{
    GeometryCategory, JOB_KINDS, JobKind, MaterialTables, SectionPlan, plan_section,
};
use super::pipelines::{
    EXPAND_WORKGROUP_SIZE, ExpansionPipelines, output_layout, source_bindings, source_layout,
    storage_buffers_per_stage,
};
use super::residency::{
    DEFAULT_GEOMETRY_BUDGET_BYTES, Eviction, Placement, Residency, ResidencyLog, SECTION_RADIUS,
    section_distance,
};
use super::shadow_cull::ShadowFrustum;
use super::vertex::{CONSTANT_STREAM_WORDS, DRAW_ORIGIN_BYTES, TerrainLayout, draw_origin};
use crate::chunk::shaderpack_sources::{
    PackedChunkBuffers, PackedChunkSources, ResidentSections, terrain_multi_draw_indirect,
};
use crate::shaderpack::atlas::ShaderpackAtlas;

/// Faces expanded per frame outside an origin move, bounding the frame's expansion work: a
/// streaming burst spreads over frames instead of costing one frame its planning (~25 µs a
/// section on the render thread) and dispatch. About 27 average sections, 2.3M faces a
/// second at 140 fps, above the rate sections arrive from the server.
pub const FRAME_EXPANSION_FACES: u32 = 1 << 14;
/// Camera travel, in blocks, after which sections waiting for room are tried again.
const RETRY_DISTANCE: f64 = 16.0;

/// Host controls of the geometry adapter, in the render world.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct ShaderpackGeometrySettings {
    /// Expand and track terrain; while false expanded GPU buffers are released.
    pub enabled: bool,
    /// The pack's `separateAo`: AO goes to the colour's alpha instead of its RGB.
    pub separate_ao: bool,
    /// The pack's `oldLighting`: bake vanilla directional face shading into the colour.
    pub old_lighting: bool,
    /// Bytes the expanded vertex pages may use; sections beyond it wait, undrawn, until
    /// others leave.
    pub budget_bytes: u64,
    /// The expanded vertex the pack's terrain pipelines read.
    pub layout: TerrainLayout,
}

impl Default for ShaderpackGeometrySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            separate_ao: false,
            old_lighting: false,
            budget_bytes: DEFAULT_GEOMETRY_BUDGET_BYTES,
            layout: TerrainLayout::WIDE,
        }
    }
}

/// Everything whose change rewrites every expanded vertex.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExpansionInputs {
    materials: BufferId,
    model_templates: BufferId,
    biome_tints: BufferId,
    biome_tint_table: ChunkBiomeTintIdentity,
    atlas_generation: u64,
    attribute_key: AttributeKey,
    separate_ao: bool,
    old_lighting: bool,
}

impl ExpansionInputs {
    /// Refreshes changed inputs, retaining drawable faces during palette-only updates.
    fn refresh_residency(
        &self,
        previous: Option<&Self>,
        rebased: bool,
        residency: &mut Residency,
        entities: impl IntoIterator<Item = Entity>,
    ) {
        let previous = previous.cloned().map(|mut inputs| {
            inputs.biome_tints = self.biome_tints;
            inputs
        });
        if !rebased && previous.as_ref() == Some(self) {
            residency.enqueue_all(entities);
        } else {
            residency.invalidate_contents(entities);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeKey {
    assets: usize,
    revision: u64,
    block_ids: Option<u64>,
}

struct Dispatch {
    kind: JobKind,
    sources: BindGroup,
    output: BindGroup,
    jobs: u32,
}

/// Expansion recorded by [`record_geometry_expansion`].
#[derive(Default)]
struct PreparedExpansion {
    dispatches: Vec<Dispatch>,
}

/// Iris-ready terrain geometry for shader packs. See the module documentation for the host
/// contract.
#[derive(Resource)]
pub struct ShaderpackGeometry {
    residency: Residency,
    pages: Vec<Buffer>,
    index_buffer: Option<Buffer>,
    index_faces: u32,
    constants: Buffer,
    attributes: Vec<u32>,
    attribute_key: Option<AttributeKey>,
    inputs: Option<ExpansionInputs>,
    /// The expanded vertex pages hold; see [`ShaderpackGeometrySettings::layout`].
    layout: TerrainLayout,
    /// Per-draw origins of packed positions, one per residency slot, while the layout packs
    /// them.
    draw_origins: Option<Buffer>,
    /// Entries `draw_origins` holds.
    draw_origin_capacity: u32,
    binding_bytes: u64,
    /// Camera draw lists by render-world view entity.
    views: HashMap<Entity, ViewDraws>,
    /// Shadow-pass draw lists by the camera view they cast shadows for.
    shadows: HashMap<Entity, ViewDraws>,
    multi_draw: bool,
    supported: bool,
    prepared: PreparedExpansion,
    /// The attribute table, jobs and parameters of each frame's expansion.
    expansion: ExpansionBuffers,
    overflow_reported: bool,
    /// Eye distance within which every visible section is expanded; `None` when all are.
    complete_radius: Option<f32>,
    /// Eye distance of the nearest section the vertex budget left unexpanded at its last
    /// overflow; `None` once no section waits.
    budget_horizon: Option<f32>,
    /// Camera position when waiting sections were last retried.
    retry_eye: Option<DVec3>,
    /// The vertex pages' size when the device last ran out of memory creating one; the
    /// budget stays below it, so nearer sections take room from farther ones instead.
    memory_cap: Option<u64>,
    log: ResidencyLog,
}

impl FromWorld for ShaderpackGeometry {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let adapter = world.resource::<RenderAdapter>();
        Self::new(device, terrain_multi_draw_indirect(adapter, device))
    }
}

impl ShaderpackGeometry {
    /// Creates empty expansion state for the device's native terrain draw mode.
    fn new(device: &RenderDevice, draw_mode: Option<bool>) -> Self {
        let limits = device.limits();
        let supported = draw_mode.is_some()
            && JOB_KINDS.iter().all(|kind| {
                storage_buffers_per_stage(*kind) <= limits.max_storage_buffers_per_shader_stage
            });
        let binding_bytes = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        Self {
            residency: Residency::new(
                binding_bytes,
                DEFAULT_GEOMETRY_BUDGET_BYTES,
                TerrainLayout::WIDE.stride(),
            ),
            pages: Vec::new(),
            index_buffer: None,
            index_faces: 0,
            constants: device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shader-pack constant vertex stream"),
                contents: bytemuck::cast_slice(&CONSTANT_STREAM_WORDS),
                usage: BufferUsages::VERTEX,
            }),
            attributes: Vec::new(),
            attribute_key: None,
            inputs: None,
            layout: TerrainLayout::WIDE,
            draw_origins: None,
            draw_origin_capacity: 0,
            binding_bytes,
            views: HashMap::new(),
            shadows: HashMap::new(),
            multi_draw: draw_mode == Some(true),
            supported,
            prepared: PreparedExpansion::default(),
            expansion: ExpansionBuffers::default(),
            overflow_reported: false,
            complete_radius: None,
            budget_horizon: None,
            retry_eye: None,
            memory_cap: None,
            log: ResidencyLog::default(),
        }
    }
}

impl ShaderpackGeometry {
    /// Whether this device can expand and draw terrain for packs at all.
    pub fn supported(&self) -> bool {
        self.supported
    }

    /// Whether every live section can fit a page; otherwise the host keeps the native scene.
    pub fn all_sections_fit(&self) -> bool {
        self.inputs.is_some() && !self.residency.has_unplaceable()
    }

    /// Block position every expanded vertex position is relative to; it moves (re-expanding
    /// all terrain) when the camera strays [`super::residency::ORIGIN_REBASE_DISTANCE`] away.
    pub fn origin(&self) -> Option<IVec3> {
        self.residency.origin
    }

    /// The pack's `chunkOffset` (and the translation to fold into `gl_ModelViewMatrix`) for a
    /// camera at `camera`: the origin relative to the camera, computed in double precision.
    pub fn chunk_offset(&self, camera: DVec3) -> Option<Vec3> {
        self.residency
            .origin
            .map(|origin| (origin.as_dvec3() - camera).as_vec3())
    }

    /// Eye distance within which every section the camera may see is expanded, or `None`
    /// when nothing waits. Beyond it terrain may be missing from the pack's geometry.
    pub fn complete_radius(&self) -> Option<f32> {
        self.complete_radius
    }

    /// Eye distance past which the vertex budget leaves terrain undrawn, `None` while every
    /// section fits. Unlike [`Self::complete_radius`] it ignores sections merely queued for
    /// this frame's expansion, so it holds steady while terrain streams in.
    pub fn budget_horizon(&self) -> Option<f32> {
        self.budget_horizon
    }

    /// The expanded vertex the pages hold, which terrain pipelines must read.
    pub(crate) fn layout(&self) -> TerrainLayout {
        self.layout
    }

    /// Records `category` for `view` (a render-world view entity with visible chunks): its
    /// camera list, or with `shadow` its shadow-pass list from
    /// [`Self::prepare_shadow_view`]. The pass must already hold a pipeline built with
    /// [`super::vertex_buffer_layouts`] and the program's bind groups. Returns the draws
    /// issued.
    pub fn draw<'w>(
        &'w self,
        pass: &mut TrackedRenderPass<'w>,
        category: GeometryCategory,
        view: Entity,
        shadow: bool,
    ) -> u32 {
        let Some(index_buffer) = &self.index_buffer else {
            return 0;
        };
        let lists = if shadow { &self.shadows } else { &self.views };
        let Some(draws) = lists.get(&view) else {
            return 0;
        };
        let origins = self.draw_origins.as_ref().filter(|_| self.layout.packed());
        draws.record(
            pass,
            category,
            &self.pages,
            index_buffer,
            &self.constants,
            origins,
        )
    }

    /// Builds (or keeps, when the sections it lists are unchanged) `view`'s shadow-pass list:
    /// the resident sections `frustum` keeps.
    pub(crate) fn prepare_shadow_view(
        &mut self,
        view: Entity,
        frustum: &ShadowFrustum,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let inputs = DrawInputs::new(
            self.residency
                .sections
                .values()
                .filter(|faces| frustum.visible(faces.origin)),
            None,
            self.residency.epoch,
            self.residency.generation,
            None,
        );
        let key = inputs.key();
        let draws = self.shadows.entry(view).or_default();
        if draws.is_current(key) {
            return;
        }
        let built = ViewDraws::build(&inputs);
        let max_faces = built.max_faces();
        draws.replace(built, key, device, queue, self.multi_draw);
        if max_faces > 0 {
            self.ensure_index_buffer(device, max_faces);
        }
    }

    /// Drops the shadow lists of views no longer drawn.
    pub(crate) fn retain_shadow_views(&mut self, mut keep: impl FnMut(Entity) -> bool) {
        self.shadows.retain(|view, _| keep(*view));
    }

    /// Releases resident and transient expansion buffers and withdraws every draw list.
    fn release_all(&mut self) {
        self.residency.clear();
        self.pages.clear();
        self.index_buffer = None;
        self.index_faces = 0;
        self.inputs = None;
        self.views.clear();
        self.shadows.clear();
        self.prepared = PreparedExpansion::default();
        self.expansion = ExpansionBuffers::default();
        self.attributes.clear();
        self.attribute_key = None;
        self.complete_radius = None;
        self.budget_horizon = None;
        self.retry_eye = None;
        self.memory_cap = None;
        self.draw_origins = None;
        self.draw_origin_capacity = 0;
    }

    /// Switches pages to expanded vertices of `layout`, dropping every page.
    fn set_layout(&mut self, layout: TerrainLayout) {
        self.release_all();
        self.layout = layout;
        self.residency = Residency::new(
            self.binding_bytes,
            DEFAULT_GEOMETRY_BUDGET_BYTES,
            layout.stride(),
        );
    }

    /// Writes the draw origin of the section in residency slot `slot`, at block `section`,
    /// for positions relative to `anchor`; a full table is replaced by one twice the size
    /// holding every resident section's origin.
    fn write_draw_origin(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        slot: u32,
        section: [i32; 3],
        anchor: IVec3,
    ) {
        if slot >= self.draw_origin_capacity || self.draw_origins.is_none() {
            self.write_draw_origins(device, queue, anchor);
            return;
        }
        if let Some(buffer) = &self.draw_origins {
            queue.write_buffer(
                buffer,
                u64::from(slot) * DRAW_ORIGIN_BYTES,
                bytemuck::cast_slice(&draw_origin(section, anchor.to_array())),
            );
        }
    }

    /// Writes every resident section's draw origin for positions relative to `anchor`, into
    /// a new table when the current one cannot hold every slot. Packed vertices are
    /// section-relative, so this alone moves them to a new anchor.
    fn write_draw_origins(&mut self, device: &RenderDevice, queue: &RenderQueue, anchor: IVec3) {
        let anchor = anchor.to_array();
        let slots = self.residency.slot_count();
        if slots > self.draw_origin_capacity || self.draw_origins.is_none() {
            let capacity = slots.next_power_of_two().max(1024);
            self.draw_origins = Some(device.create_buffer(&BufferDescriptor {
                label: Some("shader-pack terrain draw origins"),
                size: u64::from(capacity) * DRAW_ORIGIN_BYTES,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.draw_origin_capacity = capacity;
        }
        let mut entries = vec![[0.0_f32; 4]; slots as usize];
        for resident in self.residency.sections.values() {
            entries[resident.slot as usize] = draw_origin(resident.origin, anchor);
        }
        if let Some(buffer) = &self.draw_origins {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&entries));
        }
    }

    /// Creates vertex page `placement.page` when the placement opened it. Returns false when
    /// the device is out of memory.
    fn create_page(&mut self, device: &RenderDevice, placement: &Placement) -> bool {
        if !placement.new_page {
            return true;
        }
        let capacity = self.residency.pages[placement.page].faces.capacity();
        let Some(buffer) = crate::gpu_allocation::buffer(
            device,
            &BufferDescriptor {
                label: Some("shader-pack terrain vertices"),
                size: u64::from(capacity) * self.residency.face_bytes(),
                usage: BufferUsages::VERTEX | BufferUsages::STORAGE,
                mapped_at_creation: false,
            },
        ) else {
            return false;
        };
        debug_assert_eq!(placement.page, self.pages.len(), "new pages come last");
        self.pages.push(buffer);
        true
    }

    fn ensure_index_buffer(&mut self, device: &RenderDevice, faces: u32) {
        if faces <= self.index_faces && self.index_buffer.is_some() {
            return;
        }
        let faces = faces.max(1).next_power_of_two();
        self.index_buffer = Some(device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("shader-pack terrain face indices"),
            contents: bytemuck::cast_slice(&face_indices(faces)),
            usage: BufferUsages::INDEX,
        }));
        self.index_faces = faces;
    }
}

fn source_group(
    device: &RenderDevice,
    cache: &PipelineCache,
    kind: JobKind,
    buffers: &PackedChunkBuffers<'_>,
) -> BindGroup {
    let buffer = |binding| match binding {
        1 => buffers.quads,
        2 => buffers.origins,
        3 => buffers.materials,
        7 => buffers.biome_records,
        8 => buffers.biome_tints,
        12 => buffers.model_templates,
        _ => buffers.geometry_streams,
    };
    let mut entries: Vec<_> = source_bindings(kind)
        .iter()
        .map(|&binding| BindGroupEntry {
            binding,
            resource: buffer(binding).as_entire_binding(),
        })
        .collect();
    entries.extend([
        BindGroupEntry {
            binding: super::pipelines::ANIMATION_CLOCK_BINDING,
            resource: buffers.animation_clock.as_entire_binding(),
        },
        BindGroupEntry {
            binding: crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            resource: buffers.biome_query_tables.as_entire_binding(),
        },
    ]);
    device.create_bind_group(
        "shader-pack expansion sources",
        &cache.get_bind_group_layout(&source_layout(kind)),
        &entries,
    )
}

/// System parameters of [`prepare_shaderpack_geometry`].
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct GeometryInputs<'w> {
    settings: Option<Res<'w, ShaderpackGeometrySettings>>,
    textures: Option<Res<'w, ChunkTextureAssets>>,
    atlas: Option<Res<'w, ShaderpackAtlas>>,
    block_ids: Option<Res<'w, ShaderpackBlockIds>>,
    pipelines: Res<'w, ExpansionPipelines>,
    cache: Res<'w, PipelineCache>,
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
}

type GeometryViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExtractedView,
        &'static RenderVisibleEntities,
        Has<ExtractedCamera>,
    ),
>;

/// Plans this frame's expansion and rebuilds every view's draw lists.
pub(crate) fn prepare_shaderpack_geometry(
    mut geometry: ResMut<ShaderpackGeometry>,
    inputs: GeometryInputs,
    sources: PackedChunkSources,
    mut sections: ResidentSections,
    views: GeometryViews,
) {
    geometry.prepared = PreparedExpansion::default();
    let settings = inputs.settings.as_deref().cloned().unwrap_or_default();
    if !settings.enabled || !geometry.supported {
        geometry.release_all();
        return;
    }
    if settings.layout != geometry.layout {
        geometry.set_layout(settings.layout);
    }
    let geometry = &mut *geometry;
    geometry.residency.set_budget(
        settings
            .budget_bytes
            .min(geometry.memory_cap.unwrap_or(u64::MAX)),
    );
    let removed = sections.take_removed();
    let freed = !removed.is_empty();
    for removed in removed {
        geometry.residency.remove(removed);
    }
    let changed: Vec<_> = sections.changed().collect();
    if changed.len() > 512 {
        debug!(target: "cinnabar::shaderpack", sections = changed.len(), "shader-pack terrain sections changed");
    }
    geometry
        .residency
        .observe_frame(freed || !changed.is_empty());
    for changed in changed {
        geometry.residency.enqueue(changed);
    }
    let camera = views
        .iter()
        .find(|(.., camera)| *camera)
        .map(|(_, view, ..)| view.world_from_view.translation().as_dvec3());
    // Sections waiting for room get another chance once room is freed or the camera has moved
    // enough that some of them may now be nearer than resident ones.
    let moved = camera.is_some_and(|camera| {
        geometry
            .retry_eye
            .is_none_or(|last| last.distance(camera) > RETRY_DISTANCE)
    });
    if freed || moved {
        geometry.residency.retry_waiting();
        geometry.retry_eye = camera.or(geometry.retry_eye);
    }
    // Everything below needs the chunk tables, the atlas and compiled kernels.
    let (Some(buffers), Some(textures), Some(atlas)) = (
        sources.buffers(),
        inputs.textures.as_deref(),
        inputs.atlas.as_deref(),
    ) else {
        geometry.release_all();
        return;
    };
    if !expansion_sources_match(textures, &atlas.source, buffers.carrier) {
        geometry.release_all();
        return;
    }
    let kernels_ready = geometry.pipelines_ready(&inputs.cache, &inputs.pipelines);
    let attribute_key = AttributeKey {
        assets: std::sync::Arc::as_ptr(&textures.assets) as usize,
        revision: textures.revision,
        block_ids: inputs.block_ids.as_deref().map(|ids| ids.revision),
    };
    if geometry.attribute_key.as_ref() != Some(&attribute_key) {
        geometry.attributes = material_attribute_words(
            &textures.assets,
            inputs.block_ids.as_deref().map(|ids| &*ids.block_entity_id),
        );
        geometry.attribute_key = Some(attribute_key.clone());
    }
    let expansion_inputs = ExpansionInputs {
        materials: buffers.materials.id(),
        model_templates: buffers.model_templates.id(),
        biome_tints: buffers.biome_tints.id(),
        biome_tint_table: buffers.tint_identity,
        atlas_generation: atlas.generation,
        attribute_key,
        separate_ao: settings.separate_ao,
        old_lighting: settings.old_lighting,
    };
    let rebased = camera.is_some_and(|camera| geometry.residency.follow_camera(camera));
    // Packed vertices are section-relative: a rebase only moves each draw's origin.
    let reexpand_rebased = rebased && !geometry.layout.packed();
    if rebased
        && !reexpand_rebased
        && let Some(origin) = geometry.residency.origin
    {
        geometry.residency.keep_faces_across_rebase();
        geometry.write_draw_origins(&inputs.device, &inputs.queue, origin);
    }
    if reexpand_rebased || geometry.inputs.as_ref() != Some(&expansion_inputs) {
        debug!(
            target: "cinnabar::shaderpack",
            rebased,
            previous = ?geometry.inputs,
            current = ?expansion_inputs,
            "shader-pack terrain re-expands every resident section"
        );
        let resident: Vec<_> = sections.entities().collect();
        expansion_inputs.refresh_residency(
            geometry.inputs.as_ref(),
            reexpand_rebased,
            &mut geometry.residency,
            resident,
        );
        geometry.inputs = Some(expansion_inputs);
    }
    let mut expanded = 0;
    if kernels_ready && let Some(origin) = geometry.residency.origin {
        let budget = if reexpand_rebased {
            u32::MAX
        } else {
            FRAME_EXPANSION_FACES
        };
        expanded = plan_frame(
            geometry, &inputs, &buffers, &sections, textures, atlas, origin, &settings, budget,
            camera,
        );
    }
    // A page whose sections fit in the others' free room drains, its sections moving, and
    // goes once empty; the draw lists name pages by index and are rebuilt.
    if let Some(page) = geometry.residency.finish_draining() {
        geometry.pages.remove(page);
        geometry.views.clear();
        geometry.shadows.clear();
    } else {
        geometry.residency.start_draining();
    }
    geometry.complete_radius = camera.and_then(|eye| {
        geometry
            .residency
            .pending()
            // Palette refreshes and page moves keep their existing drawable faces.
            .filter(|entity| !geometry.residency.drawable(*entity))
            .filter_map(|entity| sections.get(entity, buffers.tint_identity))
            .map(|section| section_distance(section.origin, eye))
            .min_by(f64::total_cmp)
            .map(|nearest| (nearest.sqrt() - SECTION_RADIUS).max(0.0) as f32)
    });
    geometry
        .log
        .observe(&geometry.residency, expanded, geometry.complete_radius);
    if geometry.residency.pending_len() == 0 {
        geometry.budget_horizon = None;
    }
    build_draws(geometry, &inputs.device, &inputs.queue, &sources, &views);
}

impl ShaderpackGeometry {
    fn pipelines_ready(&self, cache: &PipelineCache, pipelines: &ExpansionPipelines) -> bool {
        pipelines
            .pipelines
            .iter()
            .all(|id| cache.get_compute_pipeline(*id).is_some())
    }
}

#[allow(clippy::too_many_arguments, reason = "one frame's planning inputs")]
fn plan_frame(
    geometry: &mut ShaderpackGeometry,
    inputs: &GeometryInputs,
    buffers: &PackedChunkBuffers<'_>,
    sections: &ResidentSections,
    textures: &ChunkTextureAssets,
    atlas: &ShaderpackAtlas,
    origin: IVec3,
    settings: &ShaderpackGeometrySettings,
    budget: u32,
    eye: Option<DVec3>,
) -> u32 {
    let tables = MaterialTables {
        materials: textures.assets.materials(),
        model_templates: textures.assets.model_templates(),
        model_quads: textures.assets.model_quads(),
    };
    let device = &*inputs.device;
    // Jobs per (kind, page), with face indices relative to the page.
    let mut jobs: HashMap<(usize, usize), Vec<[u32; 4]>> = HashMap::new();
    let mut planned_faces = 0_u32;
    let mut planned_jobs = 0;
    let job_capacity = frame_job_capacity(&device.limits(), geometry.attributes.len());
    let mut overflow = false;
    // Nearest first, so the budget and the frame's expansion go to what the camera sees most.
    let mut queue: Vec<(f64, Entity)> = geometry
        .residency
        .take_queue()
        .into_iter()
        .filter_map(|entity| {
            let section = sections.get(entity, buffers.tint_identity)?;
            Some((
                eye.map_or(0.0, |eye| section_distance(section.origin, eye)),
                entity,
            ))
        })
        .collect();
    queue.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut eviction = Eviction::default();
    let mut queue = queue.into_iter();
    while let Some((distance, entity)) = queue.next() {
        if planned_faces >= budget {
            geometry.residency.enqueue(entity);
            queue
                .by_ref()
                .for_each(|(_, rest)| geometry.residency.enqueue(rest));
            break;
        }
        let Some(section) = sections.get(entity, buffers.tint_identity) else {
            continue;
        };
        let mut plan: SectionPlan = plan_section(&section.geometry, &tables);
        if plan.faces == 0 {
            geometry.residency.remove(entity);
            continue;
        }
        let job_count: usize = plan.jobs.iter().map(Vec::len).sum();
        if job_count > job_capacity {
            // The device cannot bind even one section with these tables; keep native terrain.
            geometry.release_all();
            return 0;
        }
        if planned_jobs + job_count > job_capacity {
            geometry.residency.enqueue(entity);
            queue
                .by_ref()
                .for_each(|(_, rest)| geometry.residency.enqueue(rest));
            break;
        }

        let placement = geometry.residency.place_nearest(
            entity,
            &plan,
            eye.map(|eye| (eye, distance)),
            &mut eviction,
        );
        let Some(placement) = placement else {
            if !geometry.residency.can_fit(plan.faces) {
                // A changed mesh or budget can make this section fit later. Keep expanding
                // the remaining sections rather than parking them behind this request.
                geometry.residency.wait(entity);
                continue;
            }
            // Nothing farther is left to give up its room: this and every farther section
            // wait until room frees up or the camera moves.
            overflow = true;
            geometry.budget_horizon =
                eye.map(|_| (distance.sqrt() - SECTION_RADIUS).max(0.0) as f32);
            geometry.residency.wait(entity);
            queue
                .by_ref()
                .for_each(|(_, rest)| geometry.residency.wait(rest));
            break;
        };
        if !geometry.create_page(device, &placement) {
            // Out of device memory: keep the pages there are, and let nearer sections take
            // room from farther ones from now on.
            geometry.residency.revoke(entity, &placement);
            let cap = geometry.residency.capacity_faces() * geometry.residency.face_bytes();
            geometry.memory_cap = Some(cap);
            geometry
                .residency
                .set_budget(settings.budget_bytes.min(cap));
            queue
                .by_ref()
                .for_each(|(_, rest)| geometry.residency.enqueue(rest));
            break;
        }
        plan.place(placement.faces.start);
        let slot = geometry
            .residency
            .commit(entity, &placement, &plan, section.origin);
        geometry
            .residency
            .sections
            .get_mut(&entity)
            .expect("committed above")
            .translucent = section.translucent_faces(&plan, textures);
        if geometry.layout.packed() {
            geometry.write_draw_origin(device, &inputs.queue, slot, section.origin, origin);
        }
        for (kind, planned) in plan.jobs.into_iter().enumerate() {
            jobs.entry((kind, placement.page))
                .or_default()
                .extend(planned);
        }
        planned_faces = planned_faces.saturating_add(plan.faces);
        planned_jobs += job_count;
    }
    if overflow && !geometry.overflow_reported {
        warn!(
            "shader-pack terrain exceeds its {} MiB vertex budget; distant sections wait undrawn",
            settings.budget_bytes >> 20
        );
    }
    geometry.overflow_reported |= overflow;
    if jobs.is_empty() {
        return planned_faces;
    }
    let mut keys: Vec<_> = jobs.keys().copied().collect();
    keys.sort_unstable();
    let planned: Vec<&[[u32; 4]]> = keys.iter().map(|key| jobs[key].as_slice()).collect();
    geometry.expansion.write(
        device,
        &inputs.queue,
        &geometry.attributes,
        expand_params(origin, settings, &geometry.attributes),
        &planned,
    );
    let job_buffer = geometry.expansion.jobs().expect("written above");
    let output_layout = inputs.cache.get_bind_group_layout(&output_layout());
    let mut source_groups: [Option<BindGroup>; 3] = Default::default();
    for (index, (kind_index, page)) in keys.into_iter().enumerate() {
        let kind = JOB_KINDS[kind_index];
        let output = device.create_bind_group(
            "shader-pack expansion output",
            &output_layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: job_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: geometry.pages[page].as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: atlas.remap.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: geometry.expansion.params(index).expect("written above"),
                },
            ],
        );
        let sources = source_groups[kind_index]
            .get_or_insert_with(|| source_group(device, &inputs.cache, kind, buffers))
            .clone();
        geometry.prepared.dispatches.push(Dispatch {
            kind,
            sources,
            output,
            jobs: planned[index].len() as u32,
        });
    }
    planned_faces
}

/// The kernels' `ExpandParams` words: the origin, `separateAo`, `oldLighting`, the first job
/// word, the materials in the attribute table and the vertex layout.
fn expand_params(
    origin: IVec3,
    settings: &ShaderpackGeometrySettings,
    attributes: &[u32],
) -> [u32; PARAM_WORDS] {
    let mut params = [0_u32; PARAM_WORDS];
    params[..3].copy_from_slice(&origin.to_array().map(|axis| axis as u32));
    params[4] = u32::from(settings.separate_ao);
    params[5] = u32::from(settings.old_lighting);
    params[6] = attributes.len() as u32;
    params[7] = (attributes.len() / 2) as u32;
    params[8..12].copy_from_slice(&settings.layout.expand_words());
    params
}

/// Draw lists per camera view of its visible resident sections, leaving out faces the camera
/// sees only from behind. A view's list is rebuilt only when the sections it lists, the
/// camera's section or the facing groups it sees change.
fn build_draws(
    geometry: &mut ShaderpackGeometry,
    device: &RenderDevice,
    queue: &RenderQueue,
    sources: &PackedChunkSources,
    views: &GeometryViews,
) {
    let epoch = geometry.residency.epoch;
    let mut max_faces = 0;
    geometry
        .views
        .retain(|entity, _| views.get(*entity).is_ok());
    for (entity, view, visible, _) in views.iter() {
        let eye = view.world_from_view.translation().as_dvec3();
        let inputs = DrawInputs::new(
            visible
                .get::<ChunkRenderInstance>()
                .into_iter()
                .flat_map(|class| class.iter_visible())
                .map(|(entity, _)| *entity)
                .filter(|entity| !sources.is_hidden(*entity))
                .filter_map(|entity| geometry.residency.sections.get(&entity)),
            Some(eye),
            epoch,
            geometry.residency.generation,
            Some(eye),
        );
        let key = inputs.key();
        if geometry
            .views
            .get(&entity)
            .is_some_and(|draws| draws.is_current(key))
        {
            continue;
        }
        let built = ViewDraws::build(&inputs);
        max_faces = max_faces.max(built.max_faces());
        geometry.views.entry(entity).or_default().replace(
            built,
            key,
            device,
            queue,
            geometry.multi_draw,
        );
    }
    if max_faces > 0 {
        geometry.ensure_index_buffer(device, max_faces);
    }
}

/// Records this frame's expansion dispatches ahead of camera passes.
pub(crate) fn record_geometry_expansion(
    mut geometry: ResMut<ShaderpackGeometry>,
    pipelines: Res<ExpansionPipelines>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let prepared = std::mem::take(&mut geometry.prepared);
    if prepared.dispatches.is_empty() {
        return;
    }
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("shader-pack terrain expansion"),
            ..default()
        });
    for dispatch in &prepared.dispatches {
        let Some(pipeline) =
            cache.get_compute_pipeline(pipelines.pipelines[dispatch.kind as usize])
        else {
            continue;
        };
        let groups = dispatch.jobs.div_ceil(EXPAND_WORKGROUP_SIZE);
        if groups == 0 {
            continue;
        }
        let x = groups.min(u16::MAX as u32);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &dispatch.sources, &[]);
        pass.set_bind_group(1, &dispatch.output, &[]);
        pass.dispatch_workgroups(x, groups.div_ceil(x), 1);
    }
}

/// Requires the atlas and packed tables to represent the selected texture source.
fn expansion_sources_match(
    selected: &ChunkTextureAssets,
    atlas: &ChunkTextureAssets,
    packed: render_scene::ChunkTextureAssetIdentity,
) -> bool {
    selected.binding_identity() == atlas.binding_identity() && selected.identity() == packed
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
