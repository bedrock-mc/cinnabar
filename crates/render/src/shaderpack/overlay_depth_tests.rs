//! Checks the pack-to-native projection boundary with an actual depth attachment.

use crate::gpu_snapshot::Gpu;
use wgpu::util::DeviceExt;

#[test]
fn nametag_depth_restores_pack_projection_and_flips_only_the_viewport() {
    let Some(gpu) = Gpu::for_fixture("shader-pack nametag overlay depth") else {
        return;
    };
    let depth = |side, usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay depth fixture"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage,
            view_formats: &[],
        })
    };
    let pack = depth(
        2,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let native = depth(
        4,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let pack_view = pack.create_view(&Default::default());
    let native_view = native.create_view(&Default::default());
    let depths = [0.75_f32, 1.0, 0.0, 0.875];
    let fixture = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("known pack depths"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @builtin(frag_depth) f32 {
    return array<f32, 4>(DEPTH_VALUES)[u32(p.y) * 2u + u32(p.x)];
}
"#
                .replace(
                    "DEPTH_VALUES",
                    &depths.map(|depth| depth.to_string()).join(", "),
                )
                .into(),
            ),
        });
    let fill = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("known pack depths"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &fixture,
                entry_point: Some("vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fixture,
                entry_point: Some("fragment"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
    let host = super::super::host_pipelines::host_pipelines(
        &gpu.device,
        &[None; super::super::plan::MAX_COLOR_BUFFERS as usize],
        wgpu::TextureFormat::R32Float,
        false,
        [false; 3],
        &[],
        &[],
    );
    let scene_near = 0.1;
    let pack_near = 0.05;
    let far = 100.0;
    let parameters: super::super::passes::SceneDepthParameters =
        [scene_near, pack_near, far, 0., 1., 1., 2., 2.];
    let uniform = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay depth planes"),
            contents: bytemuck::bytes_of(&parameters),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay depth fixture inputs"),
        layout: &host.overlay_depth.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&pack_view),
            },
        ],
    });
    let attachment = |view, clear| wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(clear),
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    };
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("known pack depths"),
            color_attachments: &[],
            depth_stencil_attachment: Some(attachment(&pack_view, 1.)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&fill);
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("native overlay depth fixture"),
            color_attachments: &[],
            depth_stencil_attachment: Some(attachment(&native_view, 0.125)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(1., 1., 2., 2., 0., 1.);
        pass.set_pipeline(&host.overlay_depth);
        pass.set_bind_group(0, &binding, &[]);
        pass.draw(0..3, 0..1);
    }
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native overlay depth readback"),
        size: 256 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &native,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(4),
            },
        },
        native.size(),
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
    let words: &[f32] = bytemuck::cast_slice(&mapped);
    let pack_inverse =
        glam::camera::rh::proj::directx::perspective(1.0, 1.0, pack_near, far).inverse();
    let native_projection =
        glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, scene_near);
    for y in 0..4 {
        for x in 0..4 {
            let expected = if (1..3).contains(&x) && (1..3).contains(&y) {
                let forward = depths[(2 - y) * 2 + x - 1];
                if forward == 1.0 {
                    0.0
                } else {
                    let view = pack_inverse * glam::Vec4::new(0., 0., forward, 1.);
                    let clip = native_projection * (view / view.w);
                    (clip.z / clip.w).clamp(0.0, 1.0)
                }
            } else {
                0.125
            };
            assert!(
                (words[y * 64 + x] - expected).abs() < 1e-5,
                "pixel {x},{y}: {} expected {expected}",
                words[y * 64 + x]
            );
        }
    }
}
