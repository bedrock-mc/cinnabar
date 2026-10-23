//! The gamma-space UI layer: UI quads blend in an 8-bit sRGB-encoded offscreen
//! target, then one pass composites that layer over the scene in sRGB values,
//! as vanilla's UI blends, instead of in linear light.
//!
//! The layer is retained per view and redrawn only when what it holds changes.
//! The last composite of a frame runs in place of the output blit, writing the
//! camera's output directly.
use super::target::{NativeUiTarget, UiTarget};
use super::*;
use bevy::{
    camera::{CameraOutputMode, ClearColor, ClearColorConfig},
    core_pipeline::{Core3d, Core3dSystems, upscaling::upscaling},
    math::UVec2,
    render::{
        camera::ExtractedCamera,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutId, Extent3d, LoadOp,
            Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture,
            TextureDescriptor, TextureDimension, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewId,
        },
        renderer::RenderContext,
    },
};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) const UI_COMPOSITE_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("f5b1f3c2-7a0e-4d0c-9c5e-3a8a4b1e6d21");
/// The UI layer's format: raw bytes, so blending happens on sRGB-encoded values.
pub(crate) const UI_LAYER_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
pub(super) const UI_LAYER_BYTES_PER_TEXEL: u64 = {
    assert!(matches!(UI_LAYER_FORMAT, TextureFormat::Rgba8Unorm));
    4
};

/// What a retained layer was drawn from; equal content means its pixels are still valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiLayerContent {
    pub(crate) revision: u64,
    pub(crate) texture_generation: u64,
    pub(crate) skip: Option<Range<u32>>,
    pub(crate) viewport: Option<(UVec2, UVec2)>,
    pub(crate) model_depth: bool,
    /// Original publication batches belonging to this alpha segment.
    pub(crate) segment: Range<usize>,
    /// The live inversion immediately after this segment, when present.
    pub(crate) inverted: Option<usize>,
}

/// A view's retained UI layer for this frame.
#[derive(Component)]
pub(crate) struct UiLayerTexture {
    pub(crate) texture: Texture,
    pub(crate) view: TextureView,
    /// The drawn content and whether it encoded any batch.
    held: Arc<Mutex<Option<HeldLayer>>>,
    /// Bindings share the retained raster's lifetime and cover both scene ping-pong views.
    bindings: Arc<Mutex<CompositeBindings>>,
    /// Set when the frame's final layer is left for [`ui_present`] to composite.
    present: AtomicBool,
    /// Complete nonempty encoding is independent of retained-raster eligibility.
    content_drawn: AtomicBool,
    /// Whether a deferred layer reached the scene before ordinary output.
    composited: AtomicBool,
    /// Whether this frame composited a complete nonempty interface.
    content_presented: AtomicBool,
    /// Whether this frame already wrote the final camera output.
    presented: AtomicBool,
    /// The sole writer of its output with no blend, so the composite can replace the blit.
    direct_output: bool,
    /// Earlier alpha segments; this component's own texture is always the final segment.
    segments: Vec<UiLayerTexture>,
}

impl UiLayerTexture {
    /// Returns an independent raster only when the complete bounded segment set is resident.
    pub(super) fn segment(&self, index: usize, count: usize) -> Option<&Self> {
        if count != self.segments.len() + 1 || index >= count {
            return None;
        }
        if index + 1 == count {
            Some(self)
        } else {
            self.segments.get(index)
        }
    }

    /// Whether the layer already holds `content`, and if so whether that drew anything.
    pub(crate) fn holds(&self, content: &UiLayerContent) -> Option<bool> {
        let held = self.held.lock().expect("UI layer content lock");
        held.as_ref()
            .filter(|held| &held.content == content)
            .map(|held| held.encoded)
    }

    /// Preserves proven content when damage analysis permits reuse across publications.
    pub(super) fn retained_nonempty(&self) -> bool {
        self.held
            .lock()
            .expect("UI layer content lock")
            .as_ref()
            .is_some_and(|held| held.nonempty)
    }

    /// Records test content without a publication available for partial replay.
    #[cfg(test)]
    pub(crate) fn hold(&self, content: Option<(UiLayerContent, bool)>) {
        self.hold_publication(
            content.map(|(content, encoded)| (content, encoded, encoded)),
            None,
        );
    }

