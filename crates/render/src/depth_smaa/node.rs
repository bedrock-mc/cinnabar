use super::*;
use bevy::{
    anti_alias::smaa::{SmaaBindGroups, SmaaInfoUniformOffset},
    ecs::query::QueryItem,
    render::{
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        renderer::RenderContext,
    },
};

pub(super) struct DepthSmaaNode;

impl ViewNode for DepthSmaaNode {
    type ViewQuery = (
        &'static Smaa,
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static DepthSmaaView,
        &'static SmaaTextures,
        &'static SmaaBindGroups,
        &'static SmaaInfoUniformOffset,
    );

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (_, target, scene, view, textures, groups, offset): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let cache = world.resource::<PipelineCache>();
        let (Some(edge), Some(weights), Some(blend), Some(restore)) = (
            cache.get_render_pipeline(view.ids.edge),
            cache.get_render_pipeline(view.ids.weights),
            cache.get_render_pipeline(view.ids.blend),
            cache.get_render_pipeline(view.ids.restore),
        ) else {
            return Ok(());
        };
        scene.finish(context, world, target);
        let output = target.post_process_write();
        let (_, post, _) = view
            .post
            .iter()
            .find(|(id, _, _)| *id == output.source.id())
            .expect("prepared SMAA source");
        let (_, _, restored) = view
            .post
            .iter()
            .find(|(id, _, _)| *id == output.destination.id())
            .expect("prepared SMAA destination");
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
                        crate::RuntimeStage::GpuPost,
                    ),
                    occlusion_query_set: None,
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
                        crate::RuntimeStage::GpuPost,
                    ),
                    occlusion_query_set: None,
                });
            pass.set_pipeline(weights);
            pass.set_bind_group(0, post, &[offset.0]);
            pass.set_bind_group(1, &groups.blending_weight_calculation_bind_group, &[]);
            pass.set_stencil_reference(1);
            pass.draw(0..3, 0..1);
        }
        {
            let attachments = [Some(RenderPassColorAttachment {
                view: output.destination,
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
                        crate::RuntimeStage::GpuPost,
                    ),
                    occlusion_query_set: None,
                });
            pass.set_pipeline(blend);
            pass.set_bind_group(0, post, &[offset.0]);
            pass.set_bind_group(1, &groups.neighborhood_blending_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if view.samples == 1 {
            context.command_encoder().copy_texture_to_texture(
                target.main_texture().as_image_copy(),
                scene.texture.as_image_copy(),
                scene.texture.size(),
            );
        } else {
            let attachments = [Some(scene.color_attachment(target, false))];
            let mut pass = context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("SMAA restore shared scene"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                        world,
                        crate::RuntimeStage::GpuPost,
                    ),
                    occlusion_query_set: None,
                });
            pass.set_pipeline(restore);
            pass.set_bind_group(0, restored, &[]);
            pass.draw(0..3, 0..1);
        }
        scene.resume();
        Ok(())
    }
}
