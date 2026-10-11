//! The production entity-shadow shader over a rendered floor: shaded pixels are the floor's
//! encoded colour times the shadow multiplier, once, however many volumes overlap.
use crate::gpu_snapshot::Gpu;
use crate::shader_source;
use bevy::math::{Mat4, Vec3};
use render_model::{EntityShadow, EntityShadowParams, SHADOW_VOLUME_VERTICES, shadow_volume_mesh};
use wgpu::util::DeviceExt;

const SIDE: u32 = 64;
const FLOOR_Y: f32 = 64.0;
const FLOOR_BYTE: u8 = 128;

const FLOOR: &str = "
struct Camera { clip_from_world: mat4x4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@vertex fn floor(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = array(vec2(-50.0, -50.0), vec2(50.0, -50.0), vec2(50.0, 50.0),
        vec2(-50.0, -50.0), vec2(50.0, 50.0), vec2(-50.0, 50.0))[index];
    return camera.clip_from_world * vec4(corner.x, 64.0, corner.y, 1.0);
}
@fragment fn floor_colour() -> @location(0) vec4<f32> { return vec4(vec3(LINEAR), 1.0); }";

/// The linear value an sRGB target encodes as `FLOOR_BYTE`.
fn floor_linear() -> f64 {
    ((f64::from(FLOOR_BYTE) / 255.0 + 0.055) / 1.055).powf(2.4)
}

struct Scene {
    clip_from_world: Mat4,
    view_from_clip: Mat4,
    world_from_view: Mat4,
    eye: Vec3,
}

/// Looks straight down at the floor from ten blocks above the origin, with -Z up the screen.
fn scene() -> Scene {
    let eye = Vec3::new(0.0, FLOOR_Y + 10.0, 0.0);
    let world_from_view = Mat4::look_to_rh(eye, Vec3::NEG_Y, Vec3::NEG_Z).inverse();
    let clip_from_view = Mat4::perspective_infinite_reverse_rh(60f32.to_radians(), 1.0, 0.05);
    Scene {
        clip_from_world: clip_from_view * world_from_view.inverse(),
        view_from_clip: clip_from_view.inverse(),
        world_from_view,
        eye,
    }
}

/// Packs the standalone `View` layout the shader is validated against.
fn view_words(scene: &Scene) -> Vec<f32> {
    let mut words = Vec::new();
    for matrix in [
        scene.clip_from_world,
        scene.clip_from_world,
        scene.world_from_view.inverse(),
        scene.world_from_view,
        scene.view_from_clip.inverse(),
        scene.view_from_clip,
    ] {
        words.extend(matrix.to_cols_array());
    }
    words.extend([scene.eye.x, scene.eye.y, scene.eye.z, 1.0]);
    words.extend([0.0, 0.0, SIDE as f32, SIDE as f32]);
    words
}

/// Requests the native formats used by the scene, sampled depth and overlap stencil.
fn fixture() -> Option<(Gpu, Vec<u32>)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) if adapter.get_info().backend != wgpu::Backend::Noop => adapter,
        Ok(_) | Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("missing fixture: native GPU for entity shadow raster");
            return None;
        }
        Err(error) => panic!("entity shadow raster adapter: {error}"),
    };
    let counts = [1, 2, 4]
        .into_iter()
        .filter(|samples| {
            let supported = [
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureFormat::Stencil8,
            ]
            .into_iter()
            .all(|format| {
                adapter
                    .get_texture_format_features(format)
                    .flags
                    .sample_count_supported(*samples)
            });
            if !supported {
                eprintln!("missing fixture: entity shadow {samples}x attachment support");
            }
            supported
        })
        .collect();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ..Default::default()
    }))
    .expect("entity shadow raster device");
    Some((
        Gpu {
            device,
            queue,
            backend: adapter.get_info().backend,
        },
        counts,
    ))
}

