//! Native atlas filtering uses UNORM views of the existing sRGB allocations.
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

// Two texels repeat vertically, making the minified centre a known average.
const BASE: [[[[u8; 3]; 2]; 2]; assets::MAX_TEXTURE_PAGES] = [
    [[[0; 3], [255; 3]], [[255, 0, 128], [0, 255, 32]]],
    [
        [[16, 64, 240], [240, 192, 16]],
        [[64, 32, 0], [192, 224, 255]],
    ],
];
const MIP: [[[u8; 3]; 2]; assets::MAX_TEXTURE_PAGES] =
    [[[128; 3], [128, 128, 80]], [[128; 3], [128; 3]]];

struct Case {
    page: u32,
    layer: u32,
    native: bool,
    minified: bool,
    blend: f32,
}

/// Models point texels and linear interpolation between encoded mip levels.
fn sampled_channel(page: u32, layer: u32, channel: usize, minified: bool) -> f32 {
    let base = f32::from(BASE[page as usize][layer as usize][0][channel]) / 255.0;
    if !minified {
        return base;
    }
    let mip = f32::from(MIP[page as usize][layer as usize][channel]) / 255.0;
    (base + mip) * 0.5
}

fn texture(gpu: &Gpu, page: usize) -> wgpu::Texture {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shared sRGB/UNORM atlas allocation"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: BASE[page].len() as u32,
        },
        mip_level_count: 2,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    for (level, size) in [(0, 2), (1, 1)] {
        let mut bytes = Vec::new();
        for (layer, texels) in BASE[page].iter().enumerate() {
            for _ in 0..size {
                for &base_rgb in texels.iter().take(size) {
                    let rgb = if level == 0 {
                        base_rgb
                    } else {
                        MIP[page][layer]
                    };
                    bytes.extend(rgb);
                    bytes.push(255);
                }
            }
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size as u32 * 4),
                rows_per_image: Some(size as u32),
            },
            wgpu::Extent3d {
                width: size as u32,
                height: size as u32,
                depth_or_array_layers: BASE[page].len() as u32,
            },
        );
    }
    texture
}

#[test]
fn terrain_unorm_views_filter_both_pages_layers_and_frame_mix() {
    let Some(gpu) = Gpu::for_fixture("terrain texture filtering") else {
        return;
    };
    assert!(material_shader::chunk_atlas_views_fit(&gpu.device.limits()));
    let textures =
        std::array::from_fn::<_, { assets::MAX_TEXTURE_PAGES }, _>(|page| texture(&gpu, page));
    let native_views = textures.each_ref().map(|texture| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let native_sampler = gpu
        .device
        .create_sampler(&material_shader::native_leaf_sampler_descriptor());
    let mut cases = Vec::new();
    for page in 0..assets::MAX_TEXTURE_PAGES as u32 {
        for layer in 0..BASE[page as usize].len() as u32 {
            for native in [false, true] {
                cases.push(Case {
                    page,
                    layer,
                    native,
                    minified: true,
                    blend: 0.0,
                });
            }
        }
    }
    for native in [false, true] {
        cases.push(Case {
            page: 0,
            layer: 1,
            native,
            minified: true,
            blend: 0.375,
        });
        cases.push(Case {
            page: 1,
            layer: 1,
            native,
            minified: false,
            blend: 0.0,
        });
    }
    let mut words = Vec::new();
    for case in &cases {
        let flags = if case.native {
            assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR
        } else {
            0
        };
        let reference = assets::TextureRef::new(case.page, case.layer)
            .unwrap()
            .raw();
        let next = assets::TextureRef::new(1, 0).unwrap().raw();
        words.extend([
            f32::from_bits(reference),
            f32::from_bits(next),
            f32::from_bits(flags),
            0.0,
        ]);
        let (uv, gradient) = if case.minified {
            (0.4, 2.0_f32.sqrt() * 0.5)
        } else {
            (0.25, 0.05)
        };
        words.extend([uv, uv, gradient, case.blend]);
    }
    let data = gpu.buffer(&words, wgpu::BufferUsages::STORAGE);
    let chunk = material_shader::source(include_str!("../../src/chunk.wesl"));
    let source = format!("{}\n{FIXTURE}", shader_source::standalone(&chunk, &[]));
    let pixels = gpu.render_srgb(
        &source,
        "filter_vertex",
        &[Draw {
            fragment: "filter_fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    resource: wgpu::BindingResource::TextureView(&native_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    resource: wgpu::BindingResource::TextureView(&native_views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(&native_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 19,
                    resource: data.as_entire_binding(),
                },
            ],
            blend: None,
            write_depth: true,
        }],
    );
    for (index, case) in cases.iter().enumerate() {
        let offset = (((index / 4 * 64 + 32) * 256) + index % 4 * 64 + 32) * 4;
        for channel in 0..3 {
            let current = sampled_channel(case.page, case.layer, channel, case.minified);
            let next = sampled_channel(1, 0, channel, case.minified);
            let value = current * (1.0 - case.blend) + next * case.blend;
            let gamma = value;
            let expected = (gamma.clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(
                pixels[offset + channel].abs_diff(expected) <= 2,
                "case {index} channel {channel}: actual {} expected {expected}",
                pixels[offset + channel]
            );
        }
        assert_eq!(pixels[offset + 3], 255);
    }
}

