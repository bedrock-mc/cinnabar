//! The render world's distant terrain state and its per-frame preparation: takes the
//! frame's hand-off, builds the GPU objects off the render thread when drawing turns on,
//! uploads updates within the budget, orders water, and writes the view and staging.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bevy::{
    camera::{MainPassResolutionOverride, primitives::Frustum, visibility::RenderLayers},
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    ecs::{
        entity::Entity,
        resource::Resource,
        system::{Query, Res, ResMut},
    },
    render::{
        render_resource::TextureViewId,
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, Msaa, ViewDepthStencilTexture},
    },
};
use lod::select::shown::DrawUpdate;
use render_api::DistantTerrainTier;
use render_scene::{
    AtmosphereFrame,
    distant_terrain::{
        DistantTerrainColourTables, DistantTerrainLink, DistantTerrainRenderStats,
        ResidentColumnMask,
    },
};

use super::{
    budget::UploadBudget,
    gpu::{LodGpu, TargetKey},
    layout::{Capacity, Target, WORKGROUP},
    one_build::{OneBuild, Polled},
    readback::LodReadbacks,
    staging::Staging,
    table::SectionTable,
    translucent::TranslucentSorter,
    view::{PackedTables, ViewInputs, lod_view, perspective_eye},
};
use crate::chunk::{HizPyramid, PyramidBindings, gpu_cull_view_eligible};

/// The pyramid of the drawn view's depth.
pub(crate) struct LodPyramid {
    pub(crate) pyramid: HizPyramid,
    pub(crate) bindings: PyramidBindings,
    /// The depth view it seeds from; `None` when built for a test target.
    depth: Option<TextureViewId>,
}

impl LodPyramid {
    /// A pyramid seeded through `bindings`, for a target the renderer does not track.
    #[cfg(test)]
    pub(crate) fn new(pyramid: HizPyramid, bindings: PyramidBindings) -> Self {
        Self {
            pyramid,
            bindings,
            depth: None,
        }
    }
}

/// The colour tables on the GPU.
struct Tables {
    buffer: wgpu::Buffer,
    packed: PackedTables,
}

/// What the pass system encodes this frame.
pub(crate) struct LodFrame {
    pub(crate) view: Entity,
    /// Zero every slot first: the drawn set was reset.
    pub(crate) clear: bool,
    pub(crate) scatter_groups: u32,
    pub(crate) section_groups: u32,
    pub(crate) cluster_groups: u32,
    pub(crate) order_groups: u32,
    pub(crate) cull_group: wgpu::BindGroup,
    pub(crate) draw_group: wgpu::BindGroup,
    pub(crate) readback: Option<usize>,
    /// Set by the pass system once it encoded the frame's passes.
    pub(crate) encoded: AtomicBool,
}

/// One drawing renderer: the GPU objects and the CPU tables that mirror them.
pub(crate) struct LodState {
    pub(crate) gpu: LodGpu,
    table: SectionTable,
    staging: Staging,
    sorter: TranslucentSorter,
    tables: Option<Tables>,
    lightmap: Option<wgpu::Buffer>,
    draw_group: Option<wgpu::BindGroup>,
    pub(crate) pyramid: Option<LodPyramid>,
    cull_group: Option<(wgpu::BindGroup, Option<TextureViewId>)>,
    /// Slots must be zeroed before the next scatter.
    clear: bool,
    readbacks: LodReadbacks,
    /// The resident mask must be written again.
    resident_dirty: bool,
    /// The last water order did not fit the staging; space stays reserved until one does.
    order_stale: bool,
}

/// See the module documentation.
#[derive(Resource)]
pub(crate) struct LodRenderer {
    link: DistantTerrainLink,
    generation: u64,
    supported: bool,
    /// The device's caps.
    tier: DistantTerrainTier,
    /// The pool and target drawing is set up for, built or not.
    target: Option<(Capacity, TargetKey)>,
    /// Pools and pipelines being built off the render thread, one at a time.
    builds: OneBuild<(Capacity, TargetKey), LodGpu>,
    /// The counters of a renderer with nothing allocated were published; nothing changes
    /// them until drawing turns on again.
    published_off: bool,
    pub(crate) state: Option<LodState>,
    pub(crate) frame: Option<LodFrame>,
    colours: Option<DistantTerrainColourTables>,
    resident: Option<Arc<ResidentColumnMask>>,
    /// An update taken while nothing could apply it; the link stays busy until it is.
    waiting: Option<DrawUpdate>,
    budget: UploadBudget,
    stats: DistantTerrainRenderStats,
}

