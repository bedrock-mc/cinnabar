//! Single-sample consumers of the scene retain explicit MSAA resolve boundaries.

use crate::RuntimeStage;
use bevy::{
    prelude::World,
    render::{
        render_resource::{Texture, TextureView, TextureViewId},
        renderer::{RenderContext, RenderDevice},
        view::ViewDepthTexture,
    },
};

#[cfg(test)]
pub(crate) mod tests;

/// Nearest reverse-Z surface for effects that require a single-sample depth texture.
pub(crate) struct ResolvedDepth {
    pub(crate) _texture: Texture,
    pub(crate) view: TextureView,
    source: TextureViewId,
    stage: RuntimeStage,
    pipeline: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
}

impl ResolvedDepth {
    /// Samples depth instead of copying it and attributes the pass to its consumer.
    pub(crate) fn new(
        device: &RenderDevice,
        depth: &ViewDepthTexture,
        stage: RuntimeStage,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resolved scene depth"),
            size: depth.texture.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let gpu = device.wgpu_device();
        let shader = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("resolved scene depth"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene_depth.wgsl").into()),
        });
        let multi = depth.texture.sample_count() > 1;
        let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("resolved scene depth"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if multi { "multisampled" } else { "single" }),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let binding = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resolved scene depth input"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: u32::from(multi),
                resource: wgpu::BindingResource::TextureView(depth.view()),
            }],
        });
        Self {
            _texture: texture,
            view,
            source: depth.view().id(),
            stage,
            pipeline,
            binding,
        }
    }

    /// The source view identity includes both the attachment size and its sample count.
    pub(crate) fn matches(&self, depth: &ViewDepthTexture) -> bool {
        self.source == depth.view().id()
    }

    /// Reassigns a shared resolve when its first consuming mod pass changes.
    pub(crate) fn set_stage(&mut self, stage: RuntimeStage) {
        self.stage = stage;
    }

    /// Writes the nearest covered surface; Hi-Z uses its separate conservative farthest resolve.
    pub(crate) fn draw(&self, context: &mut RenderContext, world: &World, rect: Option<[u32; 4]>) {
        let rect = match rect {
            Some([x0, y0, x1, y1]) => match crate::render_bounds::scissor(
                render_model::UiScissor::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)),
                crate::render_bounds::extent(&self.view),
            ) {
                Some(rect) => Some(rect),
                None => return,
            },
            None => None,
        };
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene depth resolve"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(world, self.stage),
                occlusion_query_set: None,
                multiview_mask: None,
            });
        if let Some(rect) = rect {
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..3, 0..1);
    }
}
