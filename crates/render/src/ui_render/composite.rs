//! The gamma-space UI layer: UI quads blend in an 8-bit sRGB-encoded offscreen
//! target, then one pass composites that layer over the scene in sRGB values,
//! as vanilla's UI blends, instead of in linear light.
//!
//! The layer is retained per view and redrawn only when what it holds changes.
//! The last composite of a frame runs in place of the output blit, writing the
//! camera's output directly.
use super::*;
use bevy::{
    camera::{CameraOutputMode, ClearColor, ClearColorConfig},
    core_pipeline::{core_3d::graph::Node3d, upscaling::UpscalingNode},
    ecs::query::QueryItem,
    math::UVec2,
    render::{
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, Extent3d, LoadOp, Operations,
            RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture, TextureDescriptor,
            TextureDimension, TextureUsages, TextureView, TextureViewDescriptor,
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

/// What a retained layer was drawn from; equal content means its pixels are still valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiLayerContent {
    pub(crate) revision: u64,
    pub(crate) skip: Option<Range<u32>>,
    pub(crate) viewport: Option<(UVec2, UVec2)>,
    pub(crate) model_depth: bool,
}

/// A view's retained UI layer for this frame.
#[derive(Component)]
pub(crate) struct UiLayerTexture {
    pub(crate) texture: Texture,
    pub(crate) view: TextureView,
    /// The drawn content and whether it encoded any batch.
    held: Arc<Mutex<Option<HeldLayer>>>,
    /// Set when the frame's final layer is left for [`UiPresentNode`] to composite.
    present: AtomicBool,
    /// The sole writer of its output with no blend, so the composite can replace the blit.
    direct_output: bool,
}

impl UiLayerTexture {
    /// Whether the layer already holds `content`, and if so whether that drew anything.
    pub(crate) fn holds(&self, content: &UiLayerContent) -> Option<bool> {
        let held = self.held.lock().expect("UI layer content lock");
        held.as_ref()
            .filter(|held| &held.content == content)
            .map(|held| held.encoded)
    }

    /// Records test content without a publication available for partial replay.
    #[cfg(test)]
    pub(crate) fn hold(&self, content: Option<(UiLayerContent, bool)>) {
        self.hold_publication(content, None);
    }

    /// Retains exactly the accepted publication that produced the completed layer.
    pub(super) fn hold_publication(
        &self,
        content: Option<(UiLayerContent, bool)>,
        publication: Option<Arc<UiRenderInput>>,
    ) {
        *self.held.lock().expect("UI layer content lock") =
            content.map(|(content, encoded)| HeldLayer {
                content,
                encoded,
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

    /// Leaves the final layer for the output pass to composite.
    pub(crate) fn defer_present(&self) {
        self.present.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn detached(texture: Texture, view: TextureView) -> Self {
        Self {
            texture,
            view,
            held: Arc::default(),
            present: AtomicBool::new(false),
            direct_output: false,
        }
    }
}

/// A completed raster and the immutable publication used to produce its pixels.
struct HeldLayer {
    content: UiLayerContent,
    encoded: bool,
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
            || self.content.model_depth != content.model_depth
            || self.content.revision != previous.revision
            || content.revision != input.revision
            || extent != input.viewport_size
        {
            return super::damage::UiDamage::Full;
        }
        super::damage::plan(previous, input)
    }
}

struct RetainedLayer {
    texture: Texture,
    view: TextureView,
    held: Arc<Mutex<Option<HeldLayer>>>,
}

/// Per-view layers kept across frames, unlike the frame-scoped texture cache.
#[derive(Default, Resource)]
pub(crate) struct UiLayerStore {
    device: Option<wgpu::Device>,
    views: HashMap<Entity, RetainedLayer>,
}

/// Present when [`UiPresentNode`] replaced the output blit, so the final composite may wait for it.
#[derive(Resource)]
pub(crate) struct UiPresentInstalled;

pub(crate) fn prepare_ui_layers(
    mut commands: Commands,
    mut store: ResMut<UiLayerStore>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ViewTarget, Option<&ExtractedCamera>)>,
) {
    if store.device.as_ref() != Some(device.wgpu_device()) {
        store.views.clear();
        store.device = Some(device.wgpu_device().clone());
    }
    store.views.retain(|view, _| views.contains(*view));
    let mut writers = HashMap::<_, usize>::new();
    for (_, target, _) in &views {
        *writers.entry(target.out_texture().id()).or_default() += 1;
    }
    for (entity, target, camera) in &views {
        let size = target.main_texture().size();
        let size = Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let layer = store
            .views
            .entry(entity)
            .and_modify(|layer| {
                if layer.texture.size() != size {
                    *layer = retained_layer(&device, size);
                }
            })
            .or_insert_with(|| retained_layer(&device, size));
        let unblended = camera.is_none_or(|camera| {
            matches!(
                camera.output_mode,
                CameraOutputMode::Write {
                    blend_state: None,
                    ..
                }
            )
        });
        commands.entity(entity).insert(UiLayerTexture {
            texture: layer.texture.clone(),
            view: layer.view.clone(),
            held: Arc::clone(&layer.held),
            present: AtomicBool::new(false),
            direct_output: unblended && writers[&target.out_texture().id()] == 1,
        });
    }
}

fn retained_layer(device: &RenderDevice, size: Extent3d) -> RetainedLayer {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("retained gamma-space UI layer"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: UI_LAYER_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    RetainedLayer {
        texture,
        view,
        held: Arc::default(),
    }
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
                    format: TextureFormat::bevy_default(),
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
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
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
    target: &ViewTarget,
    layer: &TextureView,
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
        [layer, write.source],
        destination,
        None,
        pipeline,
        layout,
    );
}

fn encode_composite(
    context: &mut RenderContext,
    world: &World,
    sources: [&TextureView; 2],
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
    let bind_group: BindGroup = context.render_device().create_bind_group(
        "UI composite bind group",
        layout,
        &BindGroupEntries::sequential((sources[0], sources[1])),
    );
    if let Some(profile) = world.get_resource::<super::profile::UiProfile>() {
        profile.record_pass(crate::RuntimeStage::GpuUiComposite);
        profile.record_draw(crate::RuntimeStage::GpuUiComposite, 0);
    }
    let attachments = [Some(destination)];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gamma-space UI composite"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
            world,
            crate::RuntimeStage::GpuUiComposite,
        ),
        occlusion_query_set: None,
    });
    if let Some(rect) = scissor {
        pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Replaces the output blit: composites a deferred final layer straight into the
/// camera output, or falls back to compositing into the main texture and blitting.
#[derive(Default)]
pub(crate) struct UiPresentNode(UpscalingNode);

impl ViewNode for UiPresentNode {
    type ViewQuery = (
        <UpscalingNode as ViewNode>::ViewQuery,
        Option<&'static UiLayerTexture>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (blit, layer): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let (target, _, camera) = blit;
        let pending = layer.filter(|layer| layer.present.load(Ordering::Relaxed));
        let pipelines = world
            .get_resource::<UiGpu>()
            .and_then(|gpu| gpu.composite_pipelines.get(&graph.view_entity()).copied());
        let (Some(layer), Some(pipelines), Some(cache), Some(composite_pipeline)) = (
            pending,
            pipelines,
            world.get_resource::<PipelineCache>(),
            world.get_resource::<UiCompositePipeline>(),
        ) else {
            return self.0.run(graph, context, blit, world);
        };
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
            let scissor = camera
                .and_then(|camera| camera.viewport.as_ref())
                .map(|viewport| (viewport.physical_position, viewport.physical_size));
            encode_composite(
                context,
                world,
                [&layer.view, target.main_texture_view()],
                target.out_texture_color_attachment(clear),
                scissor,
                pipeline,
                &layout,
            );
            return Ok(());
        }
        if let Some(pipeline) = cache.get_render_pipeline(pipelines.main) {
            composite(context, world, target, &layer.view, pipeline, &layout);
        }
        self.0.run(graph, context, blit, world)
    }
}

/// Swaps the output blit for [`UiPresentNode`], keeping every installed edge.
pub(crate) fn install_present_node(world: &mut World) {
    if world.contains_resource::<UiPresentInstalled>() {
        return;
    }
    let runner = ViewNodeRunner::new(UiPresentNode::default(), world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(bevy::core_pipeline::core_3d::graph::Core3d) else {
        return;
    };
    let Ok(node) = graph.get_node_state_mut(Node3d::Upscaling) else {
        return;
    };
    node.node = Box::new(runner);
    node.type_name = std::any::type_name::<ViewNodeRunner<UiPresentNode>>();
    world.insert_resource(UiPresentInstalled);
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod tests;
