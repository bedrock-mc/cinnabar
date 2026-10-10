//! Native coverage regression for the independent Enhanced visibility depth.

use bevy::render::{renderer::RenderDevice, texture::CachedTexture, view::ViewDepthTexture};

use super::{depth::camera_depth_attachment, targets::SceneTargets};

const COVERAGE: &str = r#"
fn position(index: u32, depth: f32) -> vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], depth, 1.0);
}
@vertex fn near(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return position(index, 0.8);
}
@vertex fn farther(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return position(index, 0.5);
}
@fragment fn depth_only() {}
@fragment fn green() -> @location(0) vec4<f32> { return vec4(0.0, 1.0, 0.0, 1.0); }
"#;

const READ_DEPTH: &str = r#"
@group(0) @binding(0) var scene_depth: texture_depth_2d;
@group(0) @binding(1) var main_depth: texture_depth_2d;
@group(0) @binding(2) var<storage, read_write> result: vec2<f32>;
@compute @workgroup_size(1) fn read_depth() {
    result = vec2(textureLoad(scene_depth, vec2<i32>(0), 0),
                  textureLoad(main_depth, vec2<i32>(0), 0));
}
"#;

#[test]
fn native_camera_visibility_depth_cannot_reject_main_alpha_coverage() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for Enhanced camera depth isolation");
        return;
    };
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("Enhanced camera depth fixture device");
    let render_device = RenderDevice::from(device);
    let device = render_device.wgpu_device();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camera depth differing alpha coverage fixture"),
        source: wgpu::ShaderSource::Wgsl(COVERAGE.into()),
    });
    let make_pipeline = |vertex, fragment, targets: &[Option<wgpu::ColorTargetState>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("camera depth coverage fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vertex),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets,
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        })
    };
    let prepass_pipeline = make_pipeline("near", "depth_only", &[]);
    let main_pipeline = make_pipeline(
        "farther",
        "green",
        &[Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba8Unorm,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })],
    );
    let depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camera isolation stored depth readback"),
        source: wgpu::ShaderSource::Wgsl(READ_DEPTH.into()),
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("camera isolation stored depth fixture"),
        layout: None,
        module: &depth_shader,
        entry_point: Some("read_depth"),
        compilation_options: Default::default(),
        cache: None,
    });

    for isolated in [true, false] {
        let scene = SceneTargets::new(&render_device, [1, 1], wgpu::TextureFormat::Rgba8Unorm);
        let main_texture = render_device.create_texture(&wgpu::TextureDescriptor {
            label: Some("independent main ViewDepth fixture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let main_depth = ViewDepthTexture::new(
            CachedTexture {
                default_view: main_texture.create_view(&Default::default()),
                texture: main_texture,
            },
            Some(0.0),
        );
        let colour = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("camera depth visible pixel fixture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let colour_view = colour.create_view(&Default::default());
        let depth_values = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera isolation GPU depth values"),
            size: 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera isolation pixel and depths readback"),
            size: 264,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera isolation sample independent depth targets"),
            layout: &compute.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene.depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(main_depth.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: depth_values.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        // The shared target case verifies sensitivity to the original contamination.
        if !isolated {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                depth_stencil_attachment: Some(camera_depth_attachment(&scene.depth_view)),
                ..Default::default()
            });
        }
        {
            let attachment = if isolated {
                camera_depth_attachment(&scene.depth_view)
            } else {
                main_depth.get_attachment(wgpu::StoreOp::Store)
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("prepass opaque coverage differs from main authored alpha"),
                depth_stencil_attachment: Some(attachment),
                ..Default::default()
            });
            pass.set_pipeline(&prepass_pipeline);
            pass.draw(0..3, 0..1);
        }
        {
            let attachment = main_depth.get_attachment(wgpu::StoreOp::Store);
            assert_eq!(
                attachment.depth_ops.as_ref().unwrap().load,
                if isolated {
                    wgpu::LoadOp::Clear(0.0)
                } else {
                    wgpu::LoadOp::Load
                },
                "isolated coverage must preserve the main pass's first clear",
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visible farther surface after main alpha clips near surface"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(attachment),
                ..Default::default()
            });
            pass.set_pipeline(&main_pipeline);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&compute);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_texture_to_buffer(
            colour.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        encoder.copy_buffer_to_buffer(&depth_values, 0, &readback, 256, 8);
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        let values: &[f32] = bytemuck::cast_slice(&bytes[256..264]);
        if isolated {
            assert_eq!(
                &bytes[..4],
                &[0, 255, 0, 255],
                "main surface must remain visible"
            );
            assert!(
                (values[0] - 0.8).abs() < 0.00001,
                "prepass must store its near coverage"
            );
            assert!(
                (values[1] - 0.5).abs() < 0.00001,
                "main must own its independent coverage"
            );
        } else {
            assert_eq!(
                &bytes[..4],
                &[255, 0, 0, 255],
                "shared prepass depth must reject the surface"
            );
            assert!((values[1] - 0.8).abs() < 0.00001);
        }
    }
}
