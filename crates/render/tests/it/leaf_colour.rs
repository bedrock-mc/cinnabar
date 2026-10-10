//! Numeric legacy leaf-material witnesses: native UNORM input/output arithmetic,
//! implemented through the renderer's existing sRGB input/output boundaries.
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

const TEXTURE_RGB: [u8; 3] = [128, 192, 64];
const TEXTURE_ALPHA: [u8; 4] = [200, 0, 128, 255];

struct Case {
    tint: [f32; 3],
    ao: f32,
    light: [f32; 3],
    fog: [f32; 3],
    fog_amount: f32,
    flags: u32,
    cube: bool,
    layer: usize,
}

fn gamma_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_gamma(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn cases() -> Vec<Case> {
    let native = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR;
    let cutout = native | assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_TWO_SIDED;
    let spruce = [97.0, 153.0, 77.0].map(|value| value / 255.0);
    let palette = assets::CompiledBiomeAssets::diagnostic()
        .resolve_live(&[assets::LiveBiomeDefinition {
            name: "example:leaf_colour_witness",
            biome_id: Some(0),
            temperature: -0.5,
            downfall: 0.5,
            snow_foliage: 1.0,
            max_snow_accumulation: Some(1.0),
            map_water_argb: 0,
        }])
        .unwrap();
    let snow_palette = palette.records[palette.dense_index(0) as usize].seasonal_foliage
        [assets::seasonal_foliage_palette_index(assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE, true)];
    let snow = [snow_palette[0], snow_palette[1], snow_palette[2]].map(linear_to_gamma);
    let seasonal = assets::MATERIAL_FLAG_SEASONAL_FOLIAGE | assets::MATERIAL_FLAG_FOLIAGE_TINT;
    let mut cases = Vec::new();
    // Texture-authored seasons-agnostic leaves and palette-tinted leaves both
    // have the native RGB route, in their cutout outer and opaque deep layers.
    for (tint, species) in [([1.0; 3], 0), (spruce, seasonal), (snow, seasonal)] {
        for (flags, ao, light) in [(cutout, 1.0, [1.0; 3]), (native, 0.6, [0.7, 0.8, 0.9])] {
            cases.push(Case {
                tint,
                ao,
                light,
                fog: [0.0; 3],
                fog_amount: 0.0,
                flags: flags | species,
                cube: true,
                layer: 0,
            });
        }
    }
    for tint in [spruce, snow] {
        cases.push(Case {
            tint,
            ao: 0.8,
            light: [0.4, 0.6, 0.9],
            fog: [0.6, 0.7, 0.8],
            fog_amount: 0.4,
            flags: cutout | seasonal,
            cube: true,
            layer: 0,
        });
    }
    // Ordinary cubes (including logs seen through cutouts) use native gamma
    // lighting too, but enter through the retained sRGB sampler boundary.
    for (ao, light) in [(0.6, [1.0; 3]), (0.2, [0.15; 3])] {
        cases.push(Case {
            tint: [1.0; 3],
            ao,
            light,
            fog: [0.0; 3],
            fog_amount: 0.0,
            flags: 0,
            cube: true,
            layer: 0,
        });
    }
    // Carried materials retain their independent linear route.
    cases.push(Case {
        tint: spruce,
        ao: 0.6,
        light: [0.7, 0.8, 0.9],
        fog: [0.6, 0.7, 0.8],
        fog_amount: 0.4,
        flags: 0,
        cube: false,
        layer: 0,
    });
    // Grass-side alpha is a tint mask: dirt, mixed edge and grass all
    // remain opaque while only their authored coverage receives biome colour.
    for layer in 1..TEXTURE_ALPHA.len() {
        cases.push(Case {
            tint: spruce,
            ao: 0.6,
            light: [0.7, 0.8, 0.9],
            fog: [0.6, 0.7, 0.8],
            fog_amount: 0.4,
            flags: assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_OVERLAY_MASK,
            cube: true,
            layer,
        });
    }
    cases
}

#[test]
fn native_cube_and_leaf_pixels_match_gamma_products_without_changing_carried_colour() {
    let Some(gpu) = Gpu::for_fixture("native terrain colour") else {
        return;
    };
    assert!(material_shader::chunk_atlas_views_fit(&gpu.device.limits()));
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("leaf sRGB input witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: TEXTURE_ALPHA.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &TEXTURE_ALPHA
            .into_iter()
            .flat_map(|alpha| [TEXTURE_RGB[0], TEXTURE_RGB[1], TEXTURE_RGB[2], alpha])
            .collect::<Vec<_>>(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let native_view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let native_sampler = gpu
        .device
        .create_sampler(&material_shader::native_leaf_sampler_descriptor());
    let cases = cases();
    let mut words = Vec::new();
    for case in &cases {
        words.extend(case.tint);
        words.push(case.ao);
        words.extend(case.light);
        words.push(case.flags as f32);
        words.extend(case.fog.map(gamma_to_linear));
        words.push(case.fog_amount);
        words.extend([
            f32::from_bits(u32::from(case.cube)),
            f32::from_bits(case.layer as u32),
            0.0,
            0.0,
        ]);
    }
    let data = gpu.buffer(&words, wgpu::BufferUsages::STORAGE);
    let chunk = material_shader::source(include_str!("../../src/chunk.wgsl"));
    let source = format!("{}\n{}", shader_source::standalone(&chunk, &[]), FIXTURE);
    let pixels = gpu.render_srgb(
        &source,
        "leaf_witness_vertex",
        &[Draw {
            fragment: "leaf_witness_fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    resource: wgpu::BindingResource::TextureView(&native_view),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    resource: wgpu::BindingResource::TextureView(&native_view),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(&native_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 34,
                    resource: data.as_entire_binding(),
                },
            ],
            blend: None,
            write_depth: true,
        }],
    );
    for (index, case) in cases.iter().enumerate() {
        let x = index % 4 * 64 + 32;
        let y = index / 4 * 64 + 32;
        let offset = (y * 256 + x) * 4;
        for (channel, texture) in TEXTURE_RGB.into_iter().enumerate() {
            let texture = f32::from(texture) / 255.0;
            let expected = if case.cube {
                let tint = if case.flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0 {
                    let mask = f32::from(TEXTURE_ALPHA[case.layer]) / 255.0;
                    1.0 + (case.tint[channel] - 1.0) * mask
                } else {
                    case.tint[channel]
                };
                let lit = ((texture * tint) * case.ao) * case.light[channel];
                lit * (1.0 - case.fog_amount) + case.fog[channel] * case.fog_amount
            } else {
                let lit = gamma_to_linear(texture)
                    * gamma_to_linear(case.tint[channel])
                    * case.ao
                    * case.light[channel];
                linear_to_gamma(
                    lit * (1.0 - case.fog_amount)
                        + gamma_to_linear(case.fog[channel]) * case.fog_amount,
                )
            };
            let expected = (expected.clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(
                pixels[offset + channel].abs_diff(expected) <= 2,
                "case {index} channel {channel}: actual {} native {expected}",
                pixels[offset + channel]
            );
        }
        let opaque = !case.cube
            || case.flags
                & (assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR | assets::MATERIAL_FLAG_OVERLAY_MASK)
                != 0;
        assert_eq!(
            pixels[offset + 3],
            if opaque {
                255
            } else {
                TEXTURE_ALPHA[case.layer]
            }
        );
    }
}

const FIXTURE: &str = r#"
struct LeafWitnessCase {
    tint_ao: vec4<f32>,
    light_flags: vec4<f32>,
    fog_amount: vec4<f32>,
    route: vec4<u32>,
}
@group(0) @binding(34) var<storage, read> leaf_witness_cases: array<LeafWitnessCase>;

@vertex fn leaf_witness_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(corners[index], 0.5, 1.0);
}

@fragment fn leaf_witness_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = min(u32(position.y) / 64u * 4u + u32(position.x) / 64u, arrayLength(&leaf_witness_cases) - 1u);
    let witness = leaf_witness_cases[index];
    let flags = u32(witness.light_flags.w);
    let sampled = sample_material_texture_ref(witness.route.y, vec2(0.5), vec2(0.0), vec2(0.0), flags);
    if (witness.route.x != 0u) {
        return native_cube_colour(sampled, flags, witness.tint_ao.rgb, witness.tint_ao.w, witness.light_flags.rgb, witness.fog_amount.rgb, witness.fog_amount.w);
    }
    let lit = lit_colour(tint_to_linear(sampled).rgb * tint_to_linear(vec4(witness.tint_ao.rgb, 1.0)).rgb, witness.light_flags.rgb * witness.tint_ao.w);
    return vec4(mix(lit, witness.fog_amount.rgb, witness.fog_amount.w), 1.0);
}
"#;
