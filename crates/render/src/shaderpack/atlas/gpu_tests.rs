use super::animate::{
    AtlasAnimations, BLEND_INSTANCE_ATTRIBUTES, BLEND_SOURCE_ENTRIES, BlendInstance, BlendPipelines,
};
use super::labpbr::labpbr_normal;
use super::pixels::{MipImages, build_images};
use super::plan::{AuthoredPbr, plan_atlas};
use super::tests::{FRAME_TICKS, expected, fixture, page, slot_rect};
use super::upload::{COLOR_STORAGE_FORMAT, PBR_FORMAT, upload_atlas};
use super::*;
use assets::texture::{PBR_REF_LABPBR, PBR_REF_NORMAL};

/// Builds the production blend shader and attachment layouts for GPU readback checks.
fn pipelines(
    device: &wgpu::Device,
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::BindGroupLayout,
) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &BLEND_SOURCE_ENTRIES,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shader-pack frame blend"),
        source: wgpu::ShaderSource::Wgsl(include_str!("blend.wgsl").into()),
    });
    let pipeline = |entry_point: &str, formats: &[wgpu::TextureFormat]| {
        let targets = formats
            .iter()
            .map(|&format| Some(format.into()))
            .collect::<Vec<_>>();
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<BlendInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &BLEND_INSTANCE_ATTRIBUTES,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let colour = pipeline("fragment_colour", &[COLOR_STORAGE_FORMAT]);
    let layers = pipeline(
        "fragment_layers",
        &[COLOR_STORAGE_FORMAT, PBR_FORMAT, PBR_FORMAT],
    );
    (colour, layers, layout)
}

/// Reads one level of a colour atlas back as encoded bytes.
fn read_level(gpu: &crate::gpu_snapshot::Gpu, texture: &wgpu::Texture, level: u32) -> MipImages {
    let (width, height) = (texture.width() >> level, texture.height() >> level);
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let pixels = mapped
        .chunks_exact(row as usize)
        .flat_map(|line| &line[..width as usize * 4])
        .copied()
        .collect();
    let mut levels = vec![Vec::new(); level as usize];
    levels.push(pixels);
    MipImages {
        width: texture.width(),
        height: texture.height(),
        levels,
    }
}

/// The LabPBR frame mix: linear channels.
fn blended_linear(current: &[u8], next: &[u8], weight: f32) -> Vec<u8> {
    current
        .iter()
        .zip(next)
        .map(|(&a, &b)| (f32::from(a) * (1.0 - weight) + f32::from(b) * weight).round() as u8)
        .collect()
}

/// Blends with the colour-only pipeline when the frame store has no LabPBR frames, and with the
/// colour + normal + specular pipeline (one pass per mip) when the animation frames author normals.
#[test]
fn animated_display_sprites_show_the_clock_frame_after_an_update() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("shader-pack atlas animation update")
    else {
        return;
    };
    let (colour, layers, layout) = pipelines(&gpu.device);
    let pipelines = BlendPipelines {
        colour: &colour,
        layers: &layers,
    };
    let fixture = fixture();
    let normals = page(7, 16, 6);
    let flags = PBR_REF_NORMAL | PBR_REF_LABPBR;
    let mut refs = vec![u32::MAX; STATIC_SPRITE_SLOTS as usize];
    for layer in 3..=5 {
        refs[(static_sprite_slot(0, layer) - ATLAS_HEADER_SLOTS) as usize] = flags | layer;
    }
    for authored in [false, true] {
        let pbr = authored.then(|| AuthoredPbr {
            normal_pages: [&normals, &normals],
            mer_pages: [&normals, &normals],
            refs: &refs,
        });
        let (plan, images) = fixture.with_inputs(pbr, |inputs| {
            let plan = plan_atlas(inputs, gpu.device.limits().max_texture_dimension_2d).unwrap();
            let images = build_images(inputs, &plan);
            (plan, images)
        });
        let uploaded = upload_atlas(&gpu.device, &gpu.queue, &plan, &images);
        let mut animations = AtlasAnimations::new(
            &uploaded,
            &fixture.animations,
            &fixture.frames,
            plan.animations.clone(),
            plan.mip_levels,
            [plan.atlas.width, plan.atlas.height],
        );
        animations.bind_blend_sources(&gpu.device, &layout);
        let clock = ChunkAnimationClock::from_parts(FRAME_TICKS + 1, 0.5);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        assert_eq!(
            animations.encode(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                clock,
                Some(pipelines)
            ),
            2
        );
        gpu.queue.submit([encoder.finish()]);
        let plain = slot_rect(&plan, animation_sprite_slot(0));
        let blending = slot_rect(&plan, animation_sprite_slot(1));
        for level in 0..plan.mip_levels {
            let atlas = read_level(&gpu, &uploaded.atlas.color, level);
            let side = plain.side >> level;
            assert_eq!(
                atlas.read(level, plain),
                expected(0, 4, level, side),
                "authored {authored}, mip {level}"
            );
            let wanted = blended_linear(
                &expected(0, 4, level, side),
                &expected(0, 5, level, side),
                0.75,
            );
            for (got, want) in atlas.read(level, blending).iter().zip(&wanted) {
                assert!(
                    got.abs_diff(*want) <= 1,
                    "authored {authored}, mip {level}: {got} vs {want}"
                );
            }
            if !authored {
                continue;
            }
            let normal = |layer: u32| {
                expected(7, layer, level, side)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|&texel| labpbr_normal(texel, flags | layer))
                    .collect::<Vec<_>>()
            };
            let atlas = read_level(&gpu, &uploaded.atlas.pbr.as_ref().unwrap()[0], level);
            assert_eq!(atlas.read(level, plain), normal(4), "normal mip {level}");
            let wanted = blended_linear(&normal(4), &normal(5), 0.75);
            for (got, want) in atlas.read(level, blending).iter().zip(&wanted) {
                assert!(
                    got.abs_diff(*want) <= 1,
                    "normal mip {level}: {got} vs {want}"
                );
            }
        }
    }
}

