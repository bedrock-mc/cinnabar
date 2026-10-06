//! A sandboxed mod pass and mod world primitives through their production entry points.
use crate::{gpu_snapshot, shader_source};

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use mod_render::{Billboard, BillboardPattern, Decal, DecalStyle, Primitives};
use wgpu::util::DeviceExt;

const SIDE: usize = SNAPSHOT_SIDE as usize;

fn pixel(pixels: &[u8], u: f32, v: f32) -> [u8; 4] {
    let x = ((u * SIDE as f32) as usize).min(SIDE - 1);
    let y = ((v * SIDE as f32) as usize).min(SIDE - 1);
    let i = (y * SIDE + x) * 4;
    pixels[i..i + 4].try_into().unwrap()
}

/// A scene that is pure red on the left and pure blue on the right.
fn scene_texture(gpu: &Gpu) -> wgpu::TextureView {
    let mut texels = Vec::with_capacity(SIDE * SIDE * 4);
    for _ in 0..SIDE {
        for x in 0..SIDE {
            texels.extend(if x < SIDE / 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
    }
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SNAPSHOT_SIDE,
                    height: SNAPSHOT_SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
        .create_view(&Default::default())
}

fn frame_words(params: &[f32]) -> Vec<f32> {
    let mut words = Mat4::IDENTITY.to_cols_array().to_vec();
    words.extend(Mat4::IDENTITY.to_cols_array());
    words.extend([0.0, 0.0, 0.0, 1.0]);
    let side = SNAPSHOT_SIDE as f32;
    words.extend([side, side, 1.0 / side, 1.0 / side]);
    words.extend([0.0; 4]);
    let mut slots = [0.0; 16];
    slots[..params.len()].copy_from_slice(params);
    words.extend(slots);
    words
}

#[test]
fn sandboxed_pass_grades_the_scene_from_its_uniform_params() {
    let Some(gpu) = Gpu::for_fixture("sandboxed_pass_grades_the_scene_from_its_uniform_params")
    else {
        return;
    };
    // Desaturates by param 0, then darkens towards the edge by param 1.
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> {
        let c = scene(uv);
        let grey = mix(c, vec3<f32>(luminance(c)), param(0u));
        let edge = length(uv - vec2<f32>(0.5)) * 2.0;
        return grey * (1.0 - param(1u) * smoothstep(0.6, 1.4, edge));
    }";
    let shader = mod_render::shader::compose(source, false).unwrap();
    let scene = scene_texture(&gpu);
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let raster = |params: &[f32]| {
        let frame = gpu.buffer(&frame_words(params), wgpu::BufferUsages::UNIFORM);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&scene),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        gpu.render(
            &shader,
            mod_render::shader::VERTEX_ENTRY,
            &[Draw {
                fragment: mod_render::shader::FRAGMENT_ENTRY,
                vertices: 0..3,
                bindings: &bindings,
                blend: None,
                write_depth: false,
            }],
        )
    };
    let untouched = raster(&[0.0, 0.0]);
    assert_eq!(pixel(&untouched, 0.25, 0.5), [255, 0, 0, 255]);
    assert_eq!(pixel(&untouched, 0.75, 0.5), [0, 0, 255, 255]);
    let grey = raster(&[1.0, 0.0]);
    let left = pixel(&grey, 0.25, 0.5);
    assert!(left[0] == left[1] && left[1] == left[2], "{left:?}");
    assert!(
        (50..58).contains(&left[0]),
        "red luminance is 0.2126: {left:?}"
    );
    let right = pixel(&grey, 0.75, 0.5);
    assert!(
        (16..21).contains(&right[0]),
        "blue luminance is 0.0722: {right:?}"
    );
    let vignette = raster(&[0.0, 1.0]);
    assert!(pixel(&vignette, 0.01, 0.01)[0] < 40, "corners darken");
    assert_eq!(pixel(&vignette, 0.4, 0.5), [255, 0, 0, 255], "centre stays");
    gpu_snapshot::save("mod-pass-grey", &grey);
    gpu_snapshot::save("mod-pass-vignette", &vignette);
}

/// The standalone View layout with a real world-from-view basis for billboards.
fn view_words(clip_from_view: Mat4, world_from_view: Mat4) -> Vec<f32> {
    let view_from_world = world_from_view.inverse();
    let clip_from_world = clip_from_view * view_from_world;
    let mut words = Vec::new();
    for matrix in [
        clip_from_world,
        clip_from_world,
        view_from_world,
        world_from_view,
        clip_from_view,
        clip_from_view.inverse(),
    ] {
        words.extend(matrix.to_cols_array());
    }
    let eye = world_from_view.w_axis;
    words.extend([eye.x, eye.y, eye.z, 1.0, 0.0, 0.0]);
    words.extend([SNAPSHOT_SIDE as f32, SNAPSHOT_SIDE as f32]);
    words
}

#[test]
fn decals_render_on_the_ground_and_respect_scene_depth() {
    let Some(gpu) = Gpu::for_fixture("decals_render_on_the_ground_and_respect_scene_depth") else {
        return;
    };
    let source =
        shader_source::standalone(include_str!("../../src/mod_render/primitives.wgsl"), &[]);
    // Looks straight down at the origin from ten blocks up; screen right is +X.
    let world_from_view =
        Mat4::look_at_rh(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, Vec3::NEG_Z).inverse();
    let projection = Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1);
    let view = gpu.buffer(
        &view_words(projection, world_from_view),
        wgpu::BufferUsages::UNIFORM,
    );
    let frame = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::UNIFORM);
    let occluder = Billboard {
        position: [-5.0, 5.0, 0.0],
        width: 10.0,
        height: 30.0,
        color: [0.0, 1.0, 0.0, 1.0],
        pattern: BillboardPattern::Solid,
        upright: false,
    };
    let decal = Decal {
        center: [0.0, 0.0, 0.0],
        radius: 8.0,
        color: [1.0, 0.1, 0.0, 0.9],
        progress: 0.75,
        style: DecalStyle::Telegraph,
    };
    let vertices = mod_render::geometry::build(&Primitives {
        decals: vec![decal],
        billboards: vec![occluder],
        ..Default::default()
    });
    let storage = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: storage.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: frame.as_entire_binding(),
        },
    ];
    let draw = |range, write_depth, blend| Draw {
        fragment: "mod_primitive_fragment",
        vertices: range,
        bindings: &bindings,
        blend,
        write_depth,
    };
    let premultiplied = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
    let open = gpu.render(
        &source,
        "mod_primitive_vertex",
        &[draw(0..6, false, premultiplied)],
    );
    let occluded = gpu.render(
        &source,
        "mod_primitive_vertex",
        &[draw(6..12, true, None), draw(0..6, false, premultiplied)],
    );
    let background = [31, 46, 64];
    let outside = pixel(&open, 0.02, 0.5);
    assert_eq!(&outside[..3], &background, "beyond the radius stays clear");
    let left = pixel(&open, 0.3, 0.5);
    let right = pixel(&open, 0.7, 0.5);
    assert!(
        left[0] > background[0] + 30 && left[2] < background[2],
        "{left:?}"
    );
    assert_eq!(left, right, "the fill is radially symmetric");
    let rim = pixel(&open, 0.5 + 0.97 * 0.4, 0.5);
    assert!(
        rim[0] > left[0],
        "the rim outshines the fill: {rim:?} {left:?}"
    );
    assert_eq!(
        pixel(&occluded, 0.3, 0.5),
        [0, 255, 0, 255],
        "nearer geometry hides decals"
    );
    assert_eq!(pixel(&occluded, 0.7, 0.5), right);
    gpu_snapshot::save("mod-telegraph-decal", &open);
    gpu_snapshot::save("mod-telegraph-decal-occluded", &occluded);
}

