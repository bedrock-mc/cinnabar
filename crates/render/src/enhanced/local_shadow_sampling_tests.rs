//! Forward shading admits half-resolution shadow history only for its full-resolution receiver.

use crate::enhanced::{frame::EnhancedFrameGpu, local_lights::POINT_SHADOW_FACES};
use bevy::math::{Mat4, Vec2, Vec3, Vec4};
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

const SAMPLE: &str = r#"
#import cinnabar::enhanced_view::smooth_local_visibility
#import cinnabar::enhanced_local_lights::local_light_visibility
struct Query { world_point:vec4<f32>, pixel_index:vec4<f32>, }
@group(0) @binding(0) var<uniform> query:Query;
@group(0) @binding(31) var<storage,read_write> result:array<vec4<f32>,16>;
@group(0) @binding(1) var shadow_map:texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler:sampler_comparison;
@compute @workgroup_size(1) fn regression(){
    let world=query.world_point.xyz;
    result[u32(query.pixel_index.z)]=vec4(
        smooth_local_visibility(0u,world,vec3(0.0,0.0,1.0),query.pixel_index.xy),
        local_light_visibility(0u,world,vec3(0.0,0.0,1.0),shadow_map,shadow_sampler),
        query.world_point.w,1.0);
}
"#;

