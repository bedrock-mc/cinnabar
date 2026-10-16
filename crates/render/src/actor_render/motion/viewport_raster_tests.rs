//! Depth equality witnesses execute the production motion draw with camera metadata.

use super::*;
use bevy::render::render_resource::BufferInitDescriptor;
use bevy::{
    app::SubApp,
    asset::Assets,
    camera::Viewport,
    ecs::system::RunSystemOnce,
    prelude::{UVec2, UVec4},
    render::{camera::ExtractedCamera, texture::CachedTexture},
};
use std::ops::Range;

const SIDE: u32 = 16;

/// Creates only the scene resources needed by the real motion draw, on the supplied device.
fn world(device: &RenderDevice, queue: &RenderQueue) -> World {
    let mut app = App::new();
    app.init_resource::<Assets<Shader>>();
    app.insert_sub_app(RenderApp, SubApp::new());
    let render = app.sub_app_mut(RenderApp);
    render.insert_resource(device.clone());
    render.insert_resource(queue.clone());
    render.insert_resource(PipelineCache::new(device.clone(), true));
    render.init_resource::<render_scene::WorldLighting>();
    render.init_resource::<render_scene::WorldFullbright>();
    crate::lighting::install(&mut app);
    let render = app.sub_app_mut(RenderApp);
    render.world_mut().run_schedule(Render);
    render
        .world_mut()
        .run_system_once(super::super::init_actor_gpu)
        .unwrap();
    std::mem::take(render.world_mut())
}

/// Records actual velocity geometry against scene depth written with the same camera range.
fn covered_pixel(device: &RenderDevice, queue: &RenderQueue, range: Range<f32>) -> [u8; 8] {
    let mut world = world(device, queue);
    let viewport = UVec4::new(0, 0, SIDE, SIDE);
    let texture = |format, usage| {
        device.create_texture(&TextureDescriptor {
            label: Some("actor viewport depth witness"),
            size: Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let depth_texture = texture(
        TextureFormat::Depth32Float,
        TextureUsages::RENDER_ATTACHMENT,
    );
    let depth = ViewDepthStencilTexture::new(
        CachedTexture {
            default_view: depth_texture.create_view(&Default::default()),
            texture: depth_texture,
        },
        Some(0.0),
        None,
    );
    let mut camera: ExtractedCamera = crate::render_test_support::camera(false);
    camera.viewport = Some(Viewport {
        physical_position: UVec2::ZERO,
        physical_size: UVec2::splat(SIDE),
        depth: range.clone(),
    });
    let entity = world.spawn((camera, ViewUniformOffset { offset: 0 })).id();
    let uniform_layout = BindGroupLayoutDescriptor::new(
        "viewport witness",
        &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: BufferSize::new(16),
            },
            count: None,
        }],
    );
    let mut descriptor = actor_pipeline_descriptor(uniform_layout.clone());
    descriptor.layout.push(layout());
    MotionSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: assets::EntityRenderMaterial::Default as u32,
            },
            &mut descriptor,
        )
        .unwrap();
    descriptor.depth_stencil.as_mut().unwrap().format = TextureFormat::Depth32Float;
    let handle = Handle::<Shader>::default();
    let cache = &mut *world.resource_mut::<PipelineCache>();
    cache.set_shader(
        handle.id(),
        crate::shader_safety::from_wgsl(
            r#"
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - 1.0, 0.25, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> { return vec4(1.0); }
"#,
            "viewport-depth-witness.wgsl",
        ),
    );
    descriptor.vertex.shader = handle.clone();
    descriptor.vertex.entry_point = Some("vertex".into());
    descriptor.vertex.shader_defs.clear();
    let fragment = descriptor.fragment.as_mut().unwrap();
    fragment.shader = handle;
    fragment.entry_point = Some("fragment".into());
    fragment.shader_defs.clear();
    let id = cache.queue_render_pipeline(descriptor);
    cache.block_on_render_pipeline(id);
    assert!(
        cache.get_render_pipeline(id).is_some(),
        "fixture pipeline did not compile: {:?}",
        cache.get_render_pipeline_state(id)
    );
    let uniform = device.create_buffer_with_data(&BufferInitDescriptor {
        label: None,
        contents: &[0; 16],
        usage: BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(
        "viewport witness",
        &cache.get_bind_group_layout(&uniform_layout),
        &[BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    );
    world.resource_mut::<ActorGpu>().bind_group = Some(bind_group);
    let mut motion = MotionView::new(device, &depth).unwrap();
    motion.texture = texture(
        MOTION_FORMAT,
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
    );
    motion.view = motion.texture.create_view(&Default::default());
    motion.frame_generation = world.resource::<ActorGpu>().frame_generation;
    let cache = world.resource::<PipelineCache>();
    motion.binding = Some(device.create_bind_group(
        "viewport motion witness",
        &cache.get_bind_group_layout(&layout()),
        &[
            BindGroupEntry {
                binding: 0,
                resource: motion.instances.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: motion.bones.buffer.as_entire_binding(),
            },
        ],
    ));
    motion.ready = true;
    motion.draws.push((
        crate::actor::gpu::ActorDrawSpan {
            material: 0,
            page: 0,
            first: 0,
            count: 1,
            vertex_count: 3,
        },
        id,
    ));
    let (drawn, commands) =
        crate::render_test_support::record(&mut world, device, |world, context| {
            {
                let mut attachment = depth.get_attachment(wgpu::StoreOp::Store);
                // The chosen binary-exact values match the opaque viewport's depth mapping.
                attachment.depth_ops = Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(range.start + 0.25 * (range.end - range.start)),
                    store: wgpu::StoreOp::Store,
                });
                let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    color_attachments: &[],
                    depth_stencil_attachment: Some(attachment),
                    ..Default::default()
                });
            }
            draw(world, &motion, entity, &depth, viewport, context)
        });
    assert!(drawn, "production motion draw declined prepared fixture");
    queue.submit(commands);
    let readback = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 256,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d {
                x: SIDE / 2,
                y: SIDE / 2,
                z: 0,
            },
            ..motion.texture.as_image_copy()
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receiver.recv().unwrap().unwrap();
    readback.slice(..).get_mapped_range().unwrap()[..8]
        .try_into()
        .unwrap()
}

#[test]
fn actor_motion_equal_depth_preserves_nondefault_camera_viewport_range() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("actor motion viewport depth raster")
    else {
        return;
    };
    let device = RenderDevice::from(gpu.device);
    let queue = RenderQueue::new(gpu.queue);
    let expected = [0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c];
    assert_eq!(
        covered_pixel(&device, &queue, 0.0..1.0),
        expected,
        "default viewport control must cover the actor"
    );
    assert_eq!(
        covered_pixel(&device, &queue, 0.25..0.75),
        expected,
        "production motion Equal depth must cover actors at the camera's nondefault depth range"
    );
}
