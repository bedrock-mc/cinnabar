//! Opaque scene copies keep water sampling separate from active attachments.
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::{
    prelude::*,
    render::{
        render_resource::{TexelCopyTextureInfo, TextureAspect},
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};
use {super::gpu::EnhancedViews, render_scene::EnhancedRendering};

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
    let (entity, settings, _target, source, _depth) = query.into_inner();
    let context = &mut context;
    if !render_model::enhanced_rendering_enabled() || !settings.water_reflections {
        return Ok(());
    }
    let views = world.resource::<EnhancedViews>();
    let Some(state) = views.0.get(&entity) else {
        return Ok(());
    };
    let Some(scene) = &state.scene else {
        return Ok(());
    };
    let Some(depth) = &state.resolved_depth else {
        return Ok(());
    };
    depth.draw(context, world, None);
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(context.command_encoder(), "enhanced opaque snapshot");
    context.command_encoder().copy_texture_to_texture(
        source.texture.as_image_copy(),
        scene.colour.as_image_copy(),
        wgpu::Extent3d {
            width: scene.colour.width(),
            height: scene.colour.height(),
            depth_or_array_layers: 1,
        },
    );
    context.command_encoder().copy_texture_to_texture(
        TexelCopyTextureInfo {
            aspect: TextureAspect::DepthOnly,
            ..depth._texture.as_image_copy()
        },
        TexelCopyTextureInfo {
            aspect: TextureAspect::DepthOnly,
            ..scene.depth.as_image_copy()
        },
        scene.depth.size(),
    );
    span.end(context.command_encoder());
    super::probes::filter_mips(context, world, &state.frame, &scene.mips, &scene.depth_view);
    Ok(())
}