    /// Retains exactly the accepted publication that produced the completed layer.
    pub(super) fn hold_publication(
        &self,
        content: Option<(UiLayerContent, bool, bool)>,
        publication: Option<Arc<UiRenderInput>>,
    ) {
        *self.held.lock().expect("UI layer content lock") =
            content.map(|(content, encoded, nonempty)| HeldLayer {
                content,
                encoded,
                nonempty,
                publication,
            });
    }

    /// Plans replay only when the retained layer and publication share the same draw policy.
    pub(super) fn damage(
        &self,
        content: &UiLayerContent,
        input: &UiRenderInput,
    ) -> super::damage::UiDamage {
        let held = self.held.lock().expect("UI layer content lock");
        held.as_ref().map_or(super::damage::UiDamage::Full, |held| {
            held.damage(
                content,
                input,
                [self.texture.width(), self.texture.height()],
            )
        })
    }

    /// Whether this frame composited complete content from every retained segment.
    pub(crate) fn presented_complete_content(&self) -> bool {
        self.content_presented.load(Ordering::Relaxed)
    }

    /// Records actual encoding, including scratch rasters and trailing inversion passes.
    pub(crate) fn record_content_draw(&self, complete: bool, encoded: bool) {
        self.content_drawn
            .store(complete && encoded, Ordering::Relaxed);
    }

    /// Incomplete or empty encoding never qualifies a destination screen for handoff.
    fn complete_content(&self) -> bool {
        self.content_drawn.load(Ordering::Relaxed)
    }

    /// Leaves the final layer for the output pass to composite.
    pub(crate) fn defer_present(&self) {
        self.present.store(true, Ordering::Relaxed);
    }

    /// A raster outside the per-view retained set, such as an element filter's offscreen group.
    pub(crate) fn detached(texture: Texture, view: TextureView) -> Self {
        Self {
            texture,
            view,
            held: Arc::default(),
            bindings: Arc::default(),
            present: AtomicBool::new(false),
            content_drawn: AtomicBool::new(false),
            composited: AtomicBool::new(false),
            content_presented: AtomicBool::new(false),
            presented: AtomicBool::new(false),
            direct_output: false,
            segments: Vec::new(),
        }
    }
}

/// A completed raster and the immutable publication used to produce its pixels.
struct HeldLayer {
    content: UiLayerContent,
    encoded: bool,
    nonempty: bool,
    publication: Option<Arc<UiRenderInput>>,
}

impl HeldLayer {
    /// Partial replay requires an unchanged full-target policy and matching accepted revisions.
    fn damage(
        &self,
        content: &UiLayerContent,
        input: &UiRenderInput,
        extent: [u32; 2],
    ) -> super::damage::UiDamage {
        let Some(previous) = self.publication.as_ref() else {
            return super::damage::UiDamage::Full;
        };
        if !self.encoded
            || self.content.skip.is_some()
            || content.skip.is_some()
            || self.content.viewport.is_some()
            || content.viewport.is_some()
            || self.content.segment != content.segment
            || self.content.inverted != content.inverted
            || self.content.model_depth != content.model_depth
            || self.content.texture_generation != content.texture_generation
            || self.content.revision != previous.revision
            || content.revision != input.revision
            || extent != input.viewport_size
        {
            return super::damage::UiDamage::Full;
        }
        super::damage::plan_segment(previous, input, content.segment.clone())
    }
}

struct RetainedLayer {
    texture: Texture,
    view: TextureView,
    held: Arc<Mutex<Option<HeldLayer>>>,
    bindings: Arc<Mutex<CompositeBindings>>,
}

/// Keeps the two scene orientations without retaining replaced views indefinitely.
#[derive(Default)]
struct CompositeBindings {
    entries: Vec<([TextureViewId; 2], BindGroupLayoutId, BindGroup)>,
}

