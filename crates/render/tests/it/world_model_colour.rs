//! Real world-model fragments must light bounded snow exactly like cube terrain.
use crate::gpu_snapshot;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};

const TEXELS: [[u8; 4]; 5] = [
    [255, 255, 255, 255],
    [128, 192, 64, 255],
    [128, 192, 64, 128],
    [128, 192, 64, 0],
    [64, 96, 32, 255],
];
const NEXT_FRAME: usize = TEXELS.len() - 1;
const CASE_COUNT: usize = NEXT_FRAME;

/// Returns the centre pixel of a witness cell, including the undrawn final cell.
fn witness_pixel_offset(cell: usize) -> usize {
    let side = SNAPSHOT_SIDE as usize;
    (side / 2 * side + (cell * 2 + 1) * side / (2 * (CASE_COUNT + 1))) * 4
}

/// Decodes the reference fog colour before the production shader restores gamma RGB.
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn snow_and_other_world_models_use_native_terrain_colour_at_day_and_night() {
    let Some(gpu) = Gpu::for_fixture("world-model native colour") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("world-model native colour witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: TEXELS.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(&TEXELS),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    // Actual model entry points, not a reproduction of their arithmetic.
    let source = format!(
        "{}\n{VERTEX}",
        shader_source::standalone(include_str!("../../src/model.wgsl"), &[])
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
    .replace("GRID_COLUMNS", &format!("{}.0", CASE_COUNT + 1))
    .replace("NEXT_FRAME", &format!("{NEXT_FRAME}u"));
    let view = gpu.buffer(&[0.0; 104], wgpu::BufferUsages::UNIFORM);
    for (light, fog_amount, blend_frames, ao_face) in [
        ([1.0, 1.0, 1.0], 0.0, false, 1.0),
        ([0.13, 0.15, 0.25], 0.0, false, 1.0),
        ([0.13, 0.15, 0.25], 0.4, false, 0.6),
        ([0.13, 0.15, 0.25], 0.0, true, 0.8),
    ] {
        // A constant native byte table retains the colour-order witnesses;
        // terrain_lightmap separately exercises nonlinear coordinate lookup.
        let light = light.map(|channel| (channel * 255.0_f32).floor() / 255.0);
        let table = render::LightmapInputs::default()
            .build()
            .map(|_| [light[0], light[1], light[2], 1.0]);
        let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
        let fog = [0.5, 0.6, 0.7];
        let mut atmosphere = [0.0; 36];
        atmosphere[16..19].copy_from_slice(&fog.map(linear));
        atmosphere[20] = 100.0;
        let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
        let mut words = Vec::new();
        for layer in 0..CASE_COUNT {
            words.extend(light);
            words.push(f32::from_bits(layer as u32));
            words.extend([fog_amount * 100.0, f32::from(blend_frames), ao_face, 0.0]);
        }
        let cases = gpu.buffer(&words, wgpu::BufferUsages::STORAGE);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                resource: wgpu::BindingResource::TextureView(&atlas),
            },
            wgpu::BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                resource: wgpu::BindingResource::TextureView(&atlas),
            },
            wgpu::BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 15,
                resource: atmosphere.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 19,
                resource: cases.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 20,
                resource: lightmap.as_entire_binding(),
            },
        ];
        for (material, fragment, flags) in [
            ("opaque", "fragment", 0),
            ("cutout", "fragment", assets::MATERIAL_FLAG_ALPHA_CUTOUT),
            (
                "blended",
                "fragment_blend",
                assets::MATERIAL_FLAG_ALPHA_BLEND,
            ),
        ] {
            let source = source.replace("MATERIAL_FLAGS", &format!("{flags}u"));
            let pixels = gpu.render_srgb(
                &source,
                "model_witness_vertex",
                &[Draw {
                    fragment,
                    vertices: 0..(CASE_COUNT as u32 * 6),
                    bindings: &bindings,
                    blend: None,
                    write_depth: true,
                }],
            );
            let clear_offset = witness_pixel_offset(CASE_COUNT);
            for (layer, texel) in TEXELS[..CASE_COUNT].iter().enumerate() {
                let offset = witness_pixel_offset(layer);
                let sampled_alpha = if blend_frames {
                    (u16::from(texel[3]) + u16::from(TEXELS[NEXT_FRAME][3])).div_ceil(2) as u8
                } else {
                    texel[3]
                };
                if flags == assets::MATERIAL_FLAG_ALPHA_CUTOUT && sampled_alpha < 128 {
                    assert_eq!(
                        &pixels[offset..offset + 4],
                        &pixels[clear_offset..clear_offset + 4],
                        "{material}, layer {layer}, light {light:?}, fog {fog_amount}, frames {blend_frames}: cutout below half alpha leaves the clear colour untouched"
                    );
                    continue;
                }
                for channel in 0..3 {
                    let texture_gamma = f32::from(texel[channel]) / 255.0;
                    let texture_gamma = if blend_frames {
                        // Native frames interpolate atlas RGB, not decoded sRGB.
                        (texture_gamma + f32::from(TEXELS[NEXT_FRAME][channel]) / 255.0) * 0.5
                    } else {
                        texture_gamma
                    };
                    let expected = (texture_gamma * ao_face * light[channel] * (1.0 - fog_amount)
                        + fog[channel] * fog_amount)
                        * 255.0;
                    assert!(
                        (f32::from(pixels[offset + channel]) - expected).abs() <= 2.0,
                        "{material} {fragment}, layer {layer}, light {light:?}, fog {fog_amount}, frames {blend_frames}, channel {channel}: got {}, native {expected}",
                        pixels[offset + channel]
                    );
                }
                let alpha = if flags == 0 { 255 } else { sampled_alpha };
                assert!(
                    pixels[offset + 3].abs_diff(alpha) <= 1,
                    "{material} {fragment}, layer {layer}, light {light:?}, fog {fog_amount}, frames {blend_frames}: output alpha {}, expected {alpha}, sampled {sampled_alpha}",
                    pixels[offset + 3]
                );
            }
        }
    }
}

const VERTEX: &str = r#"
struct ModelWitnessCase { light_texture: vec4<f32>, distance_frames: vec4<f32> }
@group(0) @binding(19) var<storage, read> model_witness_cases: array<ModelWitnessCase>;
@vertex fn model_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let case_index = index / 6u;
    let witness = model_witness_cases[case_index];
    let corner = corners[index % 6u];
    var out = invisible_vertex();
    out.clip_position = vec4(-1.0 + (f32(case_index) + corner.x) * (2.0 / GRID_COLUMNS), -1.0 + corner.y * 2.0, 0.5, 1.0);
    out.uv = vec2(0.5);
    out.current_texture = bitcast<u32>(witness.light_texture.w);
    out.next_texture = NEXT_FRAME;
    out.material_flags = MATERIAL_FLAGS;
    out.frame_blend = witness.distance_frames.y * 0.5;
    out.normal = vec3(0.0, 1.0, 0.0);
    out.lighting = witness.light_texture.rgb;
    out.native_light_levels = vec2(0.0, 15.0);
    out.native_ao_face = witness.distance_frames.z;
    out.tint_gamma = vec3(1.0);
    out.world_position = vec3(witness.distance_frames.x, 0.0, 0.0);
    out.visibility.y = 1u;
    out.visibility.x = 1u;
    return out;
}
"#;
