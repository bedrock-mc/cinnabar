use std::{
    borrow::Cow,
    future::Future,
    task::{Context, Poll, Waker},
};
use wgpu::util::DeviceExt;

/// Polls the adapter/device futures without adding another executor dependency.
fn finish<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn positional_material_gpu_matches_signed_coordinate_vectors() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = finish(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("this fixture requires a native GPU adapter");
    assert_ne!(
        adapter.get_info().backend,
        wgpu::Backend::Noop,
        "native GPU required"
    );
    eprintln!("positional material GPU fixture: {:?}", adapter.get_info());
    let (device, queue) =
        finish(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let source =
        include_str!("../src/material.wgsl").replace("#define_import_path cinnabar::material", "");
    let source = format!(
        "{source}\n{}",
        r#"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(p[i],0.0,1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    var position = vec3<i32>(i32(p.x)-128, 0, i32(p.y)-128);
    if (p.y < 16.0) {
        let fixtures = array(vec3(0,0,0), vec3(-1,0,-1), vec3(16,64,-17), vec3(123456,-64,98765));
        position = fixtures[min(u32(p.x)/64u, 3u)];
    }
    let selected = positional_material(0u, position);
    return select(vec4(1.0,0.0,0.0,1.0), vec4(0.0,1.0,0.0,1.0), selected.texture == 2u);
}
"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("positional materials"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    let words: [u32; 18] = [
        1,
        0,
        u32::MAX,
        1,
        2,
        0,
        1,
        0,
        u32::MAX,
        0,
        0,
        0.25_f32.to_bits(),
        2,
        0,
        u32::MAX,
        0,
        0,
        0.75_f32.to_bits(),
    ];
    let materials = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&words),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 3,
            resource: materials.as_entire_binding(),
        }],
    });
    let size = wgpu::Extent3d {
        width: 256,
        height: 256,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * 256 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for before in [true, false] {
        queue.write_buffer(
            &materials,
            16,
            bytemuck::bytes_of(&if before { 0u32 } else { 2u32 }),
        );
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1024),
                    rows_per_image: Some(256),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range();
        let expected = if before {
            [false; 4]
        } else {
            [false, true, true, true]
        };
        for (column, green) in expected.into_iter().enumerate() {
            assert_eq!(
                pixels[(8 * 256 + column * 64 + 32) * 4 + 1],
                if green { 255 } else { 0 }
            );
        }
        if let Some(directory) = std::env::var_os("CINNABAR_VARIATION_SNAPSHOT_DIR") {
            let name = if before {
                "variations-before.png"
            } else {
                "variations-after.png"
            };
            image::save_buffer(
                std::path::PathBuf::from(directory).join(name),
                &pixels,
                256,
                256,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        drop(pixels);
        readback.unmap();
    }
}
