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
        let (Some(colour), Some(scene_depth)) = (&state.scene_colour, &state.scene_depth) else {
            return Ok(());
        };
        let diagnostics = context.diagnostic_recorder();
        let span = diagnostics.time_span(context.command_encoder(), "enhanced opaque snapshot");
        context.command_encoder().copy_texture_to_texture(
            target.main_texture().as_image_copy(),
            colour.texture.as_image_copy(),
            colour.texture.size(),
        );
        context.command_encoder().copy_texture_to_texture(
            TexelCopyTextureInfo {
                aspect: TextureAspect::DepthOnly,
                ..depth.texture.as_image_copy()
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
}