#[test]
fn native_leaf_point_mip_filter_preserves_alpha_without_changing_carried_mips() {
    let Some(gpu) = Gpu::for_fixture("terrain texture filtering") else {
        return;
    };
    assert!(material_shader::chunk_atlas_views_fit(&gpu.device.limits()));
    let mut base = vec![0; 4 * 4 * 4];
    for y in 0..2 {
        for x in 0..2 {
            base[(y * 4 + x) * 4..(y * 4 + x + 1) * 4].copy_from_slice(&[140, 140, 140, 255]);
        }
    }
    let native = assets::build_legacy_terrain_mip_chain(&base, 4).unwrap();
    let carried = assets::build_texture_mip_chain(base.into_boxed_slice(), 4).unwrap();
    assert_eq!(native[0], carried[0]);
    assert_eq!(native[1].rgba8[3], 255);
    assert_eq!(carried[1].rgba8[3], 128);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native world and unchanged carried mip witness"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 2,
        },
        mip_level_count: native.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    for (level, mip) in native.iter().enumerate() {
        let bytes = [mip.rgba8.as_ref(), carried[level].rgba8.as_ref()].concat();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(mip.size * 4),
                rows_per_image: Some(mip.size),
            },
            wgpu::Extent3d {
                width: mip.size,
                height: mip.size,
                depth_or_array_layers: 2,
            },
        );
    }
    let unorm = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let native_sampler = gpu
        .device
        .create_sampler(&material_shader::native_leaf_sampler_descriptor());
    let chunk = material_shader::source(include_str!("../../src/chunk.wesl"));
    let source = format!("{}\n{MIP_FIXTURE}", shader_source::standalone(&chunk, &[]));
    let pixels = gpu.render(
        &source,
        "mip_vertex",
        &[Draw {
            fragment: "mip_fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    resource: wgpu::BindingResource::TextureView(&unorm),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    resource: wgpu::BindingResource::TextureView(&unorm),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(&native_sampler),
                },
            ],
            blend: None,
            write_depth: true,
        }],
    );
    for (x, expected) in [(64, 255_u8), (192, 128_u8)] {
        let offset = (128 * 256 + x) * 4;
        for channel in 0..3 {
            assert!(
                pixels[offset + channel].abs_diff(expected) <= 2,
                "filtered mip alpha at {x}: {} vs {expected}",
                pixels[offset + channel]
            );
        }
    }
}

const MIP_FIXTURE: &str = r#"
@vertex fn mip_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(p[index], 0.5, 1.0);
}
@fragment fn mip_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let layer = select(0u, 1u, position.x >= 128.0);
    // A 4x4 base and .5 gradients choose mip1. Native point filtering selects
    // its opaque texel; the carried chain retains its authored alpha strength.
    let sampled = sample_material_texture_ref(layer, vec2(.35,.25), vec2(.5,0.0), vec2(0.0,.5), NATIVE_LEAF_COLOUR);
    return vec4(sampled.aaa, 1.0);
}
"#;

const FIXTURE: &str = r#"
struct FilterCase { refs_flags: vec4<u32>, uv_grad_blend: vec4<f32>, }
@group(0) @binding(19) var<storage, read> filter_cases: array<FilterCase>;
@vertex fn filter_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(p[index], 0.5, 1.0);
}
@fragment fn filter_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = min(u32(position.y)/64u * 4u + u32(position.x)/64u, arrayLength(&filter_cases)-1u);
    let witness = filter_cases[index];
    let dx = vec2(witness.uv_grad_blend.z, 0.0);
    let dy = vec2(0.0, witness.uv_grad_blend.z);
    let current = sample_material_texture_ref(witness.refs_flags.x, witness.uv_grad_blend.xy, dx, dy, witness.refs_flags.z);
    let next = sample_material_texture_ref(witness.refs_flags.y, witness.uv_grad_blend.xy, dx, dy, witness.refs_flags.z);
    let sampled = mix(current, next, witness.uv_grad_blend.w);
    if (material_uses_native_leaf_colour(witness.refs_flags.z)) {
        return vec4(native_leaf_colour(sampled.rgb, vec3(1.0), 1.0, vec3(1.0), vec3(0.0), 0.0), 1.0);
    }
    return vec4(tint_to_linear(sampled).rgb, 1.0);
}
"#;