impl LodRenderer {
    /// A renderer fed by `link` with `tier`'s caps; `supported` is false where the device
    /// cannot draw.
    pub(crate) fn new(link: DistantTerrainLink, supported: bool, tier: DistantTerrainTier) -> Self {
        Self {
            link,
            generation: 0,
            supported,
            tier,
            target: None,
            builds: OneBuild::default(),
            published_off: false,
            state: None,
            frame: None,
            colours: None,
            resident: None,
            waiting: None,
            budget: UploadBudget::default(),
            stats: DistantTerrainRenderStats {
                supported: Some(supported),
                tier: Some(tier),
                ..Default::default()
            },
        }
    }

    /// Drops every GPU object, freeing the pool; a build still running finishes and is
    /// dropped by a later [`Self::poll_building`].
    fn free(&mut self) {
        self.waiting = None;
        self.target = None;
        self.builds.want(None);
        self.state = None;
        self.frame = None;
    }

    /// Sets drawing up for `capacity` and `key`: anything else drawn so far is dropped and
    /// the new objects are built, after any build still running; true when that dropped or
    /// abandoned an earlier target.
    fn retarget(&mut self, capacity: Capacity, key: TargetKey) -> bool {
        let wanted = (capacity, key);
        if self.target == Some(wanted) {
            return false;
        }
        let replaced = self.target.is_some();
        self.free();
        self.target = Some(wanted);
        self.builds.want(Some(wanted));
        replaced
    }

    /// Installs finished objects and starts the wanted build once none runs; false while
    /// nothing is installed.
    fn poll_building(&mut self, device: &RenderDevice) -> bool {
        if self.state.is_some() {
            return true;
        }
        let timestamps = device
            .wgpu_device()
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        let polled = self.builds.poll(|(capacity, key)| {
            let device = device.wgpu_device().clone();
            move || LodGpu::new(&device, capacity, key, timestamps)
        });
        match polled {
            Polled::Ready(gpu) => {
                let staging = Staging::new(gpu.capacity.staging_words());
                self.state = Some(LodState {
                    table: SectionTable::new(gpu.capacity),
                    gpu,
                    staging,
                    sorter: TranslucentSorter::default(),
                    tables: None,
                    lightmap: None,
                    draw_group: None,
                    pyramid: None,
                    cull_group: None,
                    clear: true,
                    readbacks: LodReadbacks::default(),
                    resident_dirty: true,
                    order_stale: false,
                });
                true
            }
            Polled::Pending => false,
            Polled::Failed => {
                bevy::log::warn!("distant terrain pipeline build failed");
                self.free();
                self.supported = false;
                false
            }
        }
    }
}

/// The view distant terrain draws into: the lowest-id unmirrored perspective view on render
/// layer 0 with a sampleable depth target.
type ViewItem<'a> = (
    Entity,
    &'a ExtractedView,
    &'a Frustum,
    &'a MainEntity,
    Option<&'a RenderLayers>,
    Option<&'a ViewDepthStencilTexture>,
    &'a Msaa,
    Option<&'a MainPassResolutionOverride>,
);

