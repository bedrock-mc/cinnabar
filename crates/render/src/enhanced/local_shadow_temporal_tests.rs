//! The production fragment retains stationary receivers through camera jitter, never actor motion.

use super::{layout, update_weight};
use crate::enhanced::{frame::EnhancedFrameGpu, local_lights::POINT_SHADOW_FACES};
use bevy::math::{Mat4, Vec2, Vec3, Vec4};
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

#[test]
fn native_local_visibility_reprojects_camera_jitter_and_rejects_receiver_changes() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU for local shadow fragment reprojection");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("local visibility temporal fragment fixture"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("local visibility fixture device");
    const SIZE: u32 = 32;
    const HALF: u32 = SIZE / 2;
    const MAP: u32 = 8;
    let texture = |format, size, layers, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("local visibility temporal fixture texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let sampled = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
    let depth = texture(wgpu::TextureFormat::Depth32Float, SIZE, 1, sampled);
    let motion = texture(wgpu::TextureFormat::Rgba16Float, SIZE, 1, sampled);
    let receiver_normal = texture(wgpu::TextureFormat::Rg16Float, SIZE, 1, sampled);
    let shadow = texture(
        wgpu::TextureFormat::Depth32Float,
        MAP,
        POINT_SHADOW_FACES as u32,
        sampled,
    );
    let output = texture(
        wgpu::TextureFormat::Rgba16Float,
        HALF,
        1,
        sampled | wgpu::TextureUsages::COPY_SRC,
    );
    let history = texture(
        wgpu::TextureFormat::Rgba16Float,
        HALF,
        1,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let metadata_output = texture(
        wgpu::TextureFormat::Rgba16Float,
        HALF,
        1,
        sampled | wgpu::TextureUsages::COPY_SRC,
    );
    let metadata_history = texture(
        wgpu::TextureFormat::Rgba16Float,
        HALF,
        1,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let sun_output = texture(wgpu::TextureFormat::Rgba16Float, HALF, 1, sampled);
    let sun_output_view = sun_output.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let motion_view = motion.create_view(&Default::default());
    let receiver_normal_view = receiver_normal.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let history_view = history.create_view(&Default::default());
    let metadata_output_view = metadata_output.create_view(&Default::default());
    let metadata_history_view = metadata_history.create_view(&Default::default());
    let shadow_view = shadow.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let shadow_faces: Vec<_> = (0..POINT_SHADOW_FACES as u32)
        .map(|face| {
            shadow.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: face,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local history current and previous camera"),
        size: std::mem::size_of::<EnhancedFrameGpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let policy = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("production local history policy"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut light_data = bytemuck::cast_slice(&[1u32, MAP, 1, 1]).to_vec();
    light_data.extend_from_slice(bytemuck::bytes_of(&[
        [0.0_f32, 0.0, 0.0, 32.0],
        [1.0, 1.0, 1.0, 1.0],
        [0.0, 0.05, 0.0, 0.0],
    ]));
    let axes = [
        (Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::NEG_Z),
        (Vec3::Z, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_Y),
    ];
    assert_eq!(axes.len(), POINT_SHADOW_FACES);
    for (direction, up) in axes {
        let clip = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.05, 32.0)
            * Mat4::look_to_rh(Vec3::ZERO, direction, up);
        light_data.extend_from_slice(bytemuck::bytes_of(&clip));
    }
    let lights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("one shadowed fixture lamp"),
        contents: &light_data,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let linear = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let comparison = device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let descriptor = layout();
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(descriptor.label.as_ref()),
        entries: &descriptor.entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("production local visibility fragment layout"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("local visibility fragment fixture binding"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&history_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&linear),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: lights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: policy.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&motion_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Sampler(&comparison),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&metadata_history_view),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&history_view),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(&shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: policy.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::TextureView(&receiver_normal_view),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production local shadow history fragment"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(include_str!("local_shadow_history.wgsl"), &[]).into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("production local shadow reprojection"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("resolve_local_shadows"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    let sample_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("local shadow output samples"),
        source: wgpu::ShaderSource::Wgsl(r#"
@group(0) @binding(0) var output:texture_2d<f32>;
@group(0) @binding(1) var<storage,read_write> results:array<vec4<f32>,64>;
@group(0) @binding(2) var<uniform> destination:vec4<u32>;
@group(0) @binding(3) var metadata:texture_2d<f32>;
@compute @workgroup_size(1) fn read_visibility(){let pixel=vec2<i32>(destination.yz);let visibility=textureLoad(output,pixel,0);let history=textureLoad(metadata,pixel,0);results[destination.x]=vec4(visibility.xy,history.xy);}
"#.into()),
    });
    let sample_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("local shadow output readback"),
        layout: None,
        module: &sample_shader,
        entry_point: Some("read_visibility"),
        compilation_options: Default::default(),
        cache: None,
    });
    let samples = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local shadow regression samples"),
        size: 64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local shadow regression readback"),
        size: samples.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let destination = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local shadow sampled pixel"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sample_binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("local shadow sample fixture binding"),
        layout: &sample_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: samples.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: destination.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&metadata_output_view),
            },
        ],
    });
    let camera =
        |current_x: f32, previous_x: f32, jitter: f32, previous_jitter: f32, distance: f32| {
            let projection = Mat4::perspective_infinite_reverse_rh(60.0_f32.to_radians(), 1.0, 0.1);
            let clip = |x: f32, jitter: f32| {
                Mat4::from_translation(Vec3::new(jitter * 2.0 / SIZE as f32, 0.0, 0.0))
                    * projection
                    * Mat4::from_translation(Vec3::new(-x, 0.0, 0.0))
            };
            let mut frame = EnhancedFrameGpu::zeroed();
            frame.clip_from_world = clip(current_x, jitter);
            frame.world_from_clip = frame.clip_from_world.inverse();
            frame.previous_clip_from_world = clip(previous_x, previous_jitter);
            frame.camera_time = Vec4::new(current_x, 0.0, 0.0, 0.0);
            frame.viewport = Vec4::new(
                SIZE as f32,
                SIZE as f32,
                1.0 / SIZE as f32,
                1.0 / SIZE as f32,
            );
            frame.projection.x = 0.1;
            let h = frame.world_from_clip * Vec4::new(0.0, 0.0, 0.1 / distance, 1.0);
            let previous = frame.previous_clip_from_world * (h / h.w);
            let motion = Vec2::new(previous.x, -previous.y) / previous.w * 0.5;
            (frame, Vec4::new(motion.x, motion.y, distance, 1.0))
        };
    let weight = update_weight(1.0 / 60.0);
    let draw = |index: u32,
                frame: EnhancedFrameGpu,
                motion: Vec4,
                lit: bool,
                valid: bool,
                distance: f32,
                x: u32| {
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&frame));
        queue.write_buffer(
            &policy,
            0,
            bytemuck::bytes_of(&[f32::from(u8::from(valid)), weight, 0.0, 1.0]),
        );
        queue.write_buffer(
            &destination,
            0,
            bytemuck::bytes_of(&[index, x, HALF / 2, 0]),
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &receiver_normal_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &motion_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(motion.x),
                            g: f64::from(motion.y),
                            b: f64::from(motion.z),
                            a: f64::from(motion.w),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.projection.x / distance),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        for face in &shadow_faces {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: face,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(f32::from(u8::from(lit))),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &output_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &metadata_output_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &sun_output_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_texture(
            output.as_image_copy(),
            history.as_image_copy(),
            output.size(),
        );
        encoder.copy_texture_to_texture(
            metadata_output.as_image_copy(),
            metadata_history.as_image_copy(),
            metadata_output.size(),
        );
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&sample_pipeline);
            pass.set_bind_group(0, &sample_binding, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
    };
    let (frame, motion) = camera(0.0, 0.0, 0.0, 0.0, 8.0);
    draw(0, frame, motion, true, false, 8.0, HALF / 2);
    let mut expected = vec![1.0];
    let mut current = 1.0;
    let mut old_x = 0.0;
    let mut old_jitter = 0.0;
    for step in 1..=24 {
        let x = (step % 3) as f32 * 0.05;
        let jitter = if step % 2 == 0 { -0.25 } else { 0.25 };
        let (frame, motion) = camera(x, old_x, jitter, old_jitter, 8.0);
        let lit = step % 2 == 0;
        current += (f32::from(u8::from(lit)) - current) * weight;
        draw(step, frame, motion, lit, true, 8.0, HALF / 2);
        expected.push(current);
        old_x = x;
        old_jitter = jitter;
    }
    let (frame, motion) = camera(0.0, 0.0, 0.0, 0.0, 8.0);
    draw(25, frame, motion, true, false, 8.0, HALF / 2);
    draw(
        26,
        frame,
        motion + Vec4::new(0.5 / SIZE as f32, 0.0, 0.0, 0.0),
        false,
        true,
        8.0,
        HALF / 2,
    );
    draw(27, frame, motion, true, false, 8.0, HALF / 2);
    draw(
        28,
        frame,
        Vec4::new(0.0, 0.0, 0.0, -1.0),
        false,
        true,
        8.0,
        HALF / 2,
    );
    draw(29, frame, motion, true, false, 8.0, HALF / 2);
    let (far_frame, far_motion) = camera(0.0, 0.0, 0.0, 0.0, 12.0);
    draw(30, far_frame, far_motion, false, true, 12.0, HALF / 2);
    let mut edge_history = vec![[0x3c00_u16, 0x3c00, 0x3c00, 0x4800]; (HALF * HALF) as usize];
    for row in edge_history.chunks_exact_mut(HALF as usize) {
        for texel in &mut row[..HALF as usize / 2] {
            texel[3] = 0x4400;
        }
    }
    queue.write_texture(
        history.as_image_copy(),
        bytemuck::cast_slice(&edge_history),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(HALF * 8),
            rows_per_image: Some(HALF),
        },
        history.size(),
    );
    let mut edge_metadata = vec![[0x3c00_u16, 0x4800, 0, 0]; (HALF * HALF) as usize];
    for row in edge_metadata.chunks_exact_mut(HALF as usize) {
        for texel in &mut row[..HALF as usize / 2] {
            texel[1] = 0x4400;
        }
    }
    queue.write_texture(
        metadata_history.as_image_copy(),
        bytemuck::cast_slice(&edge_metadata),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(HALF * 8),
            rows_per_image: Some(HALF),
        },
        metadata_history.size(),
    );
    let (edge_frame, edge_motion) = camera(0.0, 0.0, 0.0, 0.5, 8.0);
    draw(31, edge_frame, edge_motion, false, true, 8.0, HALF / 2 - 1);
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&samples, 0, &readback, 0, samples.size());
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("local shadow fragment readback");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("local shadow fragment completion");
    let values: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    for (index, expected) in expected.into_iter().enumerate() {
        assert!(
            (values[index][0] - expected).abs() < 0.003,
            "static receivers retain history through camera motion and alternating TAA jitter: {index}: {:?} vs {expected}",
            values[index]
        );
        assert!((values[index][3] - 8.0).abs() < 0.01);
    }
    for index in [26, 28, 30] {
        assert!(
            values[index][0] < 0.01,
            "real receiver motion, invalid motion, and disocclusion reject stale visibility: {index}: {:?}",
            values[index]
        );
    }
    assert!(
        (values[31][0] - (1.0 - weight) * 0.5).abs() < 0.002,
        "bilinear depth-edge taps reject foreground history individually: {:?}",
        values[31]
    );
}
