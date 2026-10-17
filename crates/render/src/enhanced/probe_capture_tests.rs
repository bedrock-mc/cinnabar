//! Native capture composition keeps blended layers over sky and opaque backgrounds.

use super::frame::EnhancedFrameGpu;
use bevy::math::{Mat4, Vec4};
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

/// Renders the production sky and capture shaders around a layer that cannot write depth.
#[test]
fn capture_keeps_depthless_transparency_after_the_sky_background() {
    let instance = super::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU for reflection capture transparency");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("reflection capture fixture device");
    let size = wgpu::Extent3d {
        width: 3,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |format, size, dimension| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture composition fixture"),
            size,
            dimension,
            format,
            mip_level_count: 1,
            sample_count: 1,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let scene = texture(
        wgpu::TextureFormat::Rgba16Float,
        size,
        wgpu::TextureDimension::D2,
    );
    let captured = texture(
        wgpu::TextureFormat::Rgba16Float,
        size,
        wgpu::TextureDimension::D2,
    );
    let depth = texture(
        wgpu::TextureFormat::Depth32Float,
        size,
        wgpu::TextureDimension::D2,
    );
    let sky = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        wgpu::TextureDimension::D2,
    );
    queue.write_texture(
        sky.as_image_copy(),
        &[64, 128, 192, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        sky.size(),
    );
    let noise = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("unused capture atmosphere volume"),
        size: sky.size(),
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        mip_level_count: 1,
        sample_count: 1,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let scene_view = scene.create_view(&Default::default());
    let capture_view = captured.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let sky_view = sky.create_view(&Default::default());
    let noise_view = noise.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let mut frame = EnhancedFrameGpu::zeroed();
    frame.world_from_clip = Mat4::IDENTITY;
    frame.atmosphere.x = 1.0;
    frame.celestial.y = 1.0;
    frame.viewport = Vec4::new(3.0, 1.0, 1.0 / 3.0, 1.0);
    let frame = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Overworld capture frame"),
        contents: bytemuck::bytes_of(&frame),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let descriptor = super::probes::layout();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("production capture bindings"),
        entries: &descriptor.entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let group = |source: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&sky_view),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::TextureView(&noise_view),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 22,
                    resource: wgpu::BindingResource::TextureView(&noise_view),
                },
                wgpu::BindGroupEntry {
                    binding: 23,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    };
    let sky_group = group(&sky_view);
    let capture_group = group(&scene_view);
    let production = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production capture sky and copy"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(include_str!("probe.wesl"), &[]).into(),
        ),
    });
    let fixture = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("opaque and depthless blended geometry"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(
                r#"
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
struct OpaqueOutput { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 }
@fragment fn opaque(in:FullscreenVertexOutput)->OpaqueOutput {
    return OpaqueOutput(vec4(0.2,0.4,0.6,1.0),select(0.0,0.75,in.position.x>=2.0));
}
@fragment fn actor(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    if(in.position.x<1.0){discard;}
    return vec4(0.8,0.1,0.05,0.25);
}
"#,
                &[],
            )
            .into(),
        ),
    });

    let pipeline =
        |shader: &wgpu::ShaderModule, fragment, layout, blend, depth_write: Option<bool>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout,
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                depth_stencil: depth_write.map(|write| wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                primitive: Default::default(),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
    let opaque = pipeline(&fixture, "opaque", None, None, Some(true));
    let actor = pipeline(
        &fixture,
        "actor",
        None,
        Some(wgpu::BlendState::ALPHA_BLENDING),
        Some(false),
    );
    let background = pipeline(
        &production,
        "capture_sky",
        Some(&pipeline_layout),
        None,
        None,
    );
    let copy = pipeline(
        &production,
        "capture_probe",
        Some(&pipeline_layout),
        None,
        None,
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    for (pipeline, group, target, use_depth, clear) in [
        (&opaque, None, &scene_view, true, true),
        (&background, Some(&sky_group), &scene_view, false, false),
        (&actor, None, &scene_view, true, false),
        (&copy, Some(&capture_group), &capture_view, false, true),
    ] {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("capture composition regression"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: use_depth.then_some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(0.0)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        if let Some(group) = group {
            pass.set_bind_group(0, group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
    let read_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("capture colour and retained depth samples"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@group(0) @binding(0) var captured:texture_2d<f32>;
@group(0) @binding(1) var depth:texture_depth_2d;
@group(0) @binding(2) var<storage,read_write> results:array<vec4<f32>,6>;
@compute @workgroup_size(1) fn read_capture() {
    for(var x=0u;x<3u;x+=1u){
        results[x]=textureLoad(captured,vec2(i32(x),0),0);
        results[x+3u]=vec4(textureLoad(depth,vec2(i32(x),0),0));
    }
}
"#
            .into(),
        ),
    });
    let read_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &read_shader,
        entry_point: Some("read_capture"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 6 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let read_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &read_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&capture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: output.as_entire_binding(),
            },
        ],
    });
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&read_pipeline);
        pass.set_bind_group(0, &read_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("capture readback")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("capture fixture completion");
    let samples: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range().unwrap()).to_vec();
    let sky_rgb = [64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0];
    let actor_rgb = [0.8, 0.1, 0.05];
    for (pixel, expected) in [
        sky_rgb,
        sky_rgb.map(|v| v * 0.75),
        [0.2 * 0.75, 0.4 * 0.75, 0.6 * 0.75],
    ]
    .into_iter()
    .enumerate()
    {
        for channel in 0..3 {
            let expected = expected[channel]
                + if pixel == 0 {
                    0.0
                } else {
                    actor_rgb[channel] * 0.25
                };
            assert!(
                (samples[pixel][channel] - expected).abs() < 0.001,
                "capture pixel {pixel} channel {channel} lost its composition: {:?}",
                samples[pixel]
            );
        }
    }
    assert_eq!(samples[3][0], 0.0);
    assert_eq!(
        samples[4][0], 0.0,
        "blended actor must leave sky depth unchanged"
    );
    assert_eq!(samples[5][0], 0.75);
}