/// Prepares the frame; see the module documentation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_distant_terrain(
    mut renderer: ResMut<LodRenderer>,
    views: Query<ViewItem>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    atmosphere: Option<Res<AtmosphereFrame>>,
    lightmap: Option<Res<crate::lighting::LightmapGpu>>,
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("distant_terrain.prepare").entered();
    let renderer = &mut *renderer;
    renderer.frame = None;
    let inbox = renderer.link.take();
    if let Some(colours) = inbox.colours {
        renderer.colours = Some(colours);
        if let Some(state) = &mut renderer.state {
            state.tables = None;
        }
    }
    if let Some(mask) = inbox.resident {
        renderer.resident = Some(mask);
        if let Some(state) = &mut renderer.state {
            state.resident_dirty = true;
        }
    }
    let settings = inbox.settings.filter(|_| renderer.supported);
    let Some(settings) = settings else {
        if inbox.update.is_some() {
            renderer.link.committed();
        }
        if renderer.builds.running() {
            // Drops what the build makes once it finished.
            let _ = renderer.poll_building(&device);
        }
        if !renderer.published_off {
            renderer.free();
            publish(renderer, false);
            renderer.published_off = true;
        }
        return;
    };
    renderer.published_off = false;
    let Some((entity, view, frustum, eye, depth, msaa)) = select_view(&views) else {
        hold(renderer, inbox.update);
        publish(renderer, false);
        return;
    };
    let key = TargetKey {
        colour: view.target_format,
        depth: CORE_3D_DEPTH_FORMAT,
        samples: msaa.samples(),
    };
    // The pool follows the distance in steps of a doubling, so only crossing one rebuilds it.
    let distance_chunks = renderer.tier.clamp_distance(settings.distance_chunks);
    let pool_bytes = renderer.tier.pool_bytes(settings.quality, distance_chunks);
    let Some(capacity) = Capacity::new(pool_bytes, &device.limits()) else {
        renderer.supported = false;
        renderer.free();
        publish(renderer, false);
        return;
    };
    if renderer.retarget(capacity, key) {
        // A new pool or target: start over from nothing drawn; the frame restarts selection
        // when it sees the reset count move on.
        renderer.stats.resets_requested += 1;
    }
    if inbox.generation != renderer.generation {
        renderer.generation = inbox.generation;
        renderer.waiting = None;
        if let Some(state) = &mut renderer.state {
            reset(state);
        }
    }
    if !renderer.poll_building(&device) {
        hold(renderer, inbox.update);
        publish(renderer, false);
        return;
    }
    let state = renderer.state.as_mut().expect("polled ready");
    if let Some(update) = inbox.update.or_else(|| renderer.waiting.take()) {
        state.table.start(update);
    }
    prepare_tables(
        state,
        renderer.colours.as_ref(),
        &device,
        &queue,
        lightmap.as_deref(),
    );
    // Nothing is staged or freed in a frame that cannot draw, so no write is lost.
    let Some(draw_group) = state.draw_group.clone() else {
        publish(renderer, false);
        return;
    };
    prepare_pyramid(state, &device, depth, msaa);
    state.staging.clear();
    // A water order naming freed clusters must be replaced before they are reused.
    if !state.order_stale {
        state.table.begin_frame();
    }
    let step = {
        #[cfg(feature = "tracy")]
        let _span =
            bevy::log::info_span!("distant_terrain.upload", budget = renderer.budget.bytes())
                .entered();
        state
            .table
            .step(&mut state.staging, renderer.budget.bytes())
    };
    if step.committed {
        renderer.link.committed();
    }
    let changed = state.table.take_translucent_changed() || state.order_stale;
    let distance = distance_chunks * world::SUB_CHUNK_SIDE as u32;
    if let Some(order) = state
        .sorter
        .update(state.table.translucent(), eye, distance, changed)
    {
        state.order_stale = !state.staging.copy(Target::Order, 0, order);
        if state.order_stale {
            state.sorter.reset();
        }
    }
    if state.resident_dirty
        && let Some(mask) = &renderer.resident
        && state.staging.copy(
            Target::Sections,
            state.gpu.capacity.resident_word(),
            &mask.words,
        )
    {
        state.resident_dirty = false;
    }
    let atmosphere = atmosphere.as_deref().copied().unwrap_or_default();
    let packed = state.tables.as_ref().map(|tables| &tables.packed);
    let empty = PackedTables::default();
    let counts = [
        state.table.section_slots(),
        state.table.cluster_slots(),
        state.sorter.order().len() as u32,
        state.gpu.capacity.clusters,
    ];
    let uniform = lod_view(
        view,
        frustum,
        &ViewInputs {
            eye,
            depth_size: state
                .pyramid
                .as_ref()
                .map_or([1, 1], |prepared| prepared.pyramid.depth_size),
            hiz_mips: state
                .pyramid
                .as_ref()
                .map_or(0, |prepared| prepared.pyramid.mip_count()),
            counts,
            resident: renderer.resident.as_deref(),
            atmosphere: &atmosphere,
            tables: packed.unwrap_or(&empty),
        },
    );
    queue.write_buffer(&state.gpu.view, 0, bytemuck::bytes_of(&uniform));
    if !state.staging.is_empty()
        && let Some(size) = std::num::NonZeroU64::new(state.staging.written_bytes())
        && let Some(mut out) = queue.write_buffer_with(&state.gpu.staging, 0, size)
    {
        for (offset, bytes) in state.staging.parts() {
            out.slice(offset..offset + bytes.len())
                .copy_from_slice(bytes);
        }
    }
    let hiz = state.pyramid.as_ref().and_then(|prepared| prepared.depth);
    if state
        .cull_group
        .as_ref()
        .is_none_or(|(_, bound)| *bound != hiz)
    {
        let view = state
            .pyramid
            .as_ref()
            .map(|prepared| &prepared.pyramid.view);
        state.cull_group = Some((state.gpu.cull_group(device.wgpu_device(), view), hiz));
    }
    let period = queue.get_timestamp_period();
    if let Some(readout) = state.readbacks.drain(&state.gpu, period) {
        readout.apply(&mut renderer.stats);
    }
    let groups = |count: u32| count.div_ceil(WORKGROUP);
    let readback = state.readbacks.acquire();
    renderer.frame = Some(LodFrame {
        view: entity,
        clear: std::mem::take(&mut state.clear),
        scatter_groups: state.staging.command_count(),
        section_groups: groups(counts[0]),
        cluster_groups: groups(counts[1]),
        order_groups: groups(counts[2]),
        cull_group: state.cull_group.as_ref().expect("bound above").0.clone(),
        draw_group,
        readback,
        encoded: AtomicBool::new(false),
    });
    renderer
        .budget
        .observe(inbox.frame_seconds, step.uploaded_bytes != 0);
    renderer.stats.uploaded_bytes = state.staging.payload_bytes();
    publish(renderer, true);
}

