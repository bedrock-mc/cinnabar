//! Own depth-only cascade pass for the vertex-pulled terrain arena.

use bevy::{
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
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

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct EnhancedShadowLabel;

#[derive(Resource)]
pub(crate) struct EnhancedShadowPipelines {
    cube: CachedRenderPipelineId,
    model: CachedRenderPipelineId,
}

impl FromWorld for EnhancedShadowPipelines {
    fn from_world(world: &mut World) -> Self {
        let (layout, cube, model, offsets) = crate::chunk::enhanced::shadow_sources(world);
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
                    buffers: vec![offsets.clone()],
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
                    depth_write_enabled: Some(true),
                    depth_compare: Some(CompareFunction::LessEqual),
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

type EnhancedShadowQuery = (
    Entity,
    &'static EnhancedRendering,
    &'static ViewUniformOffset,
);

/// Records the prepared enhanced shadow maps.
pub(crate) fn enhanced_shadows(
    world: &World,
    query: bevy::render::renderer::ViewQuery<EnhancedShadowQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (entity, settings, view_offset) = query.into_inner();
    let context = &mut context;
    if !super::enhanced_rendering_enabled() || !settings.shadows {
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
        let diagnostics = diagnostics.as_deref();
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
            multiview_mask: None,
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
