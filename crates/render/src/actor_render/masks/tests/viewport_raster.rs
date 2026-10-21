//! Offscreen pixels exercise the mask draw against a full-size scene depth attachment.

use super::*;
use bevy::{
    asset::Assets,
    ecs::system::RunSystemOnce,
    render::{
        render_resource::BufferInitDescriptor,
        renderer::{CurrentView, PendingCommandBuffers},
        texture::CachedTexture,
        view::RetainedViewEntity,
    },
};

/// Records the production mask draw with a reduced main pass and an occluded left half.
fn pixels(device: &RenderDevice, queue: &RenderQueue, mode: MaskDepth, paired: bool) -> Vec<u8> {
    use crate::gpu_snapshot::SNAPSHOT_SIDE as SIDE;
    let mut app = App::new();
    app.init_resource::<Assets<Shader>>();
    app.insert_sub_app(RenderApp, SubApp::new());
    app.sub_app_mut(RenderApp)
        .insert_resource(device.clone())
        .insert_resource(queue.clone())
        .insert_resource(PipelineCache::new(device.clone(), true))
        .init_resource::<render_scene::WorldLighting>()
        .init_resource::<render_scene::WorldFullbright>();
    crate::lighting::install(&mut app);
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(Render);
    world
        .run_system_once(super::super::super::init_actor_gpu)
        .unwrap();
    world.init_resource::<MaskGpu>();
    world.init_resource::<PendingCommandBuffers>();
    let texture = |format, usage| {
        device.create_texture(&TextureDescriptor {
            label: Some("mask main pass viewport witness"),
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
    let depth_texture = texture(CORE_3D_DEPTH_FORMAT, TextureUsages::RENDER_ATTACHMENT);
    let depth = ViewDepthStencilTexture::new(
        CachedTexture {
            default_view: depth_texture.create_view(&default()),
            texture: depth_texture,
        },
        Some(0.0),
        None,
    );
    let mut target = MaskTarget::new(device, world.resource::<PipelineCache>(), &depth).unwrap();
    target.texture = texture(
        FORMAT,
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
    );
    target.render_view = target.texture.create_view(&default());
    target.view = target.render_view.clone();
    let mut camera = crate::render_test_support::camera(false);
    camera.viewport = Some(Viewport {
        physical_position: UVec2::new(32, 16),
        physical_size: UVec2::splat(192),
        depth: 0.25..0.75,
    });
    let entity = world.spawn_empty().id();
    world.entity_mut(entity).insert((
        camera,
        MainPassResolutionOverride(UVec2::splat(64)),
        ViewUniformOffset { offset: 0 },
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(entity.into(), None, 0),
            clip_from_view: bevy::math::Mat4::IDENTITY,
            world_from_view: bevy::prelude::GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: FORMAT,
            viewport: UVec4::new(32, 16, 192, 192),
            color_grading: default(),
            invert_culling: false,
        },
    ));
    let layout = BindGroupLayoutDescriptor::new(
        "mask viewport witness",
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
    let mut descriptor = actor_pipeline_descriptor(layout.clone());
    MaskSpecializer
        .specialize(
            Key {
                actor: ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: assets::EntityRenderMaterial::Default as u32,
                },
                depth: mode,
            },
            &mut descriptor,
        )
        .unwrap();
    let shader = Handle::<Shader>::default();
    let cache = &mut *world.resource_mut::<PipelineCache>();
    cache.set_shader(
        shader.id(),
        crate::shader_safety::from_wgsl(
            r#"
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - 1.0, 0.5, 1.0);
}
@fragment fn coverage() -> @location(0) vec4<f32> { return vec4(1.0); }
@fragment fn scene(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return select(0.0, 0.75, position.x < 64.0);
}
"#,
            "mask-viewport-witness.wgsl",
        ),
    );
    descriptor.vertex.shader = shader.clone();
    descriptor.vertex.entry_point = Some("vertex".into());
    descriptor.vertex.shader_defs.clear();
    let fragment = descriptor.fragment.as_mut().unwrap();
    fragment.shader = shader;
    fragment.entry_point = Some("coverage".into());
    fragment.shader_defs.clear();
    let mut scene = descriptor.clone();
    scene.fragment.as_mut().unwrap().entry_point = Some("scene".into());
    scene.fragment.as_mut().unwrap().targets.clear();
    scene.depth_stencil.as_mut().unwrap().depth_write_enabled = Some(true);
    scene.depth_stencil.as_mut().unwrap().depth_compare = Some(CompareFunction::Always);
    let mut prepass = descriptor.clone();
    prepass.fragment.as_mut().unwrap().targets.clear();
    prepass.depth_stencil.as_mut().unwrap().depth_write_enabled = Some(true);
    prepass.depth_stencil.as_mut().unwrap().depth_compare = Some(CompareFunction::GreaterEqual);
    let prepass_id = cache.queue_render_pipeline(prepass);
    if paired {
        descriptor.layout.push(dissolve_layout(false));
    }
    let mask_id = cache.queue_render_pipeline(descriptor);
    let scene_id = cache.queue_render_pipeline(scene);
    for id in [mask_id, scene_id, prepass_id] {
        cache.block_on_render_pipeline(id);
        assert!(
            cache.get_render_pipeline(id).is_some(),
            "{:?}",
            cache.get_render_pipeline_state(id)
        );
    }
    let uniform = device.create_buffer_with_data(&BufferInitDescriptor {
        label: None,
        contents: &[0; 16],
        usage: BufferUsages::UNIFORM,
    });
    let binding = device.create_bind_group(
        "mask viewport witness",
        &cache.get_bind_group_layout(&layout),
        &[BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    );
    world.resource_mut::<ActorGpu>().bind_group = Some(binding);
    target.frame_generation = world.resource::<ActorGpu>().frame_generation;
    target.ready = true;
    let span = |material| crate::actor::gpu::ActorDrawSpan {
        material,
        page: 0,
        first: 0,
        count: 1,
        vertex_count: 3,
    };
    target.draws = if paired {
        target.dissolve_binding = Some(
            device.create_bind_group(
                "mask sampled depth fixture",
                &world
                    .resource::<PipelineCache>()
                    .get_bind_group_layout(&dissolve_layout(false)),
                &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&target.dissolve_view),
                }],
            ),
        );
        vec![vec![
            (
                span(assets::EntityRenderMaterial::DissolveDepth as u32),
                prepass_id,
            ),
            (
                span(assets::EntityRenderMaterial::DissolveColor as u32),
                mask_id,
            ),
        ]]
    } else {
        vec![vec![(
            span(assets::EntityRenderMaterial::Default as u32),
            mask_id,
        )]]
    };
    let (_, commands) = crate::render_test_support::record(world, device, |world, context| {
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            depth_stencil_attachment: Some(depth.get_attachment(wgpu::StoreOp::Store)),
            ..default()
        });
        pass.set_render_pipeline(
            world
                .resource::<PipelineCache>()
                .get_render_pipeline(scene_id)
                .unwrap(),
        );
        pass.set_bind_group(
            0,
            world.resource::<ActorGpu>().bind_group.as_ref().unwrap(),
            &[0],
        );
        pass.set_bind_group(
            1,
            &world.resource::<crate::lighting::LightmapGpu>().bind_group,
            &[],
        );
        pass.set_bind_group(2, &target.placement_binding, &[]);
        pass.draw(0..3, 0..1);
    });
    queue.submit(commands);
    let handle = DrawingHandle {
        owner: 1,
        generation: 1,
        slot: 1,
    };
    let output = target.texture.clone();
    world
        .resource_mut::<MaskGpu>()
        .targets
        .insert((entity, handle), target);
    world.entity_mut(entity).insert(depth);
    world.insert_resource(CurrentView(entity));
    world.run_system_once::<_, (), _>(draw).unwrap();
    queue.submit(world.resource_mut::<PendingCommandBuffers>().finish());
    let readback = device.create_buffer(&BufferDescriptor {
        label: None,
        size: u64::from(SIDE * SIDE * 4),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: None,
            },
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receiver.recv().unwrap().unwrap();
    readback.slice(..).get_mapped_range().unwrap().to_vec()
}

