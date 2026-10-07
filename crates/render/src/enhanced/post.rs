//! Enhanced post chain node: light shafts and the graded HDR
//! composite. Runs before the hand and UI so neither is tonemapped.

use bevy::{
    core_pipeline::FullscreenShader,
    ecs::query::QueryItem,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_resource::{
            BindGroup, BindGroupEntry, BindingResource, BlendState, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, FragmentState, LoadOp, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, StoreOp,
            TextureView,
        },
        renderer::{RenderContext, RenderDevice},
        view::{ViewDepthTexture, ViewTarget},
    },
};

use super::{
    ENHANCED_POST_SHADER_HANDLE, EnhancedRendering,
    gpu::{EnhancedGpu, EnhancedViews, POST_FORMAT, enhanced_post_layout},
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedPostLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedSkyLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedLightingLabel;

#[derive(Resource)]
pub(crate) struct EnhancedPostPipelines {
    shafts: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
    sky: CachedRenderPipelineId,
    cloud_shadow: CachedRenderPipelineId,
    effects: CachedRenderPipelineId,
    temporal: CachedRenderPipelineId,
    present: CachedRenderPipelineId,
    background: CachedRenderPipelineId,
}

impl EnhancedPostPipelines {
    pub(crate) fn lighting_ready(&self, cache: &PipelineCache) -> bool {
        [self.sky, self.cloud_shadow, self.effects]
            .into_iter()
            .all(|pipeline| cache.get_render_pipeline(pipeline).is_some())
    }
}

impl FromWorld for EnhancedPostPipelines {
    fn from_world(world: &mut World) -> Self {
        let vertex = world.resource::<FullscreenShader>().to_vertex_state();
        let cache = world.resource::<PipelineCache>();
        let queue = |label: &'static str, entry: &'static str, blend: Option<BlendState>| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(label.into()),
                layout: vec![enhanced_post_layout()],
                vertex: vertex.clone(),
                fragment: Some(FragmentState {
                    shader: ENHANCED_POST_SHADER_HANDLE,
                    entry_point: Some(entry.into()),
                    targets: vec![Some(ColorTargetState {
                        format: POST_FORMAT,
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        };
        Self {
            shafts: queue("enhanced light shafts", "light_shafts", None),
            composite: queue("enhanced composite", "composite", None),
            sky: queue("enhanced sky LUT", "sky_lut", None),
            cloud_shadow: queue("enhanced cloud shadows", "cloud_shadows", None),
            effects: queue("enhanced horizon AO and clouds", "effects", None),
            temporal: queue("enhanced temporal resolve", "temporal_resolve", None),
            present: queue("enhanced filmic display", "present", None),
            background: queue("enhanced sky background", "sky_background", None),
        }
    }
}

pub(crate) fn lighting_ready(world: &World) -> bool {
    let (Some(pipelines), Some(cache)) = (
        world.get_resource::<EnhancedPostPipelines>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return false;
    };
    pipelines.lighting_ready(cache)
        && world
            .resource::<super::cloud_noise::CloudNoiseVolume>()
            .ready()
}

struct PostInputs<'a> {
    frame: BindingResource<'a>,
    source: &'a TextureView,
    opaque: &'a TextureView,
    shafts: &'a TextureView,
    depth: &'a TextureView,
    shadow: &'a TextureView,
    history: &'a TextureView,
    effects: &'a TextureView,
    sky: &'a TextureView,
    exposure: BindingResource<'a>,
    motion: &'a TextureView,
    noise: &'a super::cloud_noise::CloudNoiseVolume,
}

/// Binds this view and the post-process inputs.
fn post_bind_group(
    device: &RenderDevice,
    cache: &PipelineCache,
    gpu: &EnhancedGpu,
    inputs: PostInputs,
) -> BindGroup {
    device.create_bind_group(
        "enhanced post bind group",
        &cache.get_bind_group_layout(&enhanced_post_layout()),
        &[
            BindGroupEntry {
                binding: 0,
                resource: inputs.frame,
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(inputs.source),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(&gpu.linear_sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::TextureView(inputs.opaque),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(inputs.shafts),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(inputs.depth),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::TextureView(inputs.shadow),
            },
            BindGroupEntry {
                binding: 7,
                resource: BindingResource::Sampler(&gpu.shadow_sampler),
            },
            BindGroupEntry {
                binding: 8,
                resource: BindingResource::TextureView(inputs.history),
            },
            BindGroupEntry {
                binding: 9,
                resource: BindingResource::TextureView(inputs.effects),
            },
            BindGroupEntry {
                binding: 10,
                resource: BindingResource::TextureView(inputs.sky),
            },
            BindGroupEntry {
                binding: 11,
                resource: inputs.exposure,
            },
            BindGroupEntry {
                binding: 12,
                resource: BindingResource::TextureView(inputs.motion),
            },
            BindGroupEntry {
                binding: 13,
                resource: BindingResource::TextureView(&inputs.noise.view),
            },
            BindGroupEntry {
                binding: 14,
                resource: BindingResource::Sampler(&inputs.noise.sampler),
            },
        ],
    )
}

/// Draws one fullscreen pass.
fn fullscreen_pass(
    context: &mut RenderContext,
    label: &'static str,
    target: &TextureView,
    load: LoadOp<wgpu::Color>,
    pipeline: &bevy::render::render_resource::RenderPipeline,
    bind_group: &BindGroup,
) {
    let diagnostics = context.diagnostic_recorder();
    let span = diagnostics.time_span(context.command_encoder(), label);
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load,
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    span.end(context.command_encoder());
}

#[derive(Default)]
pub(crate) struct EnhancedPostNode;

#[derive(Default)]
pub(crate) struct EnhancedSkyNode;

#[derive(Default)]
pub(crate) struct EnhancedLightingNode;

impl ViewNode for EnhancedLightingNode {
    type ViewQuery = (&'static ViewTarget, &'static EnhancedRendering);

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, settings): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if settings.reflection_capture || !target.is_hdr() || !lighting_ready(world) {
            return Ok(());
        }
        let (Some(pipelines), Some(gpu), Some(views), Some(cache)) = (
            world.get_resource::<EnhancedPostPipelines>(),
            world.get_resource::<EnhancedGpu>(),
            world.get_resource::<EnhancedViews>(),
            world.get_resource::<PipelineCache>(),
        ) else {
            return Ok(());
        };
        let Some(state) = views.0.get(&graph.view_entity()) else {
            return Ok(());
        };
        let Some(post) = &state.post else {
            return Ok(());
        };
        let Some(scene) = &state.scene else {
            return Ok(());
        };
        super::probes::resolve_reflections(context, world, state.history.seconds);
        if let Some(history) = &state.local_shadow
            && let Some(pipeline) = cache.get_render_pipeline(
                world
                    .resource::<super::local_shadow_history::LocalShadowPipeline>()
                    .0,
            )
        {
            history.render(context, pipeline);
        }
        if !world
            .resource::<super::depth::EnhancedDepthPipelines>()
            .ready(cache)
        {
            return Ok(());
        }
        let (Some(sky), Some(cloud_shadow), Some(effects)) = (
            cache.get_render_pipeline(pipelines.sky),
            cache.get_render_pipeline(pipelines.cloud_shadow),
            cache.get_render_pipeline(pipelines.effects),
        ) else {
            return Ok(());
        };
        let device = context.render_device().clone();
        let black = &gpu.fallback_colour;
        let shadow = state
            .shadow
            .as_ref()
            .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
        let bind = |effects, sky| {
            post_bind_group(
                &device,
                cache,
                gpu,
                PostInputs {
                    noise: world.resource::<super::cloud_noise::CloudNoiseVolume>(),
                    frame: state.frame.as_entire_binding(),
                    source: black,
                    opaque: black,
                    shafts: black,
                    depth: &scene.depth_view,
                    shadow,
                    history: black,
                    effects,
                    sky,
                    exposure: state.exposure.value.as_entire_binding(),
                    motion: &scene.motion_view,
                },
            )
        };
        let clear = LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let group = bind(black, black);
        if post.atmosphere_cache.sky_needs_update() {
            fullscreen_pass(context, "enhanced sky LUT", &post.sky, clear, sky, &group);
            if super::probes::update_environment_sky(
                context,
                world,
                &state.frame,
                &post.sky,
                &scene.depth_view,
            ) {
                post.atmosphere_cache.mark_sky_rendered();
            }
        }
        if post.atmosphere_cache.cloud_shadow_needs_update() {
            fullscreen_pass(
                context,
                "enhanced cloud shadows",
                &post.cloud_shadow,
                clear,
                cloud_shadow,
                &group,
            );
            post.atmosphere_cache.mark_cloud_shadow_rendered();
        }
        if post.atmosphere_cache.indirect_sources_ready()
            && let Some(group) = &state.indirect_bind_group
        {
            world
                .resource::<super::indirect::IndirectPipelines>()
                .dispatch(context, world, &state.indirect, group);
        }
        let group = bind(black, &post.sky);
        fullscreen_pass(
            context,
            "enhanced AO and volumetric clouds",
            &post.effects,
            clear,
            effects,
            &group,
        );
        Ok(())
    }
}

impl ViewNode for EnhancedSkyNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static EnhancedRendering,
    );

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, depth, settings): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if settings.reflection_capture || !target.is_hdr() || !lighting_ready(world) {
            return Ok(());
        }
        let (Some(pipelines), Some(gpu), Some(views), Some(cache)) = (
            world.get_resource::<EnhancedPostPipelines>(),
            world.get_resource::<EnhancedGpu>(),
            world.get_resource::<EnhancedViews>(),
            world.get_resource::<PipelineCache>(),
        ) else {
            return Ok(());
        };
        let Some(state) = views.0.get(&graph.view_entity()) else {
            return Ok(());
        };
        let Some(post) = &state.post else {
            return Ok(());
        };
        let Some(background) = cache.get_render_pipeline(pipelines.background) else {
            return Ok(());
        };
        let shadow = state
            .shadow
            .as_ref()
            .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
        let black = &gpu.fallback_colour;
        let group = post_bind_group(
            context.render_device(),
            cache,
            gpu,
            PostInputs {
                noise: world.resource::<super::cloud_noise::CloudNoiseVolume>(),
                frame: state.frame.as_entire_binding(),
                source: black,
                opaque: black,
                shafts: black,
                depth: depth.view(),
                shadow,
                history: black,
                effects: &post.effects,
                sky: &post.sky,
                exposure: state.exposure.value.as_entire_binding(),
                motion: state
                    .scene
                    .as_ref()
                    .map_or(black, |scene| &scene.motion_view),
            },
        );
        fullscreen_pass(
            context,
            "enhanced sky background",
            target.main_texture_view(),
            LoadOp::Load,
            background,
            &group,
        );
        Ok(())
    }
}

