//! Water identity is recorded beside sorted terrain colour, before post-processing.

use bevy::render::{
    render_resource::*,
    renderer::{RenderContext, RenderDevice},
};

pub(crate) const FORMAT: TextureFormat = TextureFormat::R32Float;

pub(crate) struct WaterCoverage {
    _texture: Texture,
    pub(crate) view: TextureView,
}

impl WaterCoverage {
    /// Keeps the water depth at each terrain fragment with the main attachment's sample count.
    pub(crate) fn new(device: &RenderDevice, size: Extent3d, samples: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Enhanced water coverage depth"),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: TextureDimension::D2,
            format: FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
        }
    }

    /// Loads coverage across sorted terrain ranges so nearer models can replace it with zero.
    pub(crate) fn attachment(&self) -> RenderPassColorAttachment<'_> {
        RenderPassColorAttachment {
            view: &self.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        }
    }

    /// Removes the previous frame's water even when the new frame contains no terrain draws.
    pub(crate) fn clear(&self, context: &mut RenderContext) {
        let mut attachment = self.attachment();
        attachment.ops.load = LoadOp::Clear(wgpu::Color::TRANSPARENT);
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("clear Enhanced water coverage"),
                color_attachments: &[Some(attachment)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
    }
}

/// Adds an unblended depth marker without changing the terrain's colour or depth blending.
#[cfg(feature = "enhanced")]
pub(crate) fn apply_pipeline(descriptor: &mut RenderPipelineDescriptor) {
    descriptor
        .fragment
        .as_mut()
        .expect("terrain fragment")
        .targets
        .push(Some(target()));
}

/// Returns the exact coverage target used by the terrain pipeline and its validation fixtures.
#[cfg(feature = "enhanced")]
pub(crate) fn target() -> ColorTargetState {
    ColorTargetState {
        format: FORMAT,
        blend: None,
        write_mask: ColorWrites::RED,
    }
}
