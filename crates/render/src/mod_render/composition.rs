//! Shared downsampled mask halos and bounded display-space outlines, after existing post effects.

use super::passes::{self, FrameUniform};
use crate::actor_render::masks::{MaskGpu, MaskLabel};
use crate::queue_writes::ProfiledQueue as _;
use bevy::{
    asset::uuid_handle,
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{
        BevyError, Commands, Entity, Handle, IntoScheduleConfigs, Query, Res, ResMut, Resource,
        Shader, SubApp, SystemSet, World, default,
    },
    render::{
        Render, RenderSystems,
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::{ExtractedView, ViewTarget},
    },
};
use mod_api::drawing::{DrawingHandle, MASK_BLUR_LEVELS, MAX_MASK_ATTACHMENT_BYTES, MaskStyle};
use render_scene::ModRenderScene;
use std::collections::HashMap;

const COMPOSITE: &str = include_str!("composition.wgsl");
const BLUR: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { return blur(uv, 1.0); }";
const BLUR_SHADER: Handle<Shader> = uuid_handle!("ec6e8c09-3fa0-4cf9-844f-579d5e220f69");
const COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("53a30e40-6e29-41ad-98c1-aa4bc5888d6e");

struct Level {
    _texture: Texture,
    view: TextureView,
    _uniform: Buffer,
    binding: BindGroup,
}