/// Keeps an update taken while nothing can apply it, so it is applied once something can.
fn hold(renderer: &mut LodRenderer, update: Option<DrawUpdate>) {
    if let Some(update) = update {
        renderer.waiting = Some(update);
    }
}

/// Forgets everything drawn; the next frame zeroes every slot first.
fn reset(state: &mut LodState) {
    state.table = SectionTable::new(state.gpu.capacity);
    state.sorter.reset();
    state.clear = true;
    state.resident_dirty = true;
    state.order_stale = false;
}

/// The drawn view: its entity, view, frustum, eye, sampleable depth target and samples.
type SelectedView<'a> = (
    Entity,
    &'a ExtractedView,
    &'a Frustum,
    [f64; 3],
    Option<&'a ViewDepthStencilTexture>,
    Msaa,
);

/// The drawn view, its eye, depth target and sample count.
fn select_view<'a>(views: &'a Query<ViewItem>) -> Option<SelectedView<'a>> {
    views
        .iter()
        .filter_map(
            |(entity, view, frustum, main, layers, depth, msaa, resolution)| {
                let eye = perspective_eye(view)?;
                gpu_cull_view_eligible(Some(frustum), layers).then_some(())?;
                let depth = depth.filter(|depth| {
                    resolution.is_none()
                        && depth
                            .texture()
                            .usage()
                            .contains(wgpu::TextureUsages::TEXTURE_BINDING)
                });
                Some((
                    main.id().to_bits(),
                    (entity, view, frustum, eye, depth, *msaa),
                ))
            },
        )
        .min_by_key(|(order, _)| *order)
        .map(|(_, view)| view)
}

