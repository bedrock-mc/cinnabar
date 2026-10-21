//! Native pixel checks for the gamma-space layer's identity and blending contracts.

use crate::gpu_snapshot::Gpu;
use wgpu::util::DeviceExt;

/// Draws the production composite into float storage so color loss cannot hide in quantization.
fn composite(ui: &[[u8; 4]], scene: &[[f32; 4]]) -> Option<Vec<[f32; 4]>> {
    composite_entry(ui, scene, "composite_fragment")
}

/// Draws one production composite entry into float storage for exact colour and alpha checks.
fn composite_entry(ui: &[[u8; 4]], scene: &[[f32; 4]], entry: &str) -> Option<Vec<[f32; 4]>> {
    assert_eq!(ui.len(), scene.len());
    let gpu = Gpu::for_fixture("gamma-space UI composite pixels")?;
    let size = wgpu::Extent3d {
        width: ui.len() as u32,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |format, bytes: &[u8]| {
        gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytes,
        )
    };
    let ui = texture(super::UI_LAYER_FORMAT, bytemuck::cast_slice(ui));
    let scene = texture(
        wgpu::TextureFormat::Rgba32Float,
        bytemuck::cast_slice(scene),
    );
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let entries = [0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    });
    let bindings = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
    let layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bindings)],
            immediate_size: 0,
        });
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("../ui_composite.wgsl").into()),
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("composite_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba32Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
    let ui = ui.create_view(&Default::default());
    let scene = scene.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bindings,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&ui),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&scene),
            },
        ],
    });
    let row_bytes = (size.width * 16).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row_bytes),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: None,
            },
        },
        size,
    );
    gpu.queue.submit([encoder.finish()]);
    let (sent, received) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sent.send(result).unwrap()
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    received.recv().unwrap().unwrap();
    let mapped = readback.slice(..).get_mapped_range().unwrap();
    Some(bytemuck::cast_slice::<u8, [f32; 4]>(&mapped)[..size.width as usize].to_vec())
}

#[test]
fn empty_ui_preserves_in_range_scene_color_and_alpha_exactly() {
    let scene = [[0.125, 0.5, 0.875, 0.25], [0.0, 1.0, 0.25, 0.75]];
    let Some(pixels) = composite(&[[0; 4]; 2], &scene) else {
        return;
    };
    assert_eq!(pixels, scene);
}

#[test]
fn opaque_and_translucent_ui_keep_premultiplied_gamma_blending() {
    let ui = [[64, 128, 192, 255], [32, 64, 96, 128], [24, 12, 6, 0]];
    let scene = [[0.18, 0.4, 0.8, 0.5]; 3];
    let Some(pixels) = composite(&ui, &scene) else {
        return;
    };
    for (index, actual) in pixels.iter().enumerate() {
        let alpha = f32::from(ui[index][3]) / 255.0;
        for channel in 0..4 {
            let expected = if channel == 3 {
                alpha + scene[index][3] * (1.0 - alpha)
            } else {
                let linear = scene[index][channel];
                let under = if linear <= 0.003_130_8 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                let gamma = f32::from(ui[index][channel]) / 255.0 + under * (1.0 - alpha);
                if gamma <= 0.04045 {
                    gamma / 12.92
                } else {
                    ((gamma + 0.055) / 1.055).powf(2.4)
                }
            };
            assert!(
                (actual[channel] - expected).abs() < 0.000_01,
                "pixel {index} channel {channel}: {actual:?} expected {expected}"
            );
        }
    }
}

#[test]
fn empty_and_translucent_pixels_share_the_scene_clamp() {
    let scene = [[2.0, -0.5, 0.25, 0.7]; 2];
    let Some(pixels) = composite(&[[0; 4], [0, 0, 0, 1]], &scene) else {
        return;
    };
    assert_eq!(pixels[0], [1.0, 0.0, 0.25, 0.7]);
    assert!(pixels[1][0] <= 1.0);
    assert!(pixels[1][1].abs() < 0.00001);
    assert!((pixels[1][0] - pixels[0][0]).abs() < 0.01);
}

#[test]
fn empty_nametag_layer_preserves_hdr_pixels_and_scene_alpha() {
    let scene = [[2.0, 0.5, 4.0, 0.25], [-0.25, 1.5, 0.25, 0.75]];
    let Some(pixels) = composite_entry(&[[0; 4]; 2], &scene, "nametag_composite_fragment") else {
        return;
    };
    assert_eq!(pixels, scene);
}
