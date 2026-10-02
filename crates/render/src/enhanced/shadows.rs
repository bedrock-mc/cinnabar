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
}

impl FromWorld for EnhancedShadowPipelines {
    fn from_world(world: &mut World) -> Self {
        let (layout, cube, model) = crate::chunk::enhanced::shadow_sources(world);
        let cache = world.resource::<PipelineCache>();
        let queue = |shader: Handle<bevy::shader::Shader>, label: &'static str| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(label.into()),
                layout: vec![
                    layout.clone(),
                    crate::lighting::layout(),
                    enhanced_caster_layout(),
                ],
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
                    bias: DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                ..default()
            })
        };
        Self {
            cube: queue(cube, "enhanced cube shadow caster"),
            model: queue(model, "enhanced model shadow caster"),
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
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let (Some(cube), Some(model)) = (cube, model) {
                pass.set_bind_group(2, casters, &[(index as u64 * CASTER_SLOT_BYTES) as u32]);
                crate::chunk::enhanced::draw_shadow_geometry(
                    world,
                    bounds,
                    view_offset.offset,
                    &mut pass,
                    cube,
                    model,
                );
            }
            drop(pass);
            span.end(context.command_encoder());
        }
        Ok(())
    }
}
