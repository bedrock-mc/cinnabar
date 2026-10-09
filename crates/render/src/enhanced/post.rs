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

#[derive(Resource)]
pub(crate) struct EnhancedPostPipelines {
    shafts: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
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
        }
    }
}

struct PostInputs<'a> {
    frame: BindingResource<'a>,
    source: &'a TextureView,
    bloom: &'a TextureView,
    shafts: &'a TextureView,
    depth: &'a TextureView,
    shadow: &'a TextureView,
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
                resource: BindingResource::TextureView(inputs.bloom),
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
        ],
    )
}

/// Draws one fullscreen pass.
fn fullscreen_pass(
    context: &mut RenderContext,
    world: &World,
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
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            crate::RuntimeStage::GpuPost,
        ),
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

impl ViewNode for EnhancedPostNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static crate::scene_target::SceneTarget,
        &'static EnhancedRendering,
    );

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, _depth, scene, _settings): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !super::enhanced_rendering_enabled() {
            return Ok(());
        }
        super::hand_layer::clear(context, world, scene);
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
        let (Some(shafts), Some(composite)) = (
            cache.get_render_pipeline(pipelines.shafts),
            cache.get_render_pipeline(pipelines.composite),
        ) else {
            return Ok(());
        };
        if !target.is_hdr() {
            return Ok(());
        }
        let Some(depth) = &state.resolved_depth else {
            return Ok(());
        };
        depth.draw(context, world, None);
        let device = context.render_device().clone();
        let shadow = state
            .shadow
            .as_ref()
            .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
        let black = &gpu.fallback_colour;
        let bind = |source: &TextureView, bloom: &TextureView, shafts: &TextureView| {
            post_bind_group(
                &device,
                cache,
                gpu,
                PostInputs {
                    frame: state.frame.as_entire_binding(),
                    source,
                    bloom,
                    shafts,
                    depth: &depth.view,
                    shadow,
                },
            )
        };
        let clear = LoadOp::Clear(wgpu::Color::TRANSPARENT);

        let shaft_view = state.shafts.as_ref().map(|texture| &texture.default_view);
        if let Some(shaft_view) = shaft_view {
            let group = bind(black, black, black);
            fullscreen_pass(
                context,
                world,
                "enhanced light shafts",
                shaft_view,
                clear,
                shafts,
                &group,
            );
        }

        let post = target.post_process_write();
        let group = bind(post.source, black, shaft_view.unwrap_or(black));
        fullscreen_pass(
            context,
            world,
            "enhanced composite",
            post.destination,
            clear,
            composite,
            &group,
        );
        Ok(())
    }
}
