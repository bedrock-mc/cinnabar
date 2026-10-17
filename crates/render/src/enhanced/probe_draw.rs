//! Sky backgrounds precede transparent capture draws; final capture only copies radiance.

use super::{DrawBindingKey, ProbeGpu, ProbePipelines, layout};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderContext},
};

/// Draws the capture background into the retained scene before transparent geometry.
pub(crate) fn sky(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    depth: &TextureView,
    target: &TextureView,
) -> bool {
    let source = &world
        .resource::<crate::enhanced::gpu::EnhancedGpu>()
        .fallback_colour;
    draw(context, world, frame, source, depth, target, ProbePass::Sky)
}

pub(super) enum ProbePass {
    Sky,
    Capture,
    Copy,
    CloudSky,
}

/// Draws one capture or filter pass, retaining scene colour for the sky background.
pub(super) fn draw(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    source: &TextureView,
    depth: &TextureView,
    target: &TextureView,
    pass: ProbePass,
) -> bool {
    let noise = world.resource::<crate::enhanced::cloud_noise::CloudNoiseVolume>();
    let scattering = world.resource::<crate::enhanced::multiple_scattering::MultipleScattering>();
    if !noise.ready() || !scattering.ready() {
        return false;
    }
    let cache = world.resource::<PipelineCache>();
    let pipelines = world.resource::<ProbePipelines>();
    let Some(pipeline) = cache.get_render_pipeline(match pass {
        ProbePass::Sky => pipelines.background,
        ProbePass::Capture => pipelines.capture,
        ProbePass::Copy => pipelines.mip,
        ProbePass::CloudSky => pipelines.cloud_sky,
    }) else {
        return false;
    };
    let gpu = world.resource::<crate::enhanced::gpu::EnhancedGpu>();
    let views = &world.resource::<crate::enhanced::gpu::EnhancedViews>().0;
    let bound_view = views.values().find(|view| view.frame.id() == frame.id());
    let sky_view = bound_view.and_then(|view| {
        view.capture_parent
            .and_then(|parent| views.get(&parent))
            .or(Some(view))
    });
    let sky = sky_view
        .and_then(|view| view.post.as_ref())
        .map_or(&gpu.fallback_colour, |post| &post.sky);
    let probe = world.resource::<ProbeGpu>();
    let mut groups = probe.draw_groups.lock().unwrap();
    let key = DrawBindingKey {
        frame: frame.id(),
        source: source.id(),
        depth: depth.id(),
        sky: sky.id(),
    };
    if groups.len() >= 64 && !groups.contains_key(&key) {
        groups.clear();
    }
    let group = groups.entry(key).or_insert_with(|| {
        context.render_device().create_bind_group(
            "enhanced probe filter",
            &cache.get_bind_group_layout(&layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(source),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&gpu.linear_sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(depth),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(sky),
                },
                BindGroupEntry {
                    binding: 13,
                    resource: BindingResource::TextureView(&noise.view),
                },
                BindGroupEntry {
                    binding: 14,
                    resource: BindingResource::Sampler(&noise.sampler),
                },
                BindGroupEntry {
                    binding: 22,
                    resource: BindingResource::TextureView(&scattering.view),
                },
                BindGroupEntry {
                    binding: 23,
                    resource: BindingResource::Sampler(&scattering.sampler),
                },
            ],
        )
    });
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("enhanced reflection capture/filter"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: match pass {
                    ProbePass::Sky => LoadOp::Load,
                    _ => LoadOp::Clear(wgpu::Color::TRANSPARENT),
                },
                store: StoreOp::Store,
            },
        })],
        ..default()
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
    true
}