#[test]
fn mask_pixels_use_main_pass_resolution_for_coverage_and_scene_depth() {
    use crate::gpu_snapshot::{self, Gpu, SNAPSHOT_SIDE};
    let Some(gpu) = Gpu::for_fixture("mask main pass viewport raster") else {
        return;
    };
    let device = RenderDevice::from(gpu.device);
    let queue = RenderQueue::new(gpu.queue);
    for mode in [MaskDepth::Test, MaskDepth::Always] {
        let pixels = pixels(&device, &queue, mode, false);
        gpu_snapshot::save(&format!("mask-main-pass-{mode:?}"), &pixels);
        for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let x = index as u32 % SNAPSHOT_SIDE;
            let y = index as u32 / SNAPSHOT_SIDE;
            let covered = (32..96).contains(&x)
                && (16..80).contains(&y)
                && (mode == MaskDepth::Always || x >= 64);
            assert_eq!(
                *pixel,
                if covered { [255; 4] } else { [0; 4] },
                "{mode:?} at ({x}, {y})"
            );
        }
    }
}

#[test]
fn translated_mask_dissolve_prepass_does_not_sample_its_writable_depth() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("translated mask private depth") else {
        return;
    };
    let device = RenderDevice::from(gpu.device);
    let queue = RenderQueue::new(gpu.queue);
    for mode in [MaskDepth::Test, MaskDepth::Always] {
        assert_eq!(
            pixels(&device, &queue, mode, true),
            pixels(&device, &queue, mode, false)
        );
    }
}
