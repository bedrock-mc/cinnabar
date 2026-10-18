//! Compares direct multisample filtering with a single-sample result restored to each sample.

use super::*;
use bevy::anti_alias::smaa::{SmaaBindGroups, SmaaInfoUniformOffset};
use std::sync::atomic::{AtomicBool, Ordering};

const RESTORE: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn restore(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}
struct Overlay { @location(0) colour: vec4<f32>, @builtin(sample_mask) mask: u32 }
@fragment fn overlay(@builtin(position) position: vec4<f32>) -> Overlay {
    if position.x < 17.0 || position.x > 47.0 { discard; }
    return Overlay(vec4(0.81, 0.24, 0.53, 0.6), 1u);
}
"#;

#[derive(Resource)]
struct BlendReference {
    intermediate: bevy::render::render_resource::Texture,
    samples: bevy::render::render_resource::Texture,
    expected: bevy::render::render_resource::Texture,
    actual: bevy::render::render_resource::Texture,
    blend: CachedRenderPipelineId,
    restore: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
    draw_overlay: bool,
    completed: AtomicBool,
}

/// Allocates a renderable comparison image in the same format as the world attachment.
fn texture(
    device: &RenderDevice,
    format: TextureFormat,
    samples: u32,
) -> bevy::render::render_resource::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("SMAA sample comparison"),
        size: Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | if samples == 1 {
                TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC
            } else {
                TextureUsages::empty()
            },
        view_formats: &[],
    })
}

/// Draws either a full-screen restoration or a one-sample overlay without changing blend math.
fn pipeline(device: &RenderDevice, format: TextureFormat, entry: &str) -> wgpu::RenderPipeline {
    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SMAA restoration reference"),
            source: wgpu::ShaderSource::Wgsl(RESTORE.into()),
        });
    device
        .wgpu_device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SMAA restoration reference"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 4,
                ..default()
            },
            multiview_mask: None,
            cache: None,
        })
}

/// Reconstructs the former two-pass output using the same production weights and blend shader.
fn reference(
    world: &World,
    query: bevy::render::renderer::ViewQuery<(
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static DepthSmaaView,
        &'static SmaaBindGroups,
        &'static SmaaInfoUniformOffset,
    )>,
    mut context: RenderContext,
) {
    let (target, scene, view, groups, offset) = query.into_inner();
    let Some(state) = world.get_resource::<BlendReference>() else {
        return;
    };
    let Some(blend) = world
        .resource::<PipelineCache>()
        .get_render_pipeline(state.blend)
    else {
        return;
    };
    let (_, post) = view
        .post
        .iter()
        .find(|(id, _)| *id == target.main_texture_view().id())
        .unwrap();
    let intermediate = state.intermediate.create_view(&default());
    let samples = state.samples.create_view(&default());
    let expected = state.expected.create_view(&default());
    let actual = state.actual.create_view(&default());
    let encoder = context.command_encoder();
    {
        let attachments = [Some(attachment(
            &intermediate,
            LoadOp::Clear(wgpu::Color::TRANSPARENT),
        ))];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &attachments,
            ..default()
        });
        pass.set_pipeline(blend);
        pass.set_bind_group(0, post, &[offset.0]);
        pass.set_bind_group(1, &groups.neighborhood_blending_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    {
        let attachments = [Some(attachment(
            &samples,
            LoadOp::Clear(wgpu::Color::TRANSPARENT),
        ))];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &attachments,
            ..default()
        });
        pass.set_pipeline(&state.restore);
        pass.set_bind_group(0, &state.binding, &[]);
        pass.draw(0..3, 0..1);
    }
    if state.draw_overlay {
        for view in [&*samples, &**scene.color_view(false)] {
            let attachments = [Some(attachment(view, LoadOp::Load))];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &attachments,
                ..default()
            });
            pass.set_pipeline(&state.overlay);
            pass.draw(0..3, 0..1);
        }
    }
    for (view, output) in [
        (&*samples, &*expected),
        (&**scene.color_view(false), &*actual),
    ] {
        let mut colour = attachment(view, LoadOp::Load);
        colour.resolve_target = Some(output);
        let attachments = [Some(colour)];
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &attachments,
            ..default()
        });
    }
    state.completed.store(true, Ordering::Relaxed);
}