impl CompositeBindings {
    /// Reuses a binding until one source or its layout changes; at most two pairs stay resident.
    fn get(
        &mut self,
        device: &RenderDevice,
        layout: &BindGroupLayout,
        sources: [&TextureView; 2],
        live_scenes: [TextureViewId; 2],
    ) -> BindGroup {
        self.entries
            .retain(|(views, _, _)| live_scenes.contains(&views[1]));
        let key = sources.map(TextureView::id);
        if let Some((_, _, group)) = self
            .entries
            .iter()
            .find(|(views, id, _)| *views == key && *id == layout.id())
        {
            return group.clone();
        }
        let group = device.create_bind_group(
            "UI composite bind group",
            layout,
            &BindGroupEntries::sequential((sources[0], sources[1])),
        );
        if self.entries.len() == 2 {
            self.entries.remove(0);
        }
        self.entries.push((key, layout.id(), group.clone()));
        group
    }
}

/// Per-view layers kept across frames, unlike the frame-scoped texture cache.
#[derive(Default, Resource)]
pub(crate) struct UiLayerStore {
    device: Option<wgpu::Device>,
    views: HashMap<Entity, Vec<RetainedLayer>>,
    /// Extent and segment count whose rasters ran out of memory.
    failed: crate::gpu_allocation::FailedRequests<(Extent3d, usize)>,
}

impl UiLayerStore {
    /// A replacement render device cannot use color or retained publication state from the old device.
    fn synchronize_device(&mut self, device: &RenderDevice) {
        if self.device.as_ref() != Some(device.wgpu_device()) {
            self.views.clear();
            self.failed = Default::default();
            self.device = Some(device.wgpu_device().clone());
        }
    }

    /// Releases rasters and their accepted publications when the owning view disappears.
    fn retain_live_views(&mut self, live: impl Fn(Entity) -> bool) {
        self.views.retain(|view, _| live(*view));
        self.failed.retain(live);
    }
}

/// Present when [`ui_present`] replaced the output blit, so the final composite may wait for it.
#[derive(Resource)]
pub(crate) struct UiPresentInstalled;

/// Prepares a bounded independent raster set for each current view and accepted overlay topology.
pub(crate) fn prepare_ui_layers(
    mut commands: Commands,
    mut store: ResMut<UiLayerStore>,
    device: Res<RenderDevice>,
    views: Query<(
        Entity,
        &ViewTarget,
        Option<&ExtractedCamera>,
        Option<&NativeUiTarget>,
    )>,
    gpu: Option<Res<UiGpu>>,
) {
    let store = &mut *store;
    store.synchronize_device(&device);
    store.retain_live_views(|view| views.contains(view));
    let requested = gpu
        .as_ref()
        .filter(|gpu| gpu.accepted_revision.is_some())
        .map_or(0, |gpu| {
            let batches: Vec<_> = gpu
                .batches
                .iter()
                .enumerate()
                .filter(|(_, batch)| {
                    batch.world_projection == 0 && !super::backdrop::is_backdrop(batch)
                })
                .map(|(index, batch)| (index, batch, ()))
                .collect();
            super::segments::plan_ui_passes(&batches, false).layer_count()
        });
    let mut writers = HashMap::<_, usize>::new();
    for (_, target, _, _) in &views {
        if let Some(output) = target.out_texture() {
            *writers.entry(output.id()).or_default() += 1;
        }
    }
    for (entity, target, camera, native) in &views {
        let target = UiTarget::new(target, native);
        let size = target.main_texture().size();
        let size = Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let count = retained_segment_count(requested.max(1), size);
        let layers = store.views.entry(entity).or_default();
        synchronize_view_layers(layers, &mut store.failed, entity, &device, size, count);
        if layers.is_empty() {
            // Out of memory: the UI skips until its raster can be retried.
            commands.entity(entity).remove::<UiLayerTexture>();
            continue;
        }
        if requested == 0 {
            for layer in layers.iter() {
                *layer.held.lock().expect("UI layer content lock") = None;
            }
        }
        let unblended = camera.is_none_or(|camera| {
            matches!(
                camera.output_mode,
                CameraOutputMode::Write {
                    blend_state: None,
                    ..
                }
            )
        });
        let direct_output = unblended
            && target.out_texture().is_some_and(|output| {
                writers[&output.id()] == 1
                    && crate::render_bounds::extent(output) == [size.width, size.height]
            });
        let mut frame = frame_layer(
            layers.last().expect("one scratch layer is always resident"),
            direct_output,
        );
        frame.segments = layers[..layers.len() - 1]
            .iter()
            .map(|layer| frame_layer(layer, false))
            .collect();
        commands.entity(entity).insert(frame);
    }
}

