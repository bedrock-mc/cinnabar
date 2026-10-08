//! Actual ordinary water fragments over a submerged receiver, not alpha strings.
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu, RasterState};

#[test]
fn water_fragment_preserves_submerged_receiver_and_native_gamma_lighting() {
    let Some(gpu) = Gpu::for_fixture("water material sampling") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water straight-alpha colour witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texels = [[155_u8, 178, 201, 255], [202, 161, 129, 190]];
    gpu.queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(&texels),
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
    let view = gpu.buffer(&[0.0; 104], wgpu::BufferUsages::UNIFORM);
    let mut atmosphere = [0.0; 36];
    atmosphere[19] = 100.0;
    atmosphere[20] = 200.0;
    atmosphere[32] = 57.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let source = format!(
        "{}\n{WITNESS}",
        shader_source::standalone(
            include_str!("../../src/liquid.wgsl"),
            &["NATIVE_GAMMA_BLEND"]
        )
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    let mut failures = Vec::new();
    for (light, alpha, blend, distance, shade) in [
        ([0.82, 0.89, 0.96], 165.0 / 255.0, 0.0, 0.0, 1.0),
        ([0.16, 0.20, 0.29], 165.0 / 255.0, 0.0, 0.0, 1.0),
        ([0.33, 0.47, 0.61], 165.0 / 255.0, 0.375, 0.0, 1.0),
        ([0.82, 0.89, 0.96], 0.0, 0.0, 0.0, 1.0),
        ([0.82, 0.89, 0.96], 1.0, 0.0, 0.0, 1.0),
        ([0.82, 0.89, 0.96], 165.0 / 255.0, 0.0, 28.5, 1.0),
        ([0.82, 0.89, 0.96], 165.0 / 255.0, 0.0, 57.0, 1.0),
        ([0.82, 0.89, 0.96], 0.97, 0.0, 57.0, 1.0),
        ([0.33, 0.47, 0.61], 165.0 / 255.0, 0.375, 0.0, 0.6),
    ] {
        for (back_facing, two_sided) in [(false, false), (true, false), (true, true)] {
            let table = vec![[light[0], light[1], light[2], 1.0]; 256];
            let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
            // The native frozen-river RGB from the pack, already gamma encoded.
            let tint = [24.0 / 255.0, 83.0 / 255.0, 144.0 / 255.0];
            let case = gpu.buffer(
                &[
                    tint[0],
                    tint[1],
                    tint[2],
                    alpha,
                    light[0],
                    light[1],
                    light[2],
                    blend,
                    distance,
                    shade,
                    u32::from(two_sided) as f32,
                    u32::from(back_facing) as f32,
                ],
                wgpu::BufferUsages::STORAGE,
            );
            let bindings = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    resource: wgpu::BindingResource::TextureView(&atlas),
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
                    binding: 20,
                    resource: lightmap.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 24,
                    resource: case.as_entire_binding(),
                },
            ];
            let pixels = gpu.render_with_state(
                &source,
                "witness",
                &[Draw {
                    fragment: "fragment",
                    vertices: 0..3,
                    bindings: &bindings,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_depth: true,
                }],
                RasterState {
                    write_mask: wgpu::ColorWrites::RED
                        | wgpu::ColorWrites::GREEN
                        | wgpu::ColorWrites::BLUE,
                    ..Default::default()
                },
            );
            let sampled: [f32; 4] = std::array::from_fn(|channel| {
                (f32::from(texels[0][channel]) * (1.0 - blend)
                    + f32::from(texels[1][channel]) * blend)
                    / 255.0
            });
            let receiver: [f32; 3] = [0.12, 0.18, 0.25];
            let faded_alpha = if alpha < 0.95 {
                alpha + (1.0 - alpha) * (distance / 57.0).clamp(0.0, 1.0)
            } else {
                alpha
            };
            let effective_alpha = sampled[3] * faded_alpha;
            let expected: [u8; 3] = std::array::from_fn(|channel| {
                if back_facing && !two_sided {
                    return (receiver[channel] * 255.0).round() as u8;
                }
                let face_tint = (tint[channel] * shade * 255.0 + 0.0001).floor() / 255.0;
                let native_gamma =
                    sampled[channel] * face_tint * (light[channel] * 255.0).floor() / 255.0;
                ((native_gamma * effective_alpha + receiver[channel] * (1.0 - effective_alpha))
                    * 255.0)
                    .round() as u8
            });
            let actual = &pixels[(128 * 256 + 128) * 4..][..4];
            if actual[..3]
                .iter()
                .zip(expected)
                .any(|(&actual, expected)| actual.abs_diff(expected) > 1)
            {
                failures.push(format!("light {light:?}, alpha {alpha}, blend {blend}, distance {distance}, shade {shade}, back {back_facing}, two_sided {two_sided}: {actual:?} != {expected:?}"));
            }
            assert_eq!(
                actual[3], 255,
                "terrain blend must not replace target alpha"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const WITNESS: &str = r#"
@group(0) @binding(24) var<storage, read> witness_case: array<vec4<f32>>;
@vertex fn witness(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    let raster_uv = select(uv, uv.yx, witness_case[2].w != 0.0);
    out.clip_position = vec4(raster_uv * 2.0 - vec2(1.0), 0.5, 1.0);
    out.uv = uv;
    out.current_texture = 0u;
    out.next_texture = 1u;
    out.frame_blend = witness_case[1].w;
    out.water_tint = tint_to_linear(witness_case[0]);
    out.water_tint.a = liquid_vertex_alpha(out.water_tint.a, witness_case[2].x, atmosphere.liquid_distance.x);
    out.lighting = world_lightmap[0].rgb;
    out.native_light_levels = vec2(0.0);
    out.native_face_shade = witness_case[2].y;
    out.depth_write_route = 0u;
    out.two_sided = u32(witness_case[2].z);
    out.world_position = vec3(witness_case[2].x, 0.0, 0.0);
    return out;
}
"#;