impl ViewNode for EnhancedPostNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static EnhancedRendering,
        Option<&'static super::probes::ProbeFace>,
    );

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, depth, settings, face): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !super::ENHANCED_RENDERING_ENABLED {
            return Ok(());
        }
        let (Some(pipelines), Some(gpu), Some(views), Some(cache)) = (
            world.get_resource::<EnhancedPostPipelines>(),
            world.get_resource::<EnhancedGpu>(),
            world.get_resource::<EnhancedViews>(),
            world.get_resource::<PipelineCache>(),
        ) else {
            return Ok(());
        };
        let Some(state) = views.0.get(&graph.view_entity()) else {
            return Ok(());
        };
        if let Some(face) = face {
            super::probes::capture(
                context,
                world,
                face.0,
                &state.frame,
                target.main_texture_view(),
                depth.view(),
            );
            return Ok(());
        }
        if settings.reflection_capture {
            return Ok(());
        }
        let Some(post_targets) = &state.post else {
            return Ok(());
        };
        let (
            Some(shafts),
            Some(composite),
            Some(_sky),
            Some(_cloud_shadow),
            Some(_effects),
            Some(temporal),
            Some(present),
            Some(_background),
        ) = (
            cache.get_render_pipeline(pipelines.shafts),
            cache.get_render_pipeline(pipelines.composite),
            cache.get_render_pipeline(pipelines.sky),
            cache.get_render_pipeline(pipelines.cloud_shadow),
            cache.get_render_pipeline(pipelines.effects),
            cache.get_render_pipeline(pipelines.temporal),
            cache.get_render_pipeline(pipelines.present),
            cache.get_render_pipeline(pipelines.background),
        )
        else {
            return Ok(());
        };
        if !target.is_hdr() {
            return Ok(());
        }
        let device = context.render_device().clone();
        let shadow = state
            .shadow
            .as_ref()
            .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
        let black = &gpu.fallback_colour;
        let bind = |source: &TextureView,
                    shafts: &TextureView,
                    history: &TextureView,
                    effects: &TextureView,
                    sky: &TextureView| {
            post_bind_group(
                &device,
                cache,
                gpu,
                PostInputs {
                    noise: world.resource::<super::cloud_noise::CloudNoiseVolume>(),
                    frame: state.frame.as_entire_binding(),
                    source,
                    opaque: state.scene.as_ref().map_or(black, |scene| &scene.mips[0]),
                    shafts,
                    depth: depth.view(),
                    shadow,
                    history,
                    effects,
                    sky,
                    exposure: state.exposure.value.as_entire_binding(),
                    motion: state
                        .scene
                        .as_ref()
                        .map_or(black, |scene| &scene.motion_view),
                },
            )
        };
        let clear = LoadOp::Clear(wgpu::Color::TRANSPARENT);

        let shaft_view = state.shafts.as_ref().map(|texture| &texture.default_view);
        if let Some(shaft_view) = shaft_view {
            let group = bind(black, black, black, black, black);
            fullscreen_pass(
                context,
                "enhanced light shafts",
                shaft_view,
                clear,
                shafts,
                &group,
            );
        }

        if !super::exposure::meter(
            context,
            world,
            &state.frame,
            &state.exposure,
            target.main_texture_view(),
            depth.view(),
            post_targets.size,
        ) {
            return Ok(());
        }
        let group = bind(
            target.main_texture_view(),
            shaft_view.unwrap_or(black),
            black,
            &post_targets.effects,
            &post_targets.sky,
        );
        fullscreen_pass(
            context,
            "enhanced composite",
            &post_targets.composite,
            clear,
            composite,
            &group,
        );
        let index = state.history.index as usize % 2;
        let group = bind(
            &post_targets.composite,
            black,
            &post_targets.history_views[1 - index],
            &post_targets.effects,
            black,
        );
        fullscreen_pass(
            context,
            "enhanced temporal resolve",
            &post_targets.history_views[index],
            clear,
            temporal,
            &group,
        );
        let post = target.post_process_write();
        let group = bind(
            &post_targets.history_views[index],
            black,
            black,
            black,
            black,
        );
        fullscreen_pass(
            context,
            "enhanced filmic display",
            post.destination,
            clear,
            present,
            &group,
        );
        state
            .history
            .submitted
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::actor_render::mark_actor_motion_submitted(world);
        Ok(())
    }
}