/// Draws the floor and shadow union into compatible views of the same colour samples.
fn render(gpu: &Gpu, casters: &[EntityShadow], samples: u32) -> Vec<u8> {
    let device = &gpu.device;
    let scene = scene();
    let size = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let texture = |format, usage, sample_count, view_formats: &[wgpu::TextureFormat]| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats,
        })
    };
    let target = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | if samples == 1 {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            },
        samples,
        &[wgpu::TextureFormat::Rgba8UnormSrgb],
    );
    let resolved = texture(
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        1,
        &[],
    );
    let depth = texture(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        samples,
        &[],
    );
    let stencil = texture(
        wgpu::TextureFormat::Stencil8,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        samples,
        &[],
    );
    let stencil_view = stencil.create_view(&Default::default());
    let encoded_view = target.create_view(&Default::default());
    let resolved_view = resolved.create_view(&Default::default());
    let (target_view, depth_view) = (
        target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ..Default::default()
        }),
        depth.create_view(&Default::default()),
    );
    let init = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents,
            usage,
        })
    };
    let camera = init(
        bytemuck::cast_slice(&scene.clip_from_world.to_cols_array()),
        wgpu::BufferUsages::UNIFORM,
    );
    let floor_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            FLOOR
                .replace("LINEAR", &format!("{:.8}", floor_linear()))
                .into(),
        ),
    });
    let floor = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &floor_module,
            entry_point: Some("floor"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &floor_module,
            entry_point: Some("floor_colour"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba8UnormSrgb.into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let floor_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &floor.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera.as_entire_binding(),
        }],
    });
    let source = shader_source::standalone(
        include_str!("../../src/entity_shadow.wgsl"),
        if samples > 1 { &["MULTISAMPLED"] } else { &[] },
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("shadow_vertex"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            }],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Front),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Stencil8,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::NotEqual,
                    pass_op: wgpu::StencilOperation::Replace,
                    ..Default::default()
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::NotEqual,
                    pass_op: wgpu::StencilOperation::Replace,
                    ..Default::default()
                },
                read_mask: 0xff,
                write_mask: 0xff,
            },
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("shadow_fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::Src,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::COLOR,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let view = init(
        bytemuck::cast_slice(&view_words(&scene)),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut instances = casters.to_vec();
    instances.push(EntityShadow::default());
    let instances = init(
        bytemuck::cast_slice(&instances),
        wgpu::BufferUsages::STORAGE,
    );
    let params = EntityShadowParams::new([0.7, 0.7, 0.7, 1.0]);
    let params = init(bytemuck::bytes_of(&params), wgpu::BufferUsages::UNIFORM);
    let mesh = init(
        bytemuck::cast_slice(&shadow_volume_mesh()),
        wgpu::BufferUsages::VERTEX,
    );
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &shadow.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: instances.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let clear = floor_linear();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear,
                        g: clear,
                        b: clear,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&floor);
        pass.set_bind_group(0, &floor_group, &[]);
        pass.draw(0..6, 0..1);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &encoded_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &stencil_view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&shadow);
        pass.set_stencil_reference(1);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, mesh.slice(..));
        pass.draw(0..SHADOW_VOLUME_VERTICES as u32, 0..casters.len() as u32);
    }
    if samples > 1 {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("entity shadow final scene resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                depth_slice: None,
                resolve_target: Some(&resolved_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIDE * SIDE * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        if samples > 1 {
            resolved.as_image_copy()
        } else {
            target.as_image_copy()
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
        },
        size,
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    readback.slice(..).get_mapped_range().to_vec()
}

/// Encoded red byte at the pixel showing floor point `(x, z)`.
fn at(pixels: &[u8], x: f32, z: f32) -> u8 {
    let clip = scene().clip_from_world * bevy::math::Vec4::new(x, FLOOR_Y, z, 1.0);
    let column = ((clip.x / clip.w * 0.5 + 0.5) * SIDE as f32) as u32;
    let row = ((0.5 - clip.y / clip.w * 0.5) * SIDE as f32) as u32;
    pixels[((row * SIDE + column) * 4) as usize]
}

#[test]
fn shadows_darken_the_floor_once_inside_each_footprint() {
    let Some((gpu, samples)) = fixture() else {
        return;
    };
    let shaded = (f32::from(FLOOR_BYTE) * 0.7).round() as i32;
    let caster = |x: f32, y: f32, radius: f32| EntityShadow {
        feet: [x, y, 0.0],
        radius,
    };
    for samples in samples {
        let pixels = render(
            &gpu,
            &[
                caster(0.0, FLOOR_Y, 1.0),
                // Overlaps the first: the shared floor still darkens once.
                caster(0.4, FLOOR_Y, 1.0),
                // Hangs 3.5 radii above the floor, past the volume's reach.
                caster(3.0, FLOOR_Y + 3.5, 1.0),
                // 1.5 radii up: the floor sits inside a half-width footprint.
                caster(-3.0, FLOOR_Y + 1.5, 1.0),
            ],
            samples,
        );
        let near = |byte: u8, expected: i32| (i32::from(byte) - expected).abs() <= 1;
        assert!(
            near(at(&pixels, 0.0, 0.0), shaded),
            "under the feet: {}",
            at(&pixels, 0.0, 0.0)
        );
        assert!(
            near(at(&pixels, 0.2, 0.0), shaded),
            "overlap: {}",
            at(&pixels, 0.2, 0.0)
        );
        assert!(
            near(at(&pixels, 0.0, 1.2), i32::from(FLOOR_BYTE)),
            "outside the footprint"
        );
        assert!(
            near(at(&pixels, 3.0, 0.0), i32::from(FLOOR_BYTE)),
            "beyond three radii"
        );
        assert!(
            near(at(&pixels, -3.0, 0.0), shaded),
            "1.5 radii below the feet"
        );
        assert!(
            near(at(&pixels, -3.0, 0.62), i32::from(FLOOR_BYTE)),
            "past the narrowed edge"
        );
        assert!(
            near(at(&pixels, 4.5, 4.5), i32::from(FLOOR_BYTE)),
            "open floor"
        );
    }
}

/// A camera inside the volume has no visible entry face to mark a shadow receiver.
#[test]
fn a_camera_inside_a_volume_does_not_shade_the_floor() {
    let Some((gpu, samples)) = fixture() else {
        return;
    };
    // Feet 0.5 above the eye, radius 4: the volume reaches 12 blocks down past the floor.
    let around_camera = EntityShadow {
        feet: [0.0, FLOOR_Y + 10.5, 0.0],
        radius: 4.0,
    };
    for samples in samples {
        let pixels = render(&gpu, &[around_camera], samples);
        assert!(
            (i32::from(at(&pixels, 0.0, 0.0)) - i32::from(FLOOR_BYTE)).abs() <= 1,
            "{samples}x MSAA camera inside volume: {}",
            at(&pixels, 0.0, 0.0)
        );
        let ordinary = EntityShadow {
            feet: [0.0, FLOOR_Y, 0.0],
            radius: 1.0,
        };
        let pixels = render(&gpu, &[around_camera, ordinary], samples);
        let shaded = (f32::from(FLOOR_BYTE) * 0.7).round() as i32;
        assert!(
            (i32::from(at(&pixels, 0.0, 0.0)) - shaded).abs() <= 1,
            "{samples}x MSAA another caster still shades the floor"
        );
    }
}