#[test]
fn blends_preserve_encoded_colour_and_decode_no_emission_codes() {
    let Some(gpu) =
        crate::gpu_snapshot::Gpu::for_fixture("shader-pack encoded colour and emission blending")
    else {
        return;
    };
    let (colour, layers, layout) = pipelines(&gpu.device);
    let fixture = fixture();
    let plan = fixture.with_inputs(None, |inputs| plan_atlas(inputs, 4096).unwrap());
    let display = plan.animations[1].as_ref().unwrap();
    for (a, b, wanted) in [(0, 255, 0), (255, 128, 64), (128, 255, 64), (64, 192, 128)] {
        let mut images = fixture.with_inputs(None, |inputs| build_images(inputs, &plan));
        let frames = images.frames.as_mut().unwrap();
        frames.pbr = Some([
            MipImages::filled(
                frames.color.width,
                frames.color.height,
                plan.mip_levels,
                [128, 128, 255, 255],
            ),
            MipImages::filled(
                frames.color.width,
                frames.color.height,
                plan.mip_levels,
                [0; 4],
            ),
        ]);
        for level in 0..plan.mip_levels {
            for (reference, colour, emission) in [
                (tests::reference(0, 3), 0, a),
                (tests::reference(0, 4), 255, b),
            ] {
                let rect = display.frame(reference).unwrap();
                let count = (rect.side >> level).pow(2) as usize;
                frames
                    .color
                    .blit(level, rect, &[colour, colour, colour, 255].repeat(count));
                frames.pbr.as_mut().unwrap()[1].blit(
                    level,
                    rect,
                    &[0, 0, 0, emission].repeat(count),
                );
            }
        }
        let uploaded = upload_atlas(&gpu.device, &gpu.queue, &plan, &images);
        let mut animations = AtlasAnimations::new(
            &uploaded,
            &fixture.animations,
            &fixture.frames,
            plan.animations.clone(),
            plan.mip_levels,
            [plan.atlas.width, plan.atlas.height],
        );
        animations.bind_blend_sources(&gpu.device, &layout);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        animations.encode(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            ChunkAnimationClock::from_parts(1, 0.0),
            Some(BlendPipelines {
                colour: &colour,
                layers: &layers,
            }),
        );
        gpu.queue.submit([encoder.finish()]);
        for level in 0..plan.mip_levels {
            let colour =
                read_level(&gpu, &uploaded.atlas.color, level).read(level, display.display);
            assert!(
                colour
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|texel| texel[0].abs_diff(128) <= 1)
            );
            let specular = read_level(&gpu, &uploaded.atlas.pbr.as_ref().unwrap()[1], level)
                .read(level, display.display);
            assert!(
                specular
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|texel| texel[3].abs_diff(wanted) <= 1),
                "{a} to {b} at mip {level}"
            );
        }
    }
}