struct Pyramid {
    levels: Vec<Level>,
    size: Extent3d,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BindingKey {
    source: TextureViewId,
    masks: [TextureViewId; 2],
    sampler: SamplerId,
}

struct StyleGpu {
    uniform: Buffer,
    bindings: Vec<(BindingKey, BindGroup)>,
}

impl StyleGpu {
    /// Keeps a style's uniform and its dependent bindings under the same lifetime.
    fn new(device: &RenderDevice) -> Self {
        Self {
            uniform: device.create_buffer(&BufferDescriptor {
                label: Some("host selection style"),
                size: mod_render::shader::FRAME_UNIFORM_BYTES as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bindings: Vec::new(),
        }
    }

    /// Prepares both scene inputs with the current coverage and selected halo level.
    fn prepare_bindings(
        &mut self,
        device: &RenderDevice,
        cache: &PipelineCache,
        sampler: &Sampler,
        sources: [&TextureView; 2],
        masks: [&TextureView; 2],
    ) {
        let keys = sources.map(|source| BindingKey {
            source: source.id(),
            masks: masks.map(TextureView::id),
            sampler: sampler.id(),
        });
        self.bindings.retain(|(key, _)| keys.contains(key));
        for (source, key) in sources.into_iter().zip(keys) {
            if self.bindings.iter().any(|(existing, _)| *existing == key) {
                continue;
            }
            self.bindings.push((
                key,
                binding(device, cache, sampler, &self.uniform, source, &masks),
            ));
        }
    }

    /// Selects a prepared binding without changing the view's alternating scene texture.
    fn binding(&self, source: TextureViewId) -> Option<&BindGroup> {
        self.bindings
            .iter()
            .find_map(|(key, binding)| (key.source == source).then_some(binding))
    }
}

#[derive(Resource)]
pub(super) struct CompositionGpu {
    sampler: Sampler,
    blur: Option<CachedRenderPipelineId>,
    composite: HashMap<TextureFormat, CachedRenderPipelineId>,
    pyramids: HashMap<TextureViewId, Pyramid>,
    styles: HashMap<(Entity, DrawingHandle), StyleGpu>,
}

impl crate::pipeline_warmup::PrewarmPipelines for CompositionGpu {
    /// Holds loading until the downsampler and both scene-format composites finish compiling.
    fn prewarm(
        &mut self,
        _cache: &PipelineCache,
        _view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.extend(self.blur);
        ids.extend(self.composite.values().copied());
        Ok(())
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ModCompositionLabel;

#[derive(Resource)]
struct Installed;

/// Installs one shared halo producer; custom post effects keep their current schedule.
pub(super) fn install(app: &mut SubApp) {
    app.add_systems(
        Render,
        init.before(RenderSystems::PrepareViews)
            .run_if(bevy::ecs::schedule::common_conditions::not(
                bevy::prelude::resource_exists::<CompositionGpu>,
            ))
            .run_if(render_scene::startup_work::world_work_allowed),
    )
    .add_systems(
        Render,
        prepare
            .in_set(RenderSystems::PrepareBindGroups)
            .after(crate::actor_render::masks::prepare),
    );
    install_graph(app.world_mut());
}

/// Composites after authored post passes and before both UI paths.
pub(super) fn install_graph(world: &mut World) {
    if world.contains_resource::<Installed>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                draw.in_set(ModCompositionLabel)
                    .after(Core3dSystems::PostProcess)
                    .after(MaskLabel)
                    .after(super::ModPassLabel)
                    .before(crate::ui_render::UiOverlayLabel)
                    .before(crate::ui_render::overlay::UiOverlayPostLabel),
            );
        })
        .is_ok()
    {
        world.insert_resource(Installed);
    }
}

/// Queues every built-in variant at startup, before any selection or style is published.
fn init(mut commands: Commands, device: Res<RenderDevice>, mut cache: ResMut<PipelineCache>) {
    let source = mod_render::shader::compose(BLUR, false).expect("host blur shader");
    cache.set_shader(
        BLUR_SHADER.id(),
        crate::shader_safety::from_wgsl(source, "selection-blur.wgsl"),
    );
    let source =
        mod_render::shader::compose_masked(COMPOSITE, false, 2).expect("host selection shader");
    cache.set_shader(
        COMPOSITE_SHADER.id(),
        crate::shader_safety::from_wgsl(source, "selection-composite.wgsl"),
    );
    let blur = Some(queue_pipeline(
        &cache,
        BLUR_SHADER,
        0,
        TextureFormat::Rgba8Unorm,
    ));
    let composite = [
        render_scene::SCENE_COLOR_FORMAT,
        render_scene::SCENE_HDR_FORMAT,
    ]
    .map(|format| (format, queue_pipeline(&cache, COMPOSITE_SHADER, 2, format)))
    .into_iter()
    .collect();
    commands.insert_resource(CompositionGpu {
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("selection halo sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        blur,
        composite,
        pyramids: HashMap::new(),
        styles: HashMap::new(),
    });
}

/// Uses Bevy's checked cache so compilation participates in the shared loading gate.
fn queue_pipeline(
    cache: &PipelineCache,
    shader: Handle<Shader>,
    masks: usize,
    format: TextureFormat,
) -> CachedRenderPipelineId {
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("host selection composition".into()),
        layout: vec![passes::masked_layout(false, masks)],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some(mod_render::shader::VERTEX_ENTRY.into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some(mod_render::shader::FRAGMENT_ENTRY.into()),
            targets: vec![Some(passes::color_target(format))],
            ..default()
        }),
        ..default()
    })
}

/// Computes all quarter-area levels with checked accounting, including one-pixel tails.
fn level_sizes(mut size: Extent3d) -> Vec<Extent3d> {
    (0..MASK_BLUR_LEVELS)
        .map(|_| {
            size.width = size.width.div_ceil(2).max(1);
            size.height = size.height.div_ceil(2).max(1);
            size
        })
        .collect()
}

/// Packs physical pixel radii and display-encoded colours into the existing frame ABI.
fn style_uniform(view: &ExtractedView, style: MaskStyle, size: Extent3d) -> FrameUniform {
    let mut params = [0.0; mod_api::MAX_PASS_PARAMS];
    params[..4].copy_from_slice(&style.outline);
    params[4..8].copy_from_slice(&style.glow);
    params[8] = style.outline_radius;
    params[9..13].copy_from_slice(&style.fill.unwrap_or([0.; 4]));
    let mut frame = passes::frame_uniform(view, 0.0, 0.0, 0, params);
    frame.resolution = [
        size.width as f32,
        size.height as f32,
        1.0 / size.width as f32,
        1.0 / size.height as f32,
    ];
    frame
}

