//! Opaque scene copies keep water sampling separate from active attachments.
use super::{EnhancedRendering, gpu::EnhancedViews};
use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_resource::{TexelCopyTextureInfo, TextureAspect},
        renderer::RenderContext,
        view::{ViewDepthTexture, ViewTarget},
    },
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedSnapshotLabel;

pub(crate) struct EnhancedSnapshotNode;

impl ViewNode for EnhancedSnapshotNode {
    type ViewQuery = (
        Entity,
        &'static EnhancedRendering,
        &'static ViewTarget,
        &'static ViewDepthTexture,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (entity, settings, target, depth): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !super::ENHANCED_RENDERING_ENABLED || !settings.water_reflections {
            return Ok(());
        }
        let views = world.resource::<EnhancedViews>();
        let Some(state) = views.0.get(&entity) else {
            return Ok(());
        };
        let Some(scene) = &state.scene else {
            return Ok(());
        };
        let diagnostics = context.diagnostic_recorder();
        let span = diagnostics.time_span(context.command_encoder(), "enhanced opaque snapshot");
        context.command_encoder().copy_texture_to_texture(
            target.main_texture().as_image_copy(),
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
                ..depth.texture.as_image_copy()
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
}