#[test]
fn native_forward_local_visibility_owns_full_resolution_stationary_receivers() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU for forward local shadow receiver ownership");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("forward local visibility receiver fixture"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("forward visibility fixture device");
    const SIZE: u32 = 32;
    const HALF: u32 = SIZE / 2;
    const MAP: u32 = 8;
    const DISTANCE: f32 = 10.03;
    let texture = |format, size, layers| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("forward visibility fixture texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let motion = texture(wgpu::TextureFormat::Rgba16Float, SIZE, 1);
    let visibility = texture(wgpu::TextureFormat::Rgba16Float, HALF, 1);
    let metadata = texture(wgpu::TextureFormat::Rgba16Float, HALF, 1);
    let shadow = texture(
        wgpu::TextureFormat::Depth32Float,
        MAP,
        POINT_SHADOW_FACES as u32,
    );
    let motion_view = motion.create_view(&Default::default());
    let visibility_view = visibility.create_view(&Default::default());
    let metadata_view = metadata.create_view(&Default::default());
    let shadow_view = shadow.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("forward visibility current and submitted frame"),
        size: std::mem::size_of::<EnhancedFrameGpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let query = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("full resolution forward receiver"),
        size: 32,
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
        label: Some("occluded local source for ownership fixture"),
        contents: &light_data,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let comparison = device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("forward cached and raw shadow samples"),
        size: 16 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("forward receiver ownership readback"),
        size: output.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production forward local visibility sampling"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(SAMPLE, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("forward visibility receiver ownership"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let receiver = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("queried forward pixel and result"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: query.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&comparison),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[],
    });
    let illumination = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("production inferred forward shadow resources"),
        layout: &pipeline.get_bind_group_layout(2),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 20,
                resource: wgpu::BindingResource::TextureView(&shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&comparison),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: lights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 16,
                resource: wgpu::BindingResource::TextureView(&visibility_view),
            },
            wgpu::BindGroupEntry {
                binding: 17,
                resource: wgpu::BindingResource::TextureView(&motion_view),
            },
            wgpu::BindGroupEntry {
                binding: 18,
                resource: frame_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 19,
                resource: wgpu::BindingResource::TextureView(&metadata_view),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    for face in 0..POINT_SHADOW_FACES as u32 {
        let view = shadow.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: face,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
    let camera = |previous_x: f32, previous_jitter: f32| {
        let projection = Mat4::perspective_infinite_reverse_rh(60.0_f32.to_radians(), 1.0, 0.1);
        let mut frame = EnhancedFrameGpu::zeroed();
        frame.clip_from_world = projection;
        frame.world_from_clip = projection.inverse();
        frame.previous_clip_from_world =
            Mat4::from_translation(Vec3::new(previous_jitter * 2.0 / SIZE as f32, 0.0, 0.0))
                * projection
                * Mat4::from_translation(Vec3::new(-previous_x, 0.0, 0.0));
        frame.viewport = Vec4::new(
            SIZE as f32,
            SIZE as f32,
            1.0 / SIZE as f32,
            1.0 / SIZE as f32,
        );
        frame.projection = Vec4::new(0.1, 0.0, 0.0, 1.0);
        frame
    };
    let world_at = |frame: EnhancedFrameGpu, pixel: Vec2| {
        let ndc = pixel / SIZE as f32 * Vec2::new(2.0, -2.0) + Vec2::new(-1.0, 1.0);
        let h = frame.world_from_clip * Vec4::new(ndc.x, ndc.y, 0.1 / DISTANCE, 1.0);
        h.truncate() / h.w
    };
    let camera_motion = |frame: EnhancedFrameGpu, pixel: Vec2| {
        let previous = frame.previous_clip_from_world * world_at(frame, pixel).extend(1.0);
        let previous_uv = Vec2::new(previous.x, -previous.y) / previous.w * 0.5 + Vec2::splat(0.5);
        let delta = previous_uv - pixel / SIZE as f32;
        Vec4::new(delta.x, delta.y, DISTANCE, 1.0)
    };
    let draw = |index: u32,
                frame: EnhancedFrameGpu,
                pixel: Vec2,
                motion: Option<Vec4>,
                cached: Option<Vec4>| {
        queue.write_buffer(&frame_buffer, 0, bytemuck::bytes_of(&frame));
        queue.write_buffer(
            &query,
            0,
            bytemuck::bytes_of(&[
                world_at(frame, pixel).extend(DISTANCE).to_array(),
                [pixel.x, pixel.y, index as f32, 0.0],
            ]),
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        for (view, clear) in [
            (&motion_view, motion),
            (&visibility_view, cached),
            (
                &metadata_view,
                cached.map(|value| Vec4::new(value.z, DISTANCE, 0.0, 0.0)),
            ),
        ] {
            if let Some(clear) = clear {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: f64::from(clear.x),
                                g: f64::from(clear.y),
                                b: f64::from(clear.z),
                                a: f64::from(clear.w),
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
            }
        }
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &receiver, &[]);
            pass.set_bind_group(1, &empty, &[]);
            pass.set_bind_group(2, &illumination, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
    };
    let pixel = Vec2::splat(SIZE as f32 / 2.0 + 0.5);
    let frame = camera(0.0, 0.0);
    let motion_value = camera_motion(frame, pixel);
    let lit_cache = Vec4::new(1.0, 1.0, 1.0, 10.0);
    draw(0, frame, pixel, Some(motion_value), Some(lit_cache));
    let camera_frame = camera(0.05, 0.5);
    let submitted_motion = camera_motion(camera_frame, pixel);
    assert!((Vec2::new(submitted_motion.x, submitted_motion.y) * SIZE as f32).length() > 0.1);
    draw(1, camera_frame, pixel, Some(submitted_motion), None);
    for (index, residual, validity) in [
        (2, 0.5, 1.0),
        (3, 0.0, -1.0),
        (4, 0.0, 0.0),
        (5, 0.05, 1.0),
        (6, 0.15, 1.0),
    ] {
        let mut motion = submitted_motion + Vec4::new(residual / SIZE as f32, 0.0, 0.0, 0.0);
        motion.w = validity;
        draw(index, camera_frame, pixel, Some(motion), None);
    }
    draw(
        7,
        frame,
        pixel,
        Some(motion_value),
        Some(Vec4::new(1.0, 1.0, 0.0, 10.0)),
    );
    let mut unavailable = frame;
    unavailable.projection.w = 0.0;
    draw(8, unavailable, pixel, None, Some(lit_cache));
    let mut capture = frame;
    capture.probe.w = -1.0;
    draw(9, capture, pixel, None, None);
    draw(10, frame, pixel, Some(motion_value), None);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d {
                x: SIZE / 2,
                y: SIZE / 2,
                z: 0,
            },
            ..motion.as_image_copy()
        },
        bytemuck::bytes_of(&[0x2400_u16, 0, 0x4900, 0x3c00]),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    draw(11, frame, pixel, None, None);
    draw(12, frame, pixel + Vec2::X, None, None);
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("forward receiver ownership readback");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("forward visibility completion");
    let values: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    for index in [0, 1, 5, 10, 12] {
        assert!(
            values[index][0] > 0.99,
            "stationary receivers retain smoothed visibility despite camera jitter and adjacent actor motion: {index}: {:?}",
            values[index]
        );
    }
    for index in [2, 3, 4, 6, 7, 8, 9, 11] {
        assert!(
            values[index][0] < 0.01,
            "moving, unknown, unavailable, and invalid receivers use the current occluded shadow: {index}: {:?}",
            values[index]
        );
    }
    assert!(
        values[..13].iter().all(|value| value[1] < 0.01),
        "the populated point-shadow map actually occludes every queried receiver: {values:?}"
    );
}
