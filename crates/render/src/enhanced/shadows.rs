//! Own depth-only cascade pass for the vertex-pulled terrain arena.

use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_resource::{
            CachedRenderPipelineId, CompareFunction, DepthBiasState, DepthStencilState,
            FragmentState, LoadOp, Operations, PipelineCache, PrimitiveState,
            RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
            StoreOp, VertexState,
        },
        renderer::RenderContext,
        view::ViewUniformOffset,
    },
};

use super::{
    EnhancedRendering,
    gpu::{CASTER_SLOT_BYTES, EnhancedViews, SHADOW_FORMAT, enhanced_caster_layout},
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedShadowLabel;

#[derive(Resource)]
pub(crate) struct EnhancedShadowPipelines {
    cube: CachedRenderPipelineId,
    model: CachedRenderPipelineId,
    actor: CachedRenderPipelineId,
}

impl EnhancedShadowPipelines {
    pub(crate) fn ready(&self, cache: &PipelineCache) -> bool {
        [self.cube, self.model, self.actor]
            .into_iter()
            .all(|pipeline| cache.get_render_pipeline(pipeline).is_some())
    }
}

pub(crate) fn shadow_raster_bias() -> DepthBiasState {
    DepthBiasState {
        constant: 1,
        slope_scale: 0.5,
        clamp: 0.0,
    }
}

pub(super) fn terrain_shadow_pipeline_descriptor(
    layout: bevy::render::render_resource::BindGroupLayoutDescriptor,
    shader: Handle<bevy::shader::Shader>,
    label: &'static str,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(label.into()),
        layout: vec![layout, crate::lighting::layout(), enhanced_caster_layout()],
        vertex: VertexState {
            shader: shader.clone(),
            shader_defs: vec!["ENHANCED_SHADOW".into()],
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            shader_defs: vec!["ENHANCED_SHADOW".into()],
            entry_point: Some("fragment_shadow".into()),
            targets: vec![],
        }),
        primitive: PrimitiveState {
            cull_mode: None,
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: SHADOW_FORMAT,
            depth_write_enabled: true,
            depth_compare: CompareFunction::LessEqual,
            stencil: default(),
            bias: shadow_raster_bias(),
        }),
        ..default()
    }
}

impl FromWorld for EnhancedShadowPipelines {
    fn from_world(world: &mut World) -> Self {
        let (layout, cube, model) = crate::chunk::enhanced::shadow_sources(world);
        let cache = world.resource::<PipelineCache>();
        let queue = |shader: Handle<bevy::shader::Shader>, label: &'static str| {
            cache.queue_render_pipeline(terrain_shadow_pipeline_descriptor(
                layout.clone(),
                shader,
                label,
            ))
        };
        Self {
            cube: queue(cube, "enhanced cube shadow caster"),
            model: queue(model, "enhanced model shadow caster"),
            actor: cache.queue_render_pipeline(
                crate::actor_render::actor_shadow_pipeline_descriptor(
                    enhanced_caster_layout(),
                    SHADOW_FORMAT,
                ),
            ),
        }
    }
}

pub(crate) struct EnhancedShadowNode;

impl ViewNode for EnhancedShadowNode {
    type ViewQuery = (
        Entity,
        &'static EnhancedRendering,
        &'static ViewUniformOffset,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (entity, settings, view_offset): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !super::ENHANCED_RENDERING_ENABLED || !settings.shadows {
            return Ok(());
        }
        let views = world.resource::<EnhancedViews>();
        let Some(view) = views.0.get(&entity) else {
            return Ok(());
        };
        let (Some(shadow), Some(casters)) = (&view.shadow, &view.caster_bind_group) else {
            return Ok(());
        };
        let pipelines = world.resource::<EnhancedShadowPipelines>();
        let cache = world.resource::<PipelineCache>();
        let cube = cache.get_render_pipeline(pipelines.cube);
        let model = cache.get_render_pipeline(pipelines.model);
        let actor = cache.get_render_pipeline(pipelines.actor);
        for (index, (layer, bounds)) in shadow.layers.iter().zip(&view.cascades).enumerate() {
            let diagnostics = context.diagnostic_recorder();
            let label = match index {
                0 => "enhanced shadow cascade near",
                1 => "enhanced shadow cascade middle",
                _ => "enhanced shadow cascade far",
            };
            let span = diagnostics.time_span(context.command_encoder(), label);
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("enhanced sun shadow cascade"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: layer,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuShadows,
                ),
                occlusion_query_set: None,
            });
            pass.set_bind_group(2, casters, &[(index as u64 * CASTER_SLOT_BYTES) as u32]);
            if let (Some(cube), Some(model)) = (cube, model) {
                crate::chunk::enhanced::draw_shadow_geometry(
                    world,
                    entity,
                    index,
                    bounds,
                    view_offset.offset,
                    &mut pass,
                    cube,
                    model,
                );
            }
            if let Some(actor) = actor {
                crate::actor_render::draw_shadow_actors(
                    world,
                    view_offset.offset,
                    &mut pass,
                    actor,
                );
            }
            drop(pass);
            span.end(context.command_encoder());
        }
        if view.local_lights.dirty
            && let (Some(cube), Some(model), Some(actor)) = (cube, model, actor)
        {
            let diagnostics = context.diagnostic_recorder();
            let span =
                diagnostics.time_span(context.command_encoder(), "enhanced cached lamp shadows");
            for (index, light) in view.local_lights.shadows.iter().enumerate() {
                let Some(layer) = shadow.layers.get(light.layer as usize) else {
                    continue;
                };
                let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    label: Some("enhanced lamp shadow face"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                        view: layer,
                        depth_ops: Some(Operations {
                            load: LoadOp::Clear(1.0),
                            store: StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                let edge = super::local_lights::POINT_SHADOW_RESOLUTION as f32;
                pass.set_viewport(0.0, 0.0, edge, edge, 0.0, 1.0);
                pass.set_bind_group(
                    2,
                    casters,
                    &[((view.cascades.len() + index) as u64 * CASTER_SLOT_BYTES) as u32],
                );
                crate::chunk::enhanced::draw_local_light_geometry(
                    world,
                    entity,
                    index,
                    &light.clip,
                    view_offset.offset,
                    &mut pass,
                    cube,
                    model,
                );
                crate::actor_render::draw_shadow_actors(
                    world,
                    view_offset.offset,
                    &mut pass,
                    actor,
                );
            }
            view.local_lights
                .submitted
                .store(true, std::sync::atomic::Ordering::Relaxed);
            span.end(context.command_encoder());
        }
        Ok(())
    }
}
