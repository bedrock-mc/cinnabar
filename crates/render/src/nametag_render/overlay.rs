//! Composes world labels in encoded colour after the world's final post-process pass.

use super::*;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_resource::{
            LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
            TextureViewId,
        },
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};
use std::sync::Mutex;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NametagOverlay;

#[derive(Resource)]
struct Installed;

/// Graded and pack views keep labels outside every world colour effect.
pub(crate) fn enabled(enhanced: bool, pack: bool) -> bool {
    (render_model::enhanced_rendering_enabled() && enhanced)
        || (cfg!(feature = "shaderpacks") && pack)
}

/// Installs the label overlay before the HUD, after world post-processing and hands.
pub(super) fn install_graph(world: &mut World) {
    if world.contains_resource::<Installed>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    draw,
                    Some(render_scene::RuntimeStage::GpuUi),
                    "NametagOverlay",
                )
                .in_set(NametagOverlay)
                .after(Core3dSystems::PostProcess)
                .after(crate::mod_render::ModPassLabel)
                .after(crate::mod_render::composition::ModCompositionLabel)
                .before(crate::ui_render::UiOverlayLabel)
                .before(crate::ui_render::overlay::UiOverlayPostLabel)
                .before(bevy::core_pipeline::upscaling::upscaling),
            );
        })
        .is_ok()
    {
        world.insert_resource(Installed);
    }
}

#[derive(Component)]
pub(super) struct Overlay {
    layer: bevy::render::render_resource::Texture,
    view: TextureView,
    samples: u32,
    depth_source: TextureViewId,
    composite: wgpu::RenderPipeline,
    bindings: Mutex<Vec<(TextureViewId, wgpu::BindGroup)>>,
    format: TextureFormat,
}

type OverlayView = (
    Entity,
    &'static ViewTarget,
    &'static ViewDepthStencilTexture,
    &'static Msaa,
    Has<render_scene::EnhancedRendering>,
    Has<render_scene::shaderpack::ShaderPackRendering>,
    Option<&'static Overlay>,
);

/// Prepares each opted-in view before its first label, retaining unchanged GPU resources,
/// and releases the layer of views that no longer use it.
pub(super) fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    views: Query<OverlayView>,
    mut failed: Local<crate::gpu_allocation::FailedRequests<(Extent3d, TextureFormat, u32)>>,
) {
    failed.retain(|view| views.contains(view));
    for (entity, target, depth, msaa, enhanced, pack, overlay) in &views {
        if !enabled(enhanced, pack) {
            if overlay.is_some() {
                commands.entity(entity).remove::<Overlay>();
            }
            continue;
        }
        if overlay.is_some_and(|overlay| {
            overlay.layer.size() == target.main_texture().size()
                && overlay.format == target.main_texture_format()
                && overlay.samples == msaa.samples()
                && overlay.depth_source == crate::scene_sampling::view_depth(depth).id()
        }) {
            continue;
        }
        let descriptor = TextureDescriptor {
            label: Some("nametag encoded overlay"),
            size: target.main_texture().size(),
            mip_level_count: 1,
            sample_count: msaa.samples(),
            dimension: TextureDimension::D2,
            format: render_scene::SCENE_COLOR_FORMAT.remove_srgb_suffix(),
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let size = target.main_texture().size();
        let bytes = u64::from(size.width) * u64::from(size.height) * 4 * u64::from(msaa.samples());
        let Some(layer) = failed.attempt(
            entity,
            (size, target.main_texture_format(), msaa.samples()),
            || {
                crate::gpu_allocation::checked(&device, "nametag encoded overlay", bytes, || {
                    device.create_texture(&descriptor)
                })
            },
        ) else {
            commands.entity(entity).remove::<Overlay>();
            continue;
        };
        let view = layer.create_view(&default());
        let shader = device
            .wgpu_device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("nametag encoded composite"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../ui_composite.wgsl").into()),
            });
        let composite =
            device
                .wgpu_device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("nametag encoded composite"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("composite_vertex"),
                        compilation_options: default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(if msaa.samples() > 1 {
                            "nametag_multisample_composite_fragment"
                        } else {
                            "nametag_composite_fragment"
                        }),
                        compilation_options: default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target.main_texture_format(),
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    primitive: default(),
                    depth_stencil: None,
                    multisample: default(),
                    multiview_mask: None,
                    cache: None,
                });
        commands.entity(entity).insert(Overlay {
            layer,
            view,
            composite,
            samples: msaa.samples(),
            depth_source: crate::scene_sampling::view_depth(depth).id(),
            bindings: default(),
            format: target.main_texture_format(),
        });
    }
}

type OverlayQuery = (
    &'static ExtractedCamera,
    Option<&'static MainPassResolutionOverride>,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static Overlay,
    &'static ViewDepthStencilTexture,
    Has<render_scene::EnhancedRendering>,
    Has<render_scene::shaderpack::ShaderPackRendering>,
);

/// Replays label batches against the original per-sample scene depth, then composes their colour.
fn draw(
    world: &World,
    query: bevy::render::renderer::ViewQuery<OverlayQuery>,
    mut context: RenderContext,
) -> Result {
    let entity = query.entity();
    let (camera, resolution, view, target, overlay, depth, enhanced, pack) = query.into_inner();
    if !enabled(enhanced, pack) {
        return Ok(());
    }
    let Some(draw) = super::draw_function(world) else {
        return Ok(());
    };
    let Some(phases) = world.get_resource::<ViewSortedRenderPhases<Transparent3d>>() else {
        return Ok(());
    };
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return Ok(());
    };
    let mut ranges =
        crate::chunk::transparent::gamma_pass::contiguous_ranges(phase.items.values(), |item| {
            item.draw_function() == draw
        })
        .filter_map(|(range, tag)| tag.then_some(range))
        .peekable();
    if ranges.peek().is_none() {
        return Ok(());
    }
    let viewport = match Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
    {
        Some(viewport) => match crate::render_bounds::viewport(
            &viewport,
            crate::render_bounds::extent(&overlay.view),
        ) {
            Some(viewport) => Some(viewport),
            None => return Ok(()),
        },
        None => None,
    };
    {
        let attachments = [Some(RenderPassColorAttachment {
            view: &overlay.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("nametags after world post-processing"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuUi,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &viewport {
            pass.set_camera_viewport(viewport);
        }
        for range in ranges {
            phase.render_range(&mut pass, world, entity, range)?;
        }
    }
    let post = target.post_process_write();
    let device = world.resource::<RenderDevice>().wgpu_device();
    let mut bindings = overlay.bindings.lock().expect("nametag composite bindings");
    if !bindings.iter().any(|(id, _)| *id == post.source.id()) {
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nametag composite inputs"),
            layout: &overlay.composite.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: if overlay.samples > 1 { 2 } else { 0 },
                    resource: wgpu::BindingResource::TextureView(&overlay.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(post.source),
                },
            ],
        });
        // ViewTarget owns exactly two alternating source views.
        if bindings.len() == 2 {
            bindings.clear();
        }
        bindings.push((post.source.id(), binding));
    }
    let binding = &bindings
        .iter()
        .find(|(id, _)| *id == post.source.id())
        .unwrap()
        .1;
    let attachments = [Some(RenderPassColorAttachment {
        view: post.destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("nametag encoded composition"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuUi,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(&overlay.composite);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
    Ok(())
}
