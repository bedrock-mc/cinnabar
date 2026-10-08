use super::*;

#[derive(Default)]
pub(super) struct EntityShadowNode;

impl ViewNode for EntityShadowNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ViewTarget,
        &'static SceneTarget,
        &'static ViewUniformOffset,
        &'static EntityShadowView,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, target, scene, offset, state): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let gpu = world.resource::<EntityShadowGpu>();
        let (Some([x0, y0, x1, y1]), Some((_, bind_group)), Some(pipeline)) = (
            state.rect,
            state.bind_group.as_ref(),
            world
                .resource::<PipelineCache>()
                .get_render_pipeline(state.pipeline),
        ) else {
            return Ok(());
        };
        if gpu.count == 0
            || world
                .get_resource::<crate::PanoramaScene>()
                .is_some_and(|panorama| !panorama.game_visible())
        {
            return Ok(());
        }
        let colour = scene.color_attachment(target, true);
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("entity shadows"),
            color_attachments: &[Some(colour)],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &state.stencil_view,
                depth_ops: None,
                stencil_ops: Some(Operations {
                    load: LoadOp::Clear(0),
                    store: StoreOp::Discard,
                }),
            }),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuShadows,
            ),
            occlusion_query_set: None,
        });
        if let Some(viewport) = camera.viewport.as_ref() {
            pass.set_camera_viewport(viewport);
        }
        pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
        pass.set_render_pipeline(pipeline);
        pass.set_stencil_reference(1);
        pass.set_bind_group(0, bind_group, &[offset.offset]);
        pass.set_vertex_buffer(0, gpu.mesh.slice(..));
        pass.draw(0..SHADOW_VOLUME_VERTICES as u32, 0..gpu.count);

        Ok(())
    }
}
