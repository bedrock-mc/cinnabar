use crate::{gpu_snapshot, shader_source};

use assets::ResolvedFog;
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render::{AtmosphereFrame, SkyKind};
use wgpu::util::DeviceExt;

fn texture(gpu: &Gpu, size: [u32; 2], pixels: &[u8]) -> wgpu::TextureView {
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("owned End sky regression texture"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            pixels,
        )
        .create_view(&Default::default())
}

fn sampler(gpu: &Gpu) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        ..Default::default()
    })
}

fn frame(fog_rgb: [f32; 3]) -> AtmosphereFrame {
    AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0)
        .with_sky_kind(SkyKind::End)
        .with_environment_profile(
            Some(0x80_40_C0),
            Some(ResolvedFog {
                start: 10.0,
                end: 100.0,
                rgb: fog_rgb,
            }),
        )
}

fn frame_buffer(gpu: &Gpu, frame: &AtmosphereFrame) -> wgpu::Buffer {
    gpu.buffer(
        bytemuck::cast_slice(bytemuck::bytes_of(frame)),
        wgpu::BufferUsages::UNIFORM,
    )
}

fn assert_pixel(pixels: &[u8], x: usize, expected: [u8; 4]) {
    let index = (SNAPSHOT_SIDE as usize / 2 * SNAPSHOT_SIDE as usize + x) * 4;
    let actual = &pixels[index..index + 4];
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
        "pixel at {x}: {actual:?}, expected {expected:?}"
    );
}

#[test]
fn end_dimension_has_no_additive_base_sky_colour() {
    let end = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_sky_kind(SkyKind::End);
    assert_eq!(end.sky_zenith(), [0.0; 3]);
    assert_eq!(end.sky_horizon(), [0.0; 3]);
    assert_eq!(end.star_brightness(), 0.0);
    assert_eq!(end.sunrise_band()[3], 0.0);
}

#[test]
fn end_sky_multiplies_texture_by_resolved_fog_in_gamma_space() {
    let Some(gpu) = Gpu::for_fixture("end_sky_multiplies_texture_by_resolved_fog_in_gamma_space")
    else {
        return;
    };
    let source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let atmosphere = frame_buffer(&gpu, &frame([0.1, 0.2, 0.3]));
    let stars = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::STORAGE);
    let neutral = texture(&gpu, [1, 1], &[255; 4]);
    let sampler = sampler(&gpu);
    for (texel, expected) in [
        ([0, 0, 0, 255], [0, 0, 0, 255]),
        ([128, 128, 128, 255], [26, 51, 77, 255]),
    ] {
        let end_sky = texture(&gpu, [1, 1], &texel);
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: atmosphere.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&neutral),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&neutral),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&end_sky),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: stars.as_entire_binding(),
            },
        ];
        let pixels = gpu.render_srgb(
            &source,
            "atmosphere_vertex",
            &[Draw {
                fragment: "atmosphere_fragment",
                vertices: 0..3,
                bindings: &entries,
                blend: None,
                write_depth: false,
            }],
        );
        assert_pixel(&pixels, SNAPSHOT_SIDE as usize / 2, expected);
    }
}

#[test]
fn end_sky_repeats_the_pack_texture_on_all_six_world_aligned_faces() {
    let Some(gpu) =
        Gpu::for_fixture("end_sky_repeats_the_pack_texture_on_all_six_world_aligned_faces")
    else {
        return;
    };
    // Cube quad corners in UV order 00,10,11,01 after its model transform.
    let faces = [
        [
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ],
        [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
        ],
        [
            [-1.0, 1.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ],
        [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, -1.0, 1.0],
            [-1.0, -1.0, 1.0],
        ],
        [
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 1.0, 1.0],
            [1.0, -1.0, 1.0],
        ],
        [
            [-1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, -1.0, 1.0],
        ],
    ];
    let texels = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255; 4],
    ];
    let mut rays = Vec::new();
    for face in faces {
        let corners = face.map(Vec3::from_array);
        for (index, tile) in [0.0, 5.0, 9.0, 15.0].into_iter().enumerate() {
            let u = (tile + if index & 1 == 0 { 0.25 } else { 0.75 }) / 16.0;
            let v = (tile + if index & 2 == 0 { 0.25 } else { 0.75 }) / 16.0;
            let ray = corners[0] * ((1.0 - u) * (1.0 - v))
                + corners[1] * (u * (1.0 - v))
                + corners[2] * (u * v)
                + corners[3] * ((1.0 - u) * v);
            rays.extend([ray.x, ray.y, ray.z, 0.0]);
        }
    }
    let count = rays.len() / 4;
    let mut source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    source.push_str(&format!(
        "\n@vertex fn face_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {{
            let p = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
            return VertexOutput(vec4(p, 0.0, 1.0), 0.0);
        }}
        @fragment fn face_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {{
            let column = u32(input.position.x / {}.0 * {}.0);
            if u32(atmosphere.sky_extra.w + 0.5) != 2u {{ return vec4(0.0); }}
            return sky_output(end_sky(stars[column].xyz));
        }}",
        SNAPSHOT_SIDE, count,
    ));
    let atmosphere = frame_buffer(&gpu, &frame([0.5; 3]));
    let rays = gpu.buffer(&rays, wgpu::BufferUsages::STORAGE);
    let end_sky = texture(&gpu, [2, 2], &texels.concat());
    let sampler = sampler(&gpu);
    let entries = [
        wgpu::BindGroupEntry {
            binding: 1,
            resource: atmosphere.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(&end_sky),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: rays.as_entire_binding(),
        },
    ];
    let pixels = gpu.render_srgb(
        &source,
        "face_probe_vertex",
        &[Draw {
            fragment: "face_probe_fragment",
            vertices: 0..3,
            bindings: &entries,
            blend: None,
            write_depth: false,
        }],
    );
    for column in 0..count {
        let x = ((column * 2 + 1) * SNAPSHOT_SIDE as usize) / (count * 2);
        assert_pixel(&pixels, x, texels[column % texels.len()]);
    }
}
