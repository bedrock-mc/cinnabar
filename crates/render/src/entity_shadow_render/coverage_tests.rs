use super::*;
use crate::scene_sampling::tests::{fixture, pixel, texture};

/// Uses production blend/stencil states to verify overlap and uncovered samples on a real GPU.
#[test]
fn overlapping_shadows_multiply_each_covered_sample_once() {
    let Some((device, queue, adapter)) = fixture() else {
        return;
    };
    let format = TextureFormat::Rgba8Unorm;
    let gpu = EntityShadowGpu::new(&device);
    for samples in [1, 2, 4, 8] {
        if [format, TextureFormat::Stencil8].into_iter().any(|format| {
            !adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(samples)
        }) {
            eprintln!("skipping shadow coverage {samples} samples: missing attachment support");
            continue;
        }
        let scene = texture(
            &device,
            format,
            samples,
            TextureUsages::RENDER_ATTACHMENT
                | if samples == 1 {
                    TextureUsages::COPY_SRC
                } else {
                    TextureUsages::empty()
                },
        );
        let scene_view = scene.create_view(&default());
        let resolved = texture(
            &device,
            format,
            1,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        );
        let resolved_view = resolved.create_view(&default());
        let stencil = texture(
            &device,
            TextureFormat::Stencil8,
            samples,
            TextureUsages::RENDER_ATTACHMENT,
        );
        let stencil_view = stencil.create_view(&default());
        let descriptor = pipeline_descriptor(
            gpu.layouts[usize::from(samples > 1)].clone(),
            format,
            samples,
        );
        let source = format!(
            "
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2(1.0), 0.0, 1.0);
}}
@fragment fn fragment(@builtin(sample_index) sample: u32) -> @location(0) vec4<f32> {{
    if {}u > 1u && sample == {}u {{ discard; }}
    return vec4(0.5, 0.5, 0.5, 1.0);
}}",
            samples,
            samples - 1
        );
        let shader = device
            .wgpu_device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shadow coverage fixture"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline =
            device
                .wgpu_device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("shadow coverage fixture"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        compilation_options: default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment"),
                        compilation_options: default(),
                        targets: &descriptor.fragment.unwrap().targets,
                    }),
                    primitive: default(),
                    depth_stencil: descriptor.depth_stencil,
                    multisample: descriptor.multisample,
                    multiview: None,
                    cache: None,
                });
        let mut context = RenderContext::new(device.clone(), None);
        {
            let mut pass =
                context
                    .command_encoder()
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("overlapping shadow fixture"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &scene_view,
                            depth_slice: None,
                            resolve_target: (samples > 1).then_some(&*resolved_view),
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::WHITE),
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                            view: &stencil_view,
                            depth_ops: None,
                            stencil_ops: Some(Operations {
                                load: LoadOp::Clear(0),
                                store: StoreOp::Discard,
                            }),
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
            pass.set_pipeline(&pipeline);
            pass.set_stencil_reference(1);
            pass.draw(0..3, 0..2);
        }
        let actual = pixel(
            &device,
            &queue,
            context,
            if samples > 1 { &resolved } else { &scene },
        );
        let expected = if samples == 1 {
            128
        } else {
            ((0.5 * (samples - 1) as f32 + 1.0) / samples as f32 * 255.0).round() as u8
        };
        for channel in &actual[..3] {
            assert!(
                channel.abs_diff(expected) <= 1,
                "{samples} samples: {actual:?} expected {expected}"
            );
        }
        assert_eq!(actual[3], 255);
    }
}
