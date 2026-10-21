use super::*;
use crate::{
    actor_raster,
    gpu_snapshot::{Gpu, RasterState, SNAPSHOT_SIDE},
};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::{GlobalTransform, Mat4, UVec4, Vec3};

#[test]
fn shared_halo_levels_are_bounded_even_for_odd_and_tiny_views() {
    for side in [1, 127, 128, 255, 1920] {
        let size = Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        };
        let levels = level_sizes(size);
        assert_eq!(levels.len(), MASK_BLUR_LEVELS);
        assert_eq!(levels[0].width, side.div_ceil(2).max(1));
        assert!(levels.windows(2).all(|pair| pair[1].width <= pair[0].width));
    }
    for radius in [0.0, 1.0, 2.0, 4.0, 8.0, 16.0] {
        assert!(halo_level(radius) < MASK_BLUR_LEVELS);
    }
}

#[test]
fn host_composition_and_declared_mask_layouts_validate() {
    let source = mod_render::shader::compose_masked(COMPOSITE, false, 2).unwrap();
    crate::shader_test_support::assert_binding_visibility(
        &source,
        0,
        &passes::masked_layout(false, 2),
    );
    for count in 0..=mod_api::drawing::MAX_MASKS {
        let effect = if count == 0 {
            "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }"
        } else {
            "fn effect(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(mask(0u, uv)); }"
        };
        for depth in [false, true] {
            let source = mod_render::shader::compose_masked(effect, depth, count).unwrap();
            crate::shader_test_support::assert_binding_visibility(
                &source,
                0,
                &passes::masked_layout(depth, count),
            );
        }
    }
}

/// Builds original actor geometry with one root and separate head, torso, arms and legs.
fn actor() -> Vec<render_model::ActorRigVertex> {
    let cubes = [
        ([-4, 12, -2], [8, 12, 4]),
        ([-4, 24, -4], [8, 8, 8]),
        ([-8, 12, -2], [4, 12, 4]),
        ([4, 12, -2], [4, 12, 4]),
        ([-4, 0, -2], [4, 12, 4]),
        ([0, 0, -2], [4, 12, 4]),
    ]
    .map(|(origin, size)| serde_json::json!({"origin":origin,"size":size,"uv":[0,0]}));
    let geometry = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.selection_fixture","texture_width":64,"texture_height":64},"bones":[{"name":"root","pivot":[0,0,0],"cubes":cubes}]}]});
    let skin = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.selection_fixture"}}"#,
        &geometry.to_string(),
    )
    .unwrap()
    .unwrap();
    render_model::skin_geometry(&skin, render_model::EntityRigId(0))
        .unwrap()
        .vertices
        .to_vec()
}

/// Uses production actor entry points for both the ordinary frame and its coverage witness.
fn actor_pixels(gpu: &Gpu, coverage: bool, samples: u32) -> Vec<u8> {
    let eye = Vec3::new(3.0, 2.6, 5.0);
    let clip = glam::camera::rh::proj::directx::orthographic(-1.5, 1.5, -1.5, 1.5, 10.0, 0.01)
        * glam::camera::rh::view::look_at_mat4(eye, Vec3::Y, Vec3::Y);
    let texels = vec![[105, 164, 213, 255]; 64 * 64];
    actor_raster::raster_material_in_scene(
        gpu,
        &actor(),
        render_model::ActorMaterial {
            kind: assets::EntityRenderMaterial::Default,
            ..Default::default()
        },
        actor_raster::RasterScene {
            clip_from_world: clip,
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            camera_position: eye,
            texture_size: [64, 64],
            texels: &texels,
        },
        !coverage,
        false,
        render_model::pack_actor_light(15, 15),
        0,
        0,
        &[],
        RasterState {
            actor_fragment: coverage.then_some("actor_mask_fragment"),
            clear_color: if coverage {
                wgpu::Color::TRANSPARENT
            } else {
                crate::gpu_snapshot::CLEAR_COLOR
            },
            multisample: wgpu::MultisampleState {
                count: samples,
                ..default()
            },
            ..default()
        },
    )
}

/// Uploads an original raster witness; textures remain local to the test device.
fn texture(
    device: &RenderDevice,
    queue: &RenderQueue,
    pixels: &[u8],
    format: TextureFormat,
) -> Texture {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("selection composition witness"),
        size: Extent3d {
            width: SNAPSHOT_SIDE,
            height: SNAPSHOT_SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::COPY_DST
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    queue.profiled_write_texture(
        texture.as_image_copy(),
        pixels,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SNAPSHOT_SIDE * 4),
            rows_per_image: Some(SNAPSHOT_SIDE),
        },
        texture.size(),
    );
    texture
}

/// Reads a complete rendered witness with aligned rows, never during production rendering.
fn pixels(device: &RenderDevice, queue: &RenderQueue, texture: &Texture) -> Vec<u8> {
    let size = u64::from(SNAPSHOT_SIDE * SNAPSHOT_SIDE * 4);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("selection capture"),
        size,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SNAPSHOT_SIDE * 4),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

