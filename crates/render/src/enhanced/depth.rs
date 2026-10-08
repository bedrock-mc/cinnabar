//! Camera depth precedes screen-space visibility so occlusion modulates lighting.

use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_resource::{
            CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction, DepthBiasState,
            LoadOp, Operations, PipelineCache, RenderPassColorAttachment,
            RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
            StoreOp, TextureFormat,
        },
        renderer::RenderContext,
        view::ViewUniformOffset,
    },
};

use super::{
    EnhancedRendering,
    gpu::{EnhancedViews, SHADOW_FORMAT, enhanced_caster_layout},
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedDepthLabel;

#[derive(Resource)]
pub(crate) struct EnhancedDepthPipelines {
    cube: CachedRenderPipelineId,
    model: CachedRenderPipelineId,
    actor: CachedRenderPipelineId,
}

pub(super) fn camera_depth(mut descriptor: RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    descriptor.label = Some("enhanced camera depth".into());
    let depth = descriptor
        .depth_stencil
        .as_mut()
        .expect("caster depth state");
    depth.depth_compare = CompareFunction::GreaterEqual;
    depth.bias = DepthBiasState::default();
    if !descriptor
        .vertex
        .shader_defs
        .contains(&"ENHANCED_MOTION".into())
    {
        descriptor.vertex.shader_defs.push("ENHANCED_MOTION".into());
    }
    let fragment = descriptor
        .fragment
        .as_mut()
        .expect("alpha-tested camera coverage");
    if !fragment.shader_defs.contains(&"ENHANCED_MOTION".into()) {
        fragment.shader_defs.push("ENHANCED_MOTION".into());
    }
    fragment.entry_point = Some(
        if fragment
            .entry_point
            .as_deref()
            .is_some_and(|entry| entry.starts_with("actor_"))
        {
            "actor_fragment_motion"
        } else {
            "fragment_motion"
        }
        .into(),
    );
    fragment.targets = [TextureFormat::Rgba16Float, TextureFormat::Rg16Float]
        .into_iter()
        .map(|format| {
            Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })
        })
        .collect();
    descriptor
}

impl EnhancedDepthPipelines {
    pub(crate) fn ready(&self, cache: &PipelineCache) -> bool {
        [self.cube, self.model, self.actor]
            .into_iter()
            .all(|id| cache.get_render_pipeline(id).is_some())
    }
}

impl FromWorld for EnhancedDepthPipelines {
    fn from_world(world: &mut World) -> Self {
        let (layout, cube, model) = crate::chunk::enhanced::shadow_sources(world);
        let cache = world.resource::<PipelineCache>();
        let terrain = |shader| {
            cache.queue_render_pipeline(camera_depth(
                super::shadows::terrain_shadow_pipeline_descriptor(
                    layout.clone(),
                    shader,
                    "enhanced camera terrain depth",
                ),
            ))
        };
        Self {
            cube: terrain(cube),
            model: terrain(model),
            actor: cache.queue_render_pipeline(camera_depth(
                crate::actor_render::actor_motion_pipeline_descriptor(
                    enhanced_caster_layout(),
                    SHADOW_FORMAT,
                ),
            )),
        }
    }
}

pub(crate) struct EnhancedDepthNode;

impl ViewNode for EnhancedDepthNode {
    type ViewQuery = (
        Entity,
        &'static EnhancedRendering,
        &'static ViewUniformOffset,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (entity, settings, offset): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !super::ENHANCED_RENDERING_ENABLED || settings.reflection_capture {
            return Ok(());
        }
        let views = world.resource::<EnhancedViews>();
        let Some(state) = views.0.get(&entity) else {
            return Ok(());
        };
        let (Some(bind), Some(scene)) = (&state.depth_bind_group, &state.scene) else {
            return Ok(());
        };
        let pipelines = world.resource::<EnhancedDepthPipelines>();
        let cache = world.resource::<PipelineCache>();
        let (Some(cube), Some(model), Some(actor)) = (
            cache.get_render_pipeline(pipelines.cube),
            cache.get_render_pipeline(pipelines.model),
            cache.get_render_pipeline(pipelines.actor),
        ) else {
            return Ok(());
        };
        let diagnostics = context.diagnostic_recorder();
        let span = diagnostics.time_span(context.command_encoder(), "enhanced camera depth");
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("enhanced camera depth"),
            color_attachments: &[
                Some(RenderPassColorAttachment {
                    view: &scene.motion_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                }),
                Some(RenderPassColorAttachment {
                    view: &scene.receiver_normal_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(camera_depth_attachment(&scene.depth_view)),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(2, bind, &[0]);
        crate::chunk::enhanced::draw_depth_geometry(
            world,
            entity,
            &state.camera_clip,
            offset.offset,
            &mut pass,
            cube,
            model,
        );
        crate::actor_render::draw_depth_actors(world, offset.offset, &mut pass, actor);
        drop(pass);
        span.end(context.command_encoder());
        Ok(())
    }
}

/// Visibility coverage must never occlude the independent main color pass.
pub(super) fn camera_depth_attachment(
    view: &bevy::render::render_resource::TextureView,
) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(Operations {
            load: LoadOp::Clear(0.0),
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_depth_uses_reverse_z_without_moving_surface_contacts() {
        let descriptor = camera_depth(crate::actor_render::actor_motion_pipeline_descriptor(
            enhanced_caster_layout(),
            SHADOW_FORMAT,
        ));
        let depth = descriptor.depth_stencil.unwrap();
        assert_eq!(depth.depth_compare, CompareFunction::GreaterEqual);
        assert!(depth.depth_write_enabled);
        assert_eq!(depth.bias, DepthBiasState::default());
        let fragment = descriptor.fragment.unwrap();
        assert_eq!(
            fragment.targets[0].as_ref().unwrap().format,
            TextureFormat::Rgba16Float
        );
        assert_eq!(
            fragment.entry_point.as_deref(),
            Some("actor_fragment_motion")
        );
        assert_eq!(descriptor.layout.len(), 4);
    }
}
