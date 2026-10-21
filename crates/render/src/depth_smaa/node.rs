use super::*;
use bevy::{
    anti_alias::smaa::{SmaaBindGroups, SmaaInfoUniformOffset},
    render::renderer::RenderContext,
};

type DepthSmaaQuery = (
    &'static Smaa,
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
    &'static DepthSmaaView,
    &'static SmaaTextures,
    &'static SmaaBindGroups,
    &'static SmaaInfoUniformOffset,
);

/// Filters scene edges directly into the shared samples before sharp world text.
pub(crate) fn depth_smaa(
    world: &World,
    query: bevy::render::renderer::ViewQuery<DepthSmaaQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (_, target, scene, view, textures, groups, offset) = query.into_inner();
    let context = &mut context;
    let cache = world.resource::<PipelineCache>();
    let (Some(edge), Some(weights), Some(blend)) = (
        cache.get_render_pipeline(view.ids.edge),
        cache.get_render_pipeline(view.ids.weights),
        cache.get_render_pipeline(view.ids.blend),
    ) else {
        return Ok(());
    };
    scene.finish(context, world, target);
    let (_, post) = view
        .post
        .iter()
        .find(|(id, _)| *id == target.main_texture_view().id())
        .expect("prepared SMAA source");
    {
        let attachments = [Some(RenderPassColorAttachment {
            view: &textures.edge_detection_color_texture.default_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })];
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("SMAA depth edges"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &textures.edge_detection_stencil_texture.default_view,
                    depth_ops: None,
                    stencil_ops: Some(Operations {
                        load: LoadOp::Clear(0),
                        store: StoreOp::Store,
                    }),
                }),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    render_scene::RuntimeStage::GpuPost,
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(edge);
        pass.set_bind_group(0, &view.depth, &[]);
        pass.set_stencil_reference(1);
        pass.draw(0..3, 0..1);
    }
    {
        let attachments = [Some(RenderPassColorAttachment {
            view: &textures.blend_texture.default_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })];
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("SMAA spatial weights"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &textures.edge_detection_stencil_texture.default_view,
                    depth_ops: None,
                    stencil_ops: Some(Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Discard,
                    }),
                }),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    render_scene::RuntimeStage::GpuPost,
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(weights);
        pass.set_bind_group(0, post, &[offset.0]);
        pass.set_bind_group(1, &groups.blending_weight_calculation_bind_group, &[]);
        pass.set_stencil_reference(1);
        pass.draw(0..3, 0..1);
    }
    {
        let attachments = [Some(RenderPassColorAttachment {
            view: scene.color_view(false),
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })];
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("SMAA spatial blending"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    render_scene::RuntimeStage::GpuPost,
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(blend);
        pass.set_bind_group(0, post, &[offset.0]);
        pass.set_bind_group(1, &groups.neighborhood_blending_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    scene.resume();
    Ok(())
}