/// Bounds extra full-extent rasters; larger topologies use the existing single-layer replay.
fn retained_segment_count(requested: usize, size: Extent3d) -> usize {
    const MAX_LAYERS: usize = 8;
    const MAX_EXTRA_BYTES: u64 = 64 * 1024 * 1024;
    let bytes = u64::from(size.width)
        .saturating_mul(u64::from(size.height))
        .saturating_mul(UI_LAYER_BYTES_PER_TEXEL);
    if requested > 0
        && requested <= MAX_LAYERS
        && bytes.saturating_mul(requested.saturating_sub(1) as u64) <= MAX_EXTRA_BYTES
    {
        requested
    } else {
        1
    }
}

/// Allocates the view's rasters with shared backoff, keeping single-layer replay when possible.
fn synchronize_view_layers(
    layers: &mut Vec<RetainedLayer>,
    failed: &mut crate::gpu_allocation::FailedRequests<(Extent3d, usize)>,
    view: Entity,
    device: &RenderDevice,
    size: Extent3d,
    count: usize,
) {
    failed.attempt(view, (size, count), || {
        synchronize_layers(layers, device, size, count);
        (layers.len() == count).then_some(())
    });
}

/// Drops removed rasters and every old extent before allocating the required bounded set.
/// Out of memory falls back to the single-layer replay, or to no layer at all.
fn synchronize_layers(
    layers: &mut Vec<RetainedLayer>,
    device: &RenderDevice,
    size: Extent3d,
    count: usize,
) {
    if layers
        .first()
        .is_some_and(|layer| layer.texture.size() != size)
    {
        layers.clear();
    }
    layers.truncate(count);
    while layers.len() < count {
        let Some(layer) = retained_layer(device, size) else {
            layers.truncate(1);
            return;
        };
        layers.push(layer);
    }
}

/// Shares retained storage into a frame component without carrying presentation state across frames.
fn frame_layer(layer: &RetainedLayer, direct_output: bool) -> UiLayerTexture {
    UiLayerTexture {
        texture: layer.texture.clone(),
        view: layer.view.clone(),
        held: Arc::clone(&layer.held),
        bindings: Arc::clone(&layer.bindings),
        present: AtomicBool::new(false),
        content_drawn: AtomicBool::new(false),
        composited: AtomicBool::new(false),
        content_presented: AtomicBool::new(false),
        presented: AtomicBool::new(false),
        direct_output,
        segments: Vec::new(),
    }
}