/// Uploads the colour tables when they changed and binds the draw group.
fn prepare_tables(
    state: &mut LodState,
    colours: Option<&DistantTerrainColourTables>,
    device: &RenderDevice,
    queue: &RenderQueue,
    lightmap: Option<&crate::lighting::LightmapGpu>,
) {
    let (Some(colours), Some(lightmap)) = (colours, lightmap) else {
        return;
    };
    let mut rebind = false;
    if state.tables.is_none() {
        let packed = PackedTables::new(colours);
        let buffer = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("distant terrain colour tables"),
            size: (packed.words.len() as u64 * 4).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        rebind = true;
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&packed.words));
        state.tables = Some(Tables { buffer, packed });
    }
    let table = lightmap.table();
    if state
        .lightmap
        .as_ref()
        .is_none_or(|bound| *bound != **table)
    {
        state.lightmap = Some((**table).clone());
        rebind = true;
    }
    if rebind || state.draw_group.is_none() {
        let tables = &state.tables.as_ref().expect("uploaded above").buffer;
        state.draw_group = Some(state.gpu.draw_group(
            device.wgpu_device(),
            tables,
            state.lightmap.as_ref().expect("bound above"),
        ));
    }
}

/// Keeps the pyramid seeded from the view's depth, reusing it while the size holds.
fn prepare_pyramid(
    state: &mut LodState,
    device: &RenderDevice,
    depth: Option<&ViewDepthStencilTexture>,
    msaa: Msaa,
) {
    let Some(depth) = depth else {
        state.pyramid = None;
        return;
    };
    let size = depth.texture().size();
    let depth_size = [size.width, size.height];
    let view = crate::scene_sampling::view_depth(depth);
    if state.pyramid.as_ref().is_some_and(|prepared| {
        prepared.depth == Some(view.id()) && prepared.pyramid.depth_size == depth_size
    }) {
        return;
    }
    let pyramid = state
        .pyramid
        .take()
        .map(|prepared| prepared.pyramid)
        .filter(|pyramid| pyramid.depth_size == depth_size)
        .unwrap_or_else(|| HizPyramid::new(device.wgpu_device(), depth_size));
    let bindings =
        state
            .gpu
            .pyramid
            .bindings(device.wgpu_device(), view, msaa.samples() > 1, &pyramid);
    state.pyramid = Some(LodPyramid {
        pyramid,
        bindings,
        depth: Some(view.id()),
    });
}

/// Publishes the counters to the frame.
fn publish(renderer: &mut LodRenderer, drawing: bool) {
    let stats = &mut renderer.stats;
    stats.supported = Some(renderer.supported);
    stats.tier = Some(renderer.tier);
    stats.ready = drawing;
    match &renderer.state {
        Some(state) => {
            stats.pool_bytes = state.gpu.capacity.pool_bytes();
            stats.pool_used_bytes = state.table.pool_used_bytes();
            stats.sections = state.table.sections();
            stats.clusters = state.table.clusters_used();
            stats.dropped_sections = state.table.dropped();
            stats.hiz = state
                .pyramid
                .as_ref()
                .is_some_and(|prepared| prepared.pyramid.mip_count() != 0);
        }
        None => {
            *stats = DistantTerrainRenderStats {
                supported: stats.supported,
                tier: stats.tier,
                resets_requested: stats.resets_requested,
                ..Default::default()
            };
        }
    }
    stats.upload_budget = renderer.budget.bytes();
    renderer.link.publish(*stats);
}

/// Maps this frame's readback once the pass system encoded its copies.
pub(crate) fn submit_distant_terrain_readback(mut renderer: ResMut<LodRenderer>) {
    let renderer = &mut *renderer;
    let (Some(frame), Some(state)) = (renderer.frame.as_ref(), renderer.state.as_mut()) else {
        return;
    };
    let encoded = frame.encoded.load(Ordering::Acquire);
    match frame.readback {
        Some(slot) if encoded => state.readbacks.submit(slot, &state.gpu),
        Some(slot) => state.readbacks.release(slot),
        None => {}
    }
    if !encoded && (frame.clear || frame.scatter_groups != 0) {
        // Staged writes were never applied: start over from nothing drawn, and let the
        // frame start selection again.
        reset(state);
        renderer.stats.resets_requested += 1;
    }
}