/// Retains all scene samples for a later resolve or partially covered overlay.
fn attachment(
    view: &wgpu::TextureView,
    load: LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load,
            store: StoreOp::Store,
        },
    }
}

/// Reads raw colour bytes so tone mapping cannot hide a difference in the scene samples.
fn read_texture(app: &App, texture: &bevy::render::render_resource::Texture) -> Vec<u8> {
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let row = SIDE * texture.format().block_copy_size(None).unwrap();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("SMAA comparison readback"),
        size: u64::from(row * SIDE),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.wgpu_device().create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(SIDE),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

#[test]
fn direct_blending_matches_restored_colour_and_partial_overlay_coverage() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, _) = app(render);
    *app.world_mut().get_mut::<Msaa>(camera).unwrap() = Msaa::Sample4;
    app.world_mut().entity_mut(camera).insert(Smaa::default());
    app.sub_app_mut(RenderApp)
        .world_mut()
        .schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                reference
                    .after(DepthSmaaLabel)
                    .before(crate::scene_target::ScenePass::Finish)
                    .in_set(Core3dSystems::MainPass),
            );
        });
    for format in [
        render_scene::SCENE_COLOR_FORMAT,
        render_scene::SCENE_HDR_FORMAT,
    ] {
        let world = app.sub_app(RenderApp).world();
        let adapter = world.resource::<RenderAdapter>();
        if !adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(4)
        {
            eprintln!("skipping SMAA comparison: missing {format:?} 4x MSAA fixture");
            continue;
        }
        let device = world.resource::<RenderDevice>();
        let intermediate = texture(device, format, 1);
        let restore = pipeline(device, format, "restore");
        let binding = device
            .wgpu_device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SMAA restoration reference"),
                layout: &restore.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &intermediate.create_view(&default()),
                    ),
                }],
            });
        let mut state = BlendReference {
            intermediate,
            samples: texture(device, format, 4),
            expected: texture(device, format, 1),
            actual: texture(device, format, 1),
            blend: CachedRenderPipelineId::INVALID,
            restore,
            overlay: pipeline(device, format, "overlay"),
            binding,
            draw_overlay: false,
            completed: AtomicBool::new(false),
        };
        let fixture = FixturePipelines {
            checker: fixture_pipeline_format(device, 4, format, "checker"),
            sloped_checker: fixture_pipeline_format(device, 4, format, "sloped_checker"),
            silhouette: fixture_pipeline_format(device, 4, format, "covered_colour"),
            scene: 2,
        };
        if format == render_scene::SCENE_HDR_FORMAT {
            app.world_mut().entity_mut(camera).insert(bevy::camera::Hdr);
        } else {
            app.world_mut()
                .entity_mut(camera)
                .remove::<bevy::camera::Hdr>();
        }
        let world = app.sub_app_mut(RenderApp).world_mut();
        world.remove_resource::<BlendReference>();
        world.insert_resource(fixture);
        frame(&mut app);
        let world = app.sub_app_mut(RenderApp).world_mut();
        world.resource_scope(|world, mut pipelines: Mut<DepthSmaaPipelines>| {
            state.blend = pipelines
                .ids(world.resource::<PipelineCache>(), 1, format)
                .blend;
        });
        world.insert_resource(state);
        ready(&mut app);
        for overlay in [false, true] {
            {
                let mut state = app
                    .sub_app_mut(RenderApp)
                    .world_mut()
                    .resource_mut::<BlendReference>();
                state.draw_overlay = overlay;
                state.completed.store(false, Ordering::Relaxed);
            }
            frame(&mut app);
            let state = app.sub_app(RenderApp).world().resource::<BlendReference>();
            assert!(state.completed.load(Ordering::Relaxed));
            let actual = read_texture(&app, &state.actual);
            let expected = read_texture(&app, &state.expected);
            assert!(actual.iter().any(|byte| *byte != 0));
            assert!(
                actual == expected,
                "{format:?}, partial overlay={overlay}: restored colour differs"
            );
            if format == render_scene::SCENE_COLOR_FORMAT && !overlay {
                capture("direct-blend", 4, &actual);
                capture("restored-blend", 4, &expected);
            }
        }
    }
}