/// Runs the same shared pyramid, bindings and fullscreen compositor used in the view schedule.
fn composed(
    gpu: &Gpu,
    before: &[u8],
    mask: &[u8],
    style: MaskStyle,
    format: TextureFormat,
) -> Vec<u8> {
    let device = RenderDevice::from(gpu.device.clone());
    let queue = RenderQueue::new(gpu.queue.clone());
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.insert_resource(PipelineCache::new(device.clone(), true));
    world.run_system_once(init).unwrap();
    world.resource_mut::<PipelineCache>().process_queue();
    let cache = world.resource::<PipelineCache>();
    let compositor = world.resource::<CompositionGpu>();
    let view = ExtractedView {
        retained_view_entity: bevy::render::view::RetainedViewEntity::new(
            Entity::PLACEHOLDER.into(),
            None,
            0,
        ),
        clip_from_view: Mat4::IDENTITY,
        world_from_view: GlobalTransform::IDENTITY,
        clip_from_world: None,
        target_format: format,
        viewport: UVec4::new(0, 0, SNAPSHOT_SIDE, SNAPSHOT_SIDE),
        color_grading: default(),
        invert_culling: false,
    };
    let scene = texture(&device, &queue, before, TextureFormat::Rgba8UnormSrgb);
    let mask = texture(&device, &queue, mask, TextureFormat::Rgba8Unorm);
    let coverage = mask.create_view(&default());
    let scene_view = scene.create_view(&default());
    let sampler = &compositor.sampler;
    let pyramid = Pyramid::new(&device, &queue, cache, sampler, &coverage, &view).unwrap();
    let blur = cache.get_render_pipeline(compositor.blur.unwrap()).unwrap();
    let composite = cache
        .get_render_pipeline(compositor.composite[&format])
        .unwrap();
    let output = device.create_texture(&TextureDescriptor {
        label: Some("selection colour output"),
        size: scene.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let output_view = output.create_view(&default());
    let mut style_gpu = StyleGpu::new(&device);
    queue.profiled_write_buffer(
        &style_gpu.uniform,
        0,
        bytemuck::bytes_of(&style_uniform(&view, style, scene.size())),
    );
    let halo = if style.glow_radius > 0.0 {
        &pyramid.levels[halo_level(style.glow_radius)].view
    } else {
        &coverage
    };
    style_gpu.prepare_bindings(
        &device,
        cache,
        sampler,
        [&scene_view, &scene_view],
        [&coverage, halo],
    );
    let group = style_gpu.binding(scene_view.id()).unwrap();
    let converted = texture(&device, &queue, before, TextureFormat::Rgba8UnormSrgb);
    let convert_source = mod_render::shader::compose(
        "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }",
        false,
    )
    .unwrap();
    let convert = passes::create_pipeline(
        &device,
        &cache.get_bind_group_layout(&passes::layout(false)),
        &convert_source,
        TextureFormat::Rgba8UnormSrgb,
    )
    .unwrap();
    let convert_binding = binding(
        &device,
        cache,
        sampler,
        &style_gpu.uniform,
        &output_view,
        &[],
    );
    let (_, commands) =
        crate::render_test_support::record(&mut World::new(), &device, |_, context| {
            for level in &pyramid.levels {
                fullscreen(context, &level.view, blur, &level.binding);
            }
            fullscreen(context, &output_view, composite, group);
            fullscreen(
                context,
                &converted.create_view(&default()),
                &convert,
                &convert_binding,
            );
        });
    queue.submit(commands);
    pixels(&device, &queue, &converted)
}

#[test]
fn outlines_and_halos_render_from_actor_coverage_in_ldr_and_hdr_targets() {
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "selection composition raster",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..default()
        },
    ) else {
        return;
    };
    let before = actor_pixels(&gpu, false, 1);
    let mask = actor_pixels(&gpu, true, 1);
    assert!(mask.as_chunks::<4>().0.iter().any(|pixel| pixel[0] == 255));
    let outline = MaskStyle {
        fill: None,
        outline: [1.0, 0.6, 0.1, 1.0],
        outline_radius: 2.0,
        glow: [0.1, 0.6, 1.0, 0.0],
        glow_radius: 0.0,
    };
    let glow = MaskStyle {
        fill: None,
        glow: [0.1, 0.6, 1.0, 1.0],
        glow_radius: 16.0,
        ..outline
    };
    let outlined = composed(&gpu, &before, &mask, outline, TextureFormat::Rgba8UnormSrgb);
    let glowing = composed(&gpu, &before, &mask, glow, TextureFormat::Rgba8UnormSrgb);
    let filled = composed(
        &gpu,
        &before,
        &mask,
        MaskStyle {
            fill: Some([0.1, 1., 0.2, 0.4]),
            ..outline
        },
        TextureFormat::Rgba8UnormSrgb,
    );
    assert_ne!(filled, outlined);
    for ((coverage, original), (outline_pixel, fill_pixel)) in mask
        .chunks_exact(4)
        .zip(before.chunks_exact(4))
        .zip(outlined.chunks_exact(4).zip(filled.chunks_exact(4)))
    {
        if coverage[0] == 255 {
            assert_ne!(outline_pixel, fill_pixel);
        }
        if coverage[0] == 0 && outline_pixel == original {
            assert_eq!(fill_pixel, original);
        }
    }
    assert_ne!(outlined, before);
    assert_ne!(glowing, outlined);
    let hdr = composed(&gpu, &before, &mask, glow, TextureFormat::Rgba16Float);
    assert!(
        hdr.iter().zip(&glowing).all(|(a, b)| a.abs_diff(*b) <= 2),
        "display-space style must survive the intermediate format"
    );
    let msaa = actor_pixels(&gpu, true, 4);
    let multisampled = composed(&gpu, &before, &msaa, glow, TextureFormat::Rgba8UnormSrgb);
    assert_ne!(multisampled, before);
    if let Some(directory) = std::env::var_os("CINNABAR_SELECTION_CAPTURE_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (name, pixels) in [
            ("before", &before),
            ("mask", &mask),
            ("outlined", &outlined),
            ("glowing", &glowing),
            ("hdr", &hdr),
            ("msaa4", &multisampled),
        ] {
            image::save_buffer(
                directory.join(format!("{name}.png")),
                pixels,
                SNAPSHOT_SIDE,
                SNAPSHOT_SIDE,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
