//! Returns replacement geometry depth to native overlays in the view's projection.

use super::passes::{FrameIo, ViewState};

/// Converts the pack's bottom-up finite depth after its final pass, retaining the view exterior.
pub(super) fn restore(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    state: &ViewState,
    io: &FrameIo<'_>,
) {
    let Some(depth) = &state.targets.geometry_depth else {
        return;
    };
    let pipeline = &state.pack.host.overlay_depth;
    let binding = state.overlay_depth.get_or_init(|| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader pack overlay depth"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: state.depth_parameters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&depth.view),
                },
            ],
        })
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("shader pack native overlay depth"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: io.scene_depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: io.timestamps.and_then(|timestamps| {
            timestamps.shaderpack_render_pass(render_scene::RuntimeStage::GpuUi)
        }),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let [x, y, w, h] = io.viewport.map(|value| value as f32);
    pass.set_viewport(x, y, w, h, 0., 1.);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
#[path = "overlay_depth_tests.rs"]
mod tests;