/// Allocates one gamma-space raster on the current view/device lifetime; `None` when out of
/// memory.
fn retained_layer(device: &RenderDevice, size: Extent3d) -> Option<RetainedLayer> {
    let bytes = u64::from(size.width) * u64::from(size.height) * UI_LAYER_BYTES_PER_TEXEL;
    let texture =
        crate::gpu_allocation::checked(device, "retained gamma-space UI layer", bytes, || {
            device.create_texture(&TextureDescriptor {
                label: Some("retained gamma-space UI layer"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: UI_LAYER_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        })?;
    let view = texture.create_view(&TextureViewDescriptor::default());
    Some(RetainedLayer {
        texture,
        view,
        held: Arc::default(),
        bindings: Arc::default(),
    })
}

#[derive(Resource)]
pub(crate) struct UiCompositePipeline {
    pub(crate) layout: BindGroupLayoutDescriptor,
    variants: Variants<RenderPipeline, UiCompositeSpecializer>,
    clear: Option<CachedRenderPipelineId>,
}

struct UiCompositeSpecializer;

/// The composite's colour target: the view's main texture or its output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(crate) struct UiCompositeKey {
    pub(crate) format: TextureFormat,
}

impl Specializer<RenderPipeline> for UiCompositeSpecializer {
    type Key = UiCompositeKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = key.format;
        Ok(key)
    }
}

impl FromWorld for UiCompositePipeline {
    fn from_world(_world: &mut World) -> Self {
        let texture = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout =
            BindGroupLayoutDescriptor::new("UI composite layout", &[texture(0), texture(1)]);
        let descriptor = RenderPipelineDescriptor {
            label: Some("gamma-space UI composite".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: UI_COMPOSITE_SHADER_HANDLE,
                entry_point: Some("composite_vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: UI_COMPOSITE_SHADER_HANDLE,
                entry_point: Some("composite_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        };
        Self {
            layout,
            variants: Variants::new(UiCompositeSpecializer, descriptor),
            clear: None,
        }
    }
}

impl UiCompositePipeline {
    pub(crate) fn specialize(
        &mut self,
        cache: &PipelineCache,
        key: UiCompositeKey,
    ) -> Option<CachedRenderPipelineId> {
        self.clear_pipeline_id(cache);
        self.variants.specialize(cache, key).ok()
    }

    /// Composites into an `hdr` view's main texture and into its `output` format, if known.
    pub(crate) fn view_pipelines(
        &mut self,
        cache: &PipelineCache,
        hdr: bool,
        output: Option<TextureFormat>,
    ) -> Option<CompositePipelines> {
        let format = if hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        let main = self.specialize(cache, UiCompositeKey { format })?;
        let output = output.and_then(|format| self.specialize(cache, UiCompositeKey { format }));
        Some(CompositePipelines { main, output })
    }

    /// Shares one rectangle-clear pipeline between startup warmup and all view formats.
    fn clear_pipeline_id(&mut self, cache: &PipelineCache) -> CachedRenderPipelineId {
        *self
            .clear
            .get_or_insert_with(|| cache.queue_render_pipeline(clear_pipeline_descriptor()))
    }

    /// Returns the unblended rectangle-clear pipeline once asynchronous compilation finishes.
    pub(super) fn clear_pipeline<'a>(
        &self,
        cache: &'a PipelineCache,
    ) -> Option<&'a RenderPipeline> {
        self.clear.and_then(|id| cache.get_render_pipeline(id))
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for UiCompositePipeline {
    /// Covers the damage clear and the view's main and output composites.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.clear_pipeline_id(cache));
        if let Some(pipelines) = self.view_pipelines(cache, view.hdr, view.output) {
            ids.push(pipelines.main);
            ids.extend(pipelines.output);
        }
        Ok(())
    }
}

/// Clears a scissored rectangle without reading or blending the old gamma-space layer.
pub(super) fn clear_pipeline_descriptor() -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("retained UI damage clear".into()),
        vertex: VertexState {
            shader: UI_COMPOSITE_SHADER_HANDLE,
            entry_point: Some("composite_vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader: UI_COMPOSITE_SHADER_HANDLE,
            entry_point: Some("clear_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: UI_LAYER_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    }
}

/// A view's composite pipelines into its main texture and into its output.
#[derive(Clone, Copy)]
pub(crate) struct CompositePipelines {
    pub(crate) main: CachedRenderPipelineId,
    pub(crate) output: Option<CachedRenderPipelineId>,
}

/// Composite `layer` over the view's scene into its next main texture.
pub(crate) fn composite(
    context: &mut RenderContext,
    world: &World,
    target: UiTarget<'_>,
    layer: &UiLayerTexture,
    pipeline: &RenderPipeline,
    layout: &BindGroupLayout,
) {
    let write = target.post_process_write();
    let destination = RenderPassColorAttachment {
        view: write.destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(Default::default()),
            store: StoreOp::Store,
        },
    };
    encode_composite(
        context,
        world,
        CompositeSources {
            layer,
            scene: write.source,
            target,
        },
        destination,
        None,
        pipeline,
        layout,
    );
}

/// UI and scene views, together with the view that owns the live scene pair.
struct CompositeSources<'a> {
    layer: &'a UiLayerTexture,
    scene: &'a TextureView,
    target: UiTarget<'a>,
}

/// Blends a raster over its current scene and releases bindings for retired scene targets.
fn encode_composite(
    context: &mut RenderContext,
    world: &World,
    sources: CompositeSources<'_>,
    destination: RenderPassColorAttachment,
    scissor: Option<(UVec2, UVec2)>,
    pipeline: &RenderPipeline,
    layout: &BindGroupLayout,
) {
    let scissor = match scissor {
        Some((position, size)) => match crate::render_bounds::scissor(
            render_model::UiScissor::new(position.x, position.y, size.x, size.y),
            crate::render_bounds::extent(destination.view),
        ) {
            Some(rect) => Some(rect),
            None => return,
        },
        None => None,
    };
    let CompositeSources {
        layer,
        scene,
        target,
    } = sources;
    let bind_group = layer
        .bindings
        .lock()
        .expect("UI composite bindings lock")
        .get(
            context.render_device(),
            layout,
            [&layer.view, scene],
            [
                target.main_texture_view().id(),
                target.main_texture_other_view().id(),
            ],
        );
    if let Some(profile) = world.get_resource::<super::profile::UiProfile>() {
        profile.record_pass(render_scene::RuntimeStage::GpuUiComposite);
        profile.record_draw(render_scene::RuntimeStage::GpuUiComposite, 0);
    }
    let attachments = [Some(destination)];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gamma-space UI composite"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuUiComposite,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(rect) = scissor {
        pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Presents native screen UI after the world upscale, with or without a final alpha layer.
pub(crate) fn ui_present(
    world: &World,
    view: bevy::render::renderer::ViewQuery<(
        &ViewTarget,
        &bevy::core_pipeline::upscaling::ViewUpscalingPipeline,
        Option<&ExtractedCamera>,
        Option<&UiLayerTexture>,
        Option<&NativeUiTarget>,
    )>,
    mut context: RenderContext,
) {
    let entity = view.entity();
    let (output, _, camera, layer, native) = view.into_inner();
    if let Some(layer) = layer {
        layer.presented.store(false, Ordering::Relaxed);
        layer.content_presented.store(false, Ordering::Relaxed);
        layer.composited.store(false, Ordering::Relaxed);
    }
    // A native layer cannot be composited against an unseeded or reduced scene.
    if native.is_some_and(|native| !native.is_ready()) {
        return;
    }
    let target = UiTarget::new(output, native);
    let resources = world
        .get_resource::<UiGpu>()
        .and_then(|gpu| gpu.composite_pipelines.get(&entity).copied())
        .zip(world.get_resource::<PipelineCache>())
        .zip(world.get_resource::<UiCompositePipeline>());
    if let Some(layer) = layer.filter(|layer| layer.present.load(Ordering::Relaxed))
        && let Some(((pipelines, cache), composite_pipeline)) = resources
    {
        let layout = cache.get_bind_group_layout(&composite_pipeline.layout);
        if layer.direct_output
            && let Some(pipeline) = pipelines
                .output
                .and_then(|id| cache.get_render_pipeline(id))
        {
            let clear = match camera.map(|camera| &camera.output_mode) {
                Some(CameraOutputMode::Write { clear_color, .. }) => *clear_color,
                _ => ClearColorConfig::Default,
            };
            let clear = match clear {
                ClearColorConfig::Default => Some(world.resource::<ClearColor>().0.into()),
                ClearColorConfig::Custom(color) => Some(color.into()),
                ClearColorConfig::None => None,
            };
            if let Some(attachment) = target.out_texture_color_attachment(clear) {
                let scissor = camera
                    .and_then(|camera| camera.viewport.as_ref())
                    .map(|viewport| (viewport.physical_position, viewport.physical_size));
                encode_composite(
                    &mut context,
                    world,
                    CompositeSources {
                        layer,
                        scene: target.main_texture_view(),
                        target,
                    },
                    attachment,
                    scissor,
                    pipeline,
                    &layout,
                );
                layer.presented.store(true, Ordering::Relaxed);
                if let Some(native) = native {
                    native.mark_presented();
                }
                return;
            }
        }
        if let Some(pipeline) = cache.get_render_pipeline(pipelines.main) {
            composite(&mut context, world, target, layer, pipeline, &layout);
            layer.composited.store(true, Ordering::Relaxed);
        }
    }
    if let Some(native) = native {
        native.present(world, &mut context, output);
    }
}

type UiOutputQuery = (
    &'static ViewTarget,
    &'static bevy::core_pipeline::upscaling::ViewUpscalingPipeline,
    Option<&'static ExtractedCamera>,
    Option<&'static UiLayerTexture>,
    Option<&'static bevy::render::view::ResolvedCompositingSpace>,
    Option<&'static NativeUiTarget>,
);

/// Qualifies content only after direct output or a successfully encoded ordinary output blit.
fn observe_ui_output(
    view: bevy::render::renderer::ViewQuery<UiOutputQuery>,
    cache: Res<PipelineCache>,
    blit: Res<bevy::core_pipeline::blit::BlitPipeline>,
    mut pipelines: ResMut<
        bevy::render::render_resource::SpecializedRenderPipelines<
            bevy::core_pipeline::blit::BlitPipeline,
        >,
    >,
) {
    let (target, _, camera, layer, space, native) = view.into_inner();
    let Some(layer) = layer else {
        return;
    };
    if !layer.complete_content() {
        return;
    }
    if layer.presented.load(Ordering::Relaxed) {
        layer.content_presented.store(true, Ordering::Relaxed);
        return;
    }
    if layer.present.load(Ordering::Relaxed) && !layer.composited.load(Ordering::Relaxed) {
        return;
    }
    if let Some(native) = native {
        layer
            .content_presented
            .store(native.was_presented(), Ordering::Relaxed);
        return;
    }
    let blend_state = match camera.map(|camera| &camera.output_mode) {
        Some(CameraOutputMode::Skip) => return,
        Some(CameraOutputMode::Write { blend_state, .. }) => blend_state.or_else(|| {
            camera
                .filter(|camera| camera.sorted_camera_index_for_target > 0)
                .map(|_| BlendState::ALPHA_BLENDING)
        }),
        None => None,
    };
    let Some(target_format) = target.out_texture_view_format() else {
        return;
    };
    let pipeline = pipelines.specialize(
        &cache,
        &blit,
        bevy::core_pipeline::blit::BlitPipelineKey {
            target_format,
            blend_state,
            samples: 1,
            source_space: bevy::render::view::ResolvedCompositingSpace::space(space),
        },
    );
    layer.content_presented.store(
        cache.get_render_pipeline(pipeline).is_some(),
        Ordering::Relaxed,
    );
}

/// Runs Bevy's output blit only while neither native composition nor the HUD wrote the output.
fn needs_output_blit(
    view: bevy::render::renderer::ViewQuery<(Option<&UiLayerTexture>, Option<&NativeUiTarget>)>,
) -> bool {
    let (layer, native) = view.into_inner();
    !layer.is_some_and(|layer| layer.presented.load(Ordering::Relaxed))
        && !native.is_some_and(NativeUiTarget::was_presented)
}

/// Presents the HUD after post-processing, then blits only if direct output was unavailable.
pub(crate) fn install_present_node(world: &mut World) {
    if world.contains_resource::<UiPresentInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |world, schedule| {
            crate::scene_target::order_camera_stages(schedule);
            schedule
                .remove_systems_in_set(
                    upscaling,
                    world,
                    bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("replace the output blit");
            schedule.add_systems((
                crate::gpu_timing::profiled(
                    ui_present,
                    (!cfg!(target_os = "macos")).then_some(render_scene::RuntimeStage::GpuBlit),
                    "UiPresent",
                )
                .after(Core3dSystems::PostProcess)
                .after(super::overlay::UiOverlayLabel)
                .after(super::overlay::UiOverlayPostLabel),
                crate::gpu_timing::profiled(
                    upscaling,
                    (!cfg!(target_os = "macos")).then_some(render_scene::RuntimeStage::GpuBlit),
                    "Upscaling",
                )
                .after(ui_present)
                .after(Core3dSystems::PostProcess)
                .run_if(needs_output_blit),
                observe_ui_output.after(ui_present).after(upscaling),
            ));
        })
        .is_ok();
    if installed {
        world.insert_resource(UiPresentInstalled);
    }
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "composite_raster_tests.rs"]
mod raster_tests;