/// Creates one standard fullscreen binding, with only explicit additional owner textures.
fn binding(
    device: &RenderDevice,
    cache: &PipelineCache,
    sampler: &Sampler,
    uniform: &Buffer,
    source: &TextureView,
    masks: &[&TextureView],
) -> BindGroup {
    let mut entries = vec![
        BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 1,
            resource: BindingResource::TextureView(source),
        },
        BindGroupEntry {
            binding: 2,
            resource: BindingResource::Sampler(sampler),
        },
    ];
    entries.extend(
        masks
            .iter()
            .enumerate()
            .map(|(index, view)| BindGroupEntry {
                binding: 4 + index as u32,
                resource: BindingResource::TextureView(view),
            }),
    );
    device.create_bind_group(
        "host selection composition",
        &cache.get_bind_group_layout(&passes::masked_layout(false, masks.len())),
        &entries,
    )
}

impl Pyramid {
    /// Allocates a complete shared chain or rejects it whole; no full-resolution blur is used.
    fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        sampler: &Sampler,
        source: &TextureView,
        view: &ExtractedView,
    ) -> Option<Self> {
        let size = source.texture().size();
        let sizes = level_sizes(size);
        let bytes: u64 = sizes
            .iter()
            .map(|size| u64::from(size.width) * u64::from(size.height) * 4)
            .sum();
        crate::gpu_allocation::checked(device, "shared selection halo", bytes, || {
            let mut levels: Vec<Level> = Vec::new();
            let mut previous_size = size;
            for size in sizes {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("shared selection halo level"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8Unorm,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let target = texture.create_view(&default());
                let uniform = device.create_buffer(&BufferDescriptor {
                    label: Some("selection halo dimensions"),
                    size: mod_render::shader::FRAME_UNIFORM_BYTES as u64,
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut frame =
                    passes::frame_uniform(view, 0.0, 0.0, 0, [0.0; mod_api::MAX_PASS_PARAMS]);
                frame.resolution = [
                    previous_size.width as f32,
                    previous_size.height as f32,
                    1.0 / previous_size.width as f32,
                    1.0 / previous_size.height as f32,
                ];
                queue.profiled_write_buffer(&uniform, 0, bytemuck::bytes_of(&frame));
                let previous = levels.last().map_or(source, |level| &level.view);
                let binding = binding(device, cache, sampler, &uniform, previous, &[]);
                levels.push(Level {
                    _texture: texture,
                    view: target,
                    _uniform: uniform,
                    binding,
                });
                previous_size = size;
            }
            Self { levels, size }
        })
    }
}

/// Keeps one pyramid per compatible mask texture and drops revoked sources before allocation.
fn prepare(
    scene: Res<ModRenderScene>,
    masks: Option<Res<MaskGpu>>,
    gpu: Option<ResMut<CompositionGpu>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    views: Query<(Entity, &ExtractedView, &ViewTarget)>,
) {
    let (Some(masks), Some(mut gpu)) = (masks, gpu) else {
        return;
    };
    let mut sources = HashMap::new();
    for (entity, view, _) in &views {
        for mask in scene.masks.iter() {
            if mask
                .style
                .is_some_and(|style| style.glow[3] > 0.0 && style.glow_radius > 0.0)
                && let Some(source) = masks.planned(entity, mask.handle)
            {
                sources.insert(source.id(), (source, view));
            }
        }
    }
    gpu.pyramids.retain(|id, _| sources.contains_key(id));
    let bytes: u64 = sources
        .values()
        .flat_map(|(source, _)| level_sizes(source.texture().size()))
        .map(|size| u64::from(size.width) * u64::from(size.height) * 4)
        .sum();
    let halo_budget = bytes
        .checked_add(masks.attachment_bytes)
        .is_some_and(|bytes| bytes <= MAX_MASK_ATTACHMENT_BYTES);
    if !halo_budget {
        gpu.pyramids.clear();
    }
    if halo_budget
        && gpu
            .blur
            .is_some_and(|id| cache.get_render_pipeline(id).is_some())
    {
        for (id, (source, view)) in sources {
            if gpu
                .pyramids
                .get(&id)
                .is_none_or(|pyramid| pyramid.size != source.texture().size())
            {
                gpu.pyramids.remove(&id);
                if let Some(pyramid) =
                    Pyramid::new(&device, &queue, &cache, &gpu.sampler, source, view)
                {
                    gpu.pyramids.insert(id, pyramid);
                }
            }
        }
    }
    gpu.styles.retain(|(entity, handle), _| {
        views.contains(*entity)
            && masks.planned(*entity, *handle).is_some()
            && scene
                .masks
                .iter()
                .any(|mask| mask.handle == *handle && mask.style.is_some())
    });
    for (entity, view, target) in &views {
        if !scene.masks.iter().any(|mask| mask.style.is_some()) {
            continue;
        }
        for mask in scene.masks.iter() {
            let Some(style) = mask.style else {
                continue;
            };
            let Some(coverage) = masks.planned(entity, mask.handle) else {
                continue;
            };
            let CompositionGpu {
                styles,
                pyramids,
                sampler,
                ..
            } = &mut *gpu;
            let style_gpu = styles
                .entry((entity, mask.handle))
                .or_insert_with(|| StyleGpu::new(&device));
            queue.profiled_write_buffer(
                &style_gpu.uniform,
                0,
                bytemuck::bytes_of(&style_uniform(view, style, coverage.texture().size())),
            );
            let halo = if style.glow[3] > 0.0 && style.glow_radius > 0.0 {
                pyramids
                    .get(&coverage.id())
                    .map(|pyramid| &pyramid.levels[halo_level(style.glow_radius)].view)
                    .unwrap_or(coverage)
            } else {
                coverage
            };
            style_gpu.prepare_bindings(
                &device,
                &cache,
                sampler,
                [target.main_texture_view(), target.main_texture_other_view()],
                [coverage, halo],
            );
        }
    }
}

/// Selects a bounded pyramid level; zero radius disables glow in the composition adapter.
fn halo_level(radius: f32) -> usize {
    ((radius.max(1.0).log2().ceil() as usize).saturating_sub(1)).min(MASK_BLUR_LEVELS - 1)
}

/// Encodes a single fullscreen pass without changing existing mod-pass timing slots.
fn fullscreen(
    context: &mut RenderContext,
    destination: &TextureView,
    pipeline: &RenderPipeline,
    binding: &BindGroup,
) {
    let attachments = [Some(RenderPassColorAttachment {
        view: destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(default()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("host selection composition"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Runs each compatible halo once, then overlays owner styles before UI, after grading.
fn draw(
    world: &World,
    query: ViewQuery<&ViewTarget>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let entity = query.entity();
    let target = query.into_inner();
    let (Some(scene), Some(masks), Some(gpu), Some(cache)) = (
        world.get_resource::<ModRenderScene>(),
        world.get_resource::<MaskGpu>(),
        world.get_resource::<CompositionGpu>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return Ok(());
    };
    if world
        .get_resource::<render_scene::PanoramaScene>()
        .is_some_and(|panorama| !panorama.game_visible())
    {
        return Ok(());
    }
    let mut blurred = std::collections::HashSet::new();
    for mask in scene.masks.iter() {
        let (Some(_), Some(coverage), Some(style_gpu), Some(pipeline)) = (
            mask.style,
            masks.sample(entity, mask.handle),
            gpu.styles.get(&(entity, mask.handle)),
            gpu.composite
                .get(&target.main_texture_format())
                .and_then(|id| cache.get_render_pipeline(*id)),
        ) else {
            continue;
        };
        let pyramid = gpu.pyramids.get(&coverage.id());
        if let Some(pyramid) = pyramid {
            let Some(blur) = gpu.blur.and_then(|id| cache.get_render_pipeline(id)) else {
                continue;
            };
            if blurred.insert(coverage.id()) {
                for level in &pyramid.levels {
                    fullscreen(&mut context, &level.view, blur, &level.binding);
                }
            }
        }
        let Some(binding) = style_gpu.binding(target.main_texture_view().id()) else {
            continue;
        };
        let post = target.post_process_write();
        fullscreen(&mut context, post.destination, pipeline, binding);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resource_tests;