/// Renders a depth ramp whose texel `i` holds `(i + 0.5) / side`, then reads it back through
/// `depth(uv)` at each pixel centre of a pass target with the same size.
fn depth_through_pass(gpu: &Gpu, side: u32) -> Vec<u8> {
    let size = wgpu::Extent3d {
        width: side,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |format, usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let depth = texture(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let depth_view = depth.create_view(&Default::default());
    let ramp = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@vertex fn v(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
                    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
                    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
                }}
                @fragment fn f(@builtin(position) p: vec4<f32>) -> @builtin(frag_depth) f32 {{
                    return p.x / {side}.0;
                }}"
                )
                .into(),
            ),
        });
    let ramp_pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &ramp,
                entry_point: Some("v"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ramp,
                entry_point: Some("f"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview: None,
            cache: None,
        });
    // Touches every binding so the derived layout matches the production one.
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(depth(uv)) + scene(uv) * param(0u); }";
    let shader = mod_render::shader::compose(source, true).unwrap();
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
    let target = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let target_view = target.create_view(&Default::default());
    let pass_pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(mod_render::shader::VERTEX_ENTRY),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(mod_render::shader::FRAGMENT_ENTRY),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview: None,
            cache: None,
        });
    let frame = gpu.buffer(&frame_words(&[]), wgpu::BufferUsages::UNIFORM);
    let scene = scene_texture(gpu);
    let sampler = gpu.device.create_sampler(&Default::default());
    let bindings = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pass_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&scene),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
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
        });
        pass.set_pipeline(&ramp_pipeline);
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
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
        pass.set_pipeline(&pass_pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
    // Rows are padded to the copy alignment.
    let row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(1),
            },
        },
        size,
    );
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    readback.slice(..).get_mapped_range()[..side as usize * 4].to_vec()
}

#[test]
fn depth_reads_the_texel_under_each_scene_pixel() {
    let Some(gpu) = Gpu::for_fixture("depth_reads_the_texel_under_each_scene_pixel") else {
        return;
    };
    let side = 4;
    let pixels = depth_through_pass(&gpu, side);
    for texel in 0..side as usize {
        let expected = (texel as f32 + 0.5) / side as f32 * 255.0;
        let actual = f32::from(pixels[texel * 4]);
        assert!(
            (actual - expected).abs() <= 1.5,
            "pixel {texel} read depth {actual}, expected {expected}: {pixels:?}"
        );
    }
}
