//! Keeps the hand outside grading without copying the graded scene into MSAA.
use super::{EnhancedRendering, gpu::EnhancedViews};
use crate::scene_target::SceneTarget;
use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_resource::{Texture, TextureView},
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
    },
};

/// A resolved transparent hand layer; the world's discarded MSAA storage is reused for drawing.
pub(crate) struct HandLayer {
    texture: Texture,
    view: TextureView,
    pipeline: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
}

impl HandLayer {
    /// Retains the composite pipeline and binding until the viewport format or extent changes.
    pub(crate) fn new(device: &RenderDevice, target: &ViewTarget) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("enhanced resolved hand"),
            size: target.main_texture().size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: target.main_texture_format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let gpu = device.wgpu_device();
        let shader = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("enhanced hand composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hand_layer.wgsl").into()),
        });
        let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("enhanced hand composite"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture.format(),
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: default(),
            depth_stencil: None,
            multisample: default(),
            multiview: None,
            cache: None,
        });
        let binding = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("enhanced hand composite input"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        Self {
            texture,
            view,
            pipeline,
            binding,
        }
    }

    /// A stable viewport needs no new attachment or bind group.
    pub(crate) fn matches(&self, target: &ViewTarget) -> bool {
        self.texture.size() == target.main_texture().size()
            && self.texture.format() == target.main_texture_format()
    }
}

/// Starts a transparent hand layer in the same attachment after the world has been resolved.
pub(crate) fn clear(context: &mut RenderContext, world: &World, scene: &SceneTarget) {
    let _pass = context
        .command_encoder()
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("enhanced hand layer clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: scene.color_view(false),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuHand,
            ),
            occlusion_query_set: None,
        });
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EnhancedHandCompositeLabel;
pub(crate) struct EnhancedHandCompositeNode;

impl ViewNode for EnhancedHandCompositeNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static SceneTarget,
        &'static EnhancedRendering,
    );

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, scene, _): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !render_model::enhanced_rendering_enabled() {
            return Ok(());
        }
        let Some(layer) = world
            .get_resource::<EnhancedViews>()
            .and_then(|views| views.0.get(&graph.view_entity()))
            .and_then(|view| view.hand_layer.as_ref())
        else {
            return Ok(());
        };
        if scene.texture.sample_count() > 1 {
            let _pass = context
                .command_encoder()
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("enhanced hand resolve"),
                    color_attachments: &[Some(
                        scene.resolve_attachment(&layer.view, wgpu::StoreOp::Discard),
                    )],
                    depth_stencil_attachment: None,
                    timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                        world,
                        crate::RuntimeStage::GpuHand,
                    ),
                    occlusion_query_set: None,
                });
        } else {
            context.command_encoder().copy_texture_to_texture(
                scene.texture.as_image_copy(),
                layer.texture.as_image_copy(),
                layer.texture.size(),
            );
        }
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("enhanced hand composite"),
                color_attachments: &[Some(target.get_unsampled_color_attachment())],
                depth_stencil_attachment: None,
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuHand,
                ),
                occlusion_query_set: None,
            });
        pass.set_pipeline(&layer.pipeline);
        pass.set_bind_group(0, &layer.binding, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hand_layer_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("hand_layer.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
