//! Opaque scene copies keep water sampling separate from active attachments.
use super::{EnhancedRendering, gpu::EnhancedViews};
use bevy::{
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_resource::{TexelCopyTextureInfo, TextureAspect},
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct EnhancedSnapshotLabel;

type EnhancedSnapshotQuery = (
    Entity,
    &'static EnhancedRendering,
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
    &'static ViewDepthStencilTexture,
);

/// Captures scene colour and depth for enhanced material sampling.
pub(crate) fn enhanced_snapshot(
    world: &World,
    query: bevy::render::renderer::ViewQuery<EnhancedSnapshotQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (entity, settings, _target, scene, _depth) = query.into_inner();
    let context = &mut context;
    if !super::enhanced_rendering_enabled() || !settings.water_reflections {
        return Ok(());
    }
    let views = world.resource::<EnhancedViews>();
    let Some(state) = views.0.get(&entity) else {
        return Ok(());
    };
    let (Some(colour), Some(scene_depth)) = (&state.scene_colour, &state.scene_depth) else {
        return Ok(());
    };
    let Some(depth) = &state.resolved_depth else {
        return Ok(());
    };
    depth.draw(context, world, None);
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(context.command_encoder(), "enhanced opaque snapshot");
    if scene.texture.sample_count() > 1 {
        let _pass = context
            .command_encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("enhanced opaque colour resolve"),
                color_attachments: &[Some(
                    scene.resolve_attachment(&colour.default_view, wgpu::StoreOp::Store),
                )],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
    } else {
        context.command_encoder().copy_texture_to_texture(
            scene.texture.as_image_copy(),
            colour.texture.as_image_copy(),
            colour.texture.size(),
        );
    }
    context.command_encoder().copy_texture_to_texture(
        TexelCopyTextureInfo {
            aspect: TextureAspect::DepthOnly,
            ..depth._texture.as_image_copy()
        },
        TexelCopyTextureInfo {
            aspect: TextureAspect::DepthOnly,
            ..scene_depth.texture.as_image_copy()
        },
        scene_depth.texture.size(),
    );
    span.end(context.command_encoder());
    Ok(())
}
