//! RenderChunk interpolates light coordinates, then samples the byte lightmap.
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

/// Samples the byte-quantized reference table at interpolated terrain light levels.
fn native_sample(table: &[[f32; 4]], levels: [f32; 2]) -> [f32; 3] {
    let side = table.len().isqrt();
    let coordinate = levels
        .map(|level| (level * side as f32 / (side - 1) as f32 - 0.5).clamp(0.0, (side - 1) as f32));
    let lower = coordinate.map(|value| value.floor() as usize);
    let upper = lower.map(|value| (value + 1).min(side - 1));
    let fraction = std::array::from_fn::<_, 2, _>(|axis| coordinate[axis] - lower[axis] as f32);
    std::array::from_fn(|channel| {
        let byte = |block: usize, sky: usize| {
            (table[sky * side + block][channel].clamp(0.0, 1.0) * 255.0).floor() / 255.0
        };
        let lerp = |a, b, t| a + (b - a) * t;
        lerp(
            lerp(
                byte(lower[0], lower[1]),
                byte(upper[0], lower[1]),
                fraction[0],
            ),
            lerp(
                byte(lower[0], upper[1]),
                byte(upper[0], upper[1]),
                fraction[0],
            ),
            fraction[1],
        )
    })
}

#[test]
fn terrain_fragments_sample_interpolated_levels_not_interpolated_light_rgb() {
    let Some(gpu) = Gpu::for_fixture("terrain light coordinate witness") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain light coordinate witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[255; 4],
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
    let records = gpu.buffer(&[0.0], wgpu::BufferUsages::STORAGE);
    let tints = gpu.buffer(
        &[0.0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4],
        wgpu::BufferUsages::STORAGE,
    );
    let mut atmosphere = [0.0; 32];
    atmosphere[20] = 100.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let mut table = render::LightmapInputs::default().build();
    let side = table.len().isqrt();
    // Deliberately nonlinear RGB distinguishes coordinate lookup from the
    // former vertex-colour shortcut even when both corner pixels agree.
    for (index, entry) in table.iter_mut().enumerate() {
        let block = (index % side) as f32 / (side - 1) as f32;
        let sky = (index / side) as f32 / (side - 1) as f32;
        *entry = [
            0.07 + 0.8 * sky.powi(3),
            0.05 + 0.8 * block.powi(2),
            0.03 + 0.8 * sky * block,
            1.0,
        ];
    }
    let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
    let mut failures = Vec::new();
    for (kind, shader, fragments) in [
        (
            "model",
            include_str!("../../src/model.wgsl"),
            &["fragment", "fragment_blend"][..],
        ),
        (
            "cube",
            include_str!("../../src/chunk.wgsl"),
            &["fragment"][..],
        ),
    ] {
        let vertex = if kind == "model" {
            MODEL_VERTEX
        } else {
            CUBE_VERTEX
        };
        let source = format!(
            "{}\n{VERTEX}\n{vertex}",
            shader_source::standalone(shader, &[])
        )
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
        for (lower, upper) in [
            ([0.0, 13.0], [0.0, 15.0]),
            ([0.0, 0.0], [15.0, 15.0]),
            ([3.0, 12.0], [13.0, 2.0]),
            ([0.0, 0.0], [0.0, 0.0]),
            ([15.0, 15.0], [15.0, 15.0]),
        ] {
            let case = gpu.buffer(
                &[lower[0], lower[1], 0.0, 0.0, upper[0], upper[1], 0.0, 0.0],
                wgpu::BufferUsages::STORAGE,
            );
            let mut bindings = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
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
                    binding: 19,
                    resource: case.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 20,
                    resource: lightmap.as_entire_binding(),
                },
            ];
            if kind == "cube" {
                bindings.extend([
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: records.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: tints.as_entire_binding(),
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
                        binding: material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ]);
            }
            for &fragment in fragments {
                let pixels = gpu.render_srgb(
                    &source,
                    "terrain_witness_vertex",
                    &[Draw {
                        fragment,
                        vertices: 0..6,
                        bindings: &bindings,
                        blend: None,
                        write_depth: true,
                    }],
                );
                for x in [0, 32, 64, 128, 192, 255] {
                    let fraction = (x as f32 + 0.5) / 256.0;
                    let levels = std::array::from_fn(|axis| {
                        lower[axis] + (upper[axis] - lower[axis]) * fraction
                    });
                    let expected = native_sample(&table, levels);
                    let offset = (128 * 256 + x) * 4;
                    for channel in 0..3 {
                        let native = expected[channel] * 255.0;
                        let actual = pixels[offset + channel];
                        if (f32::from(actual) - native).abs() > 1.5 {
                            failures.push(format!("{kind}/{fragment} {lower:?}->{upper:?} x{x} c{channel}: got{actual}, native{native:.3}"));
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const VERTEX: &str = r#"
struct TerrainWitness { lower: vec4<f32>, upper: vec4<f32> }
@group(0) @binding(19) var<storage, read> terrain_witness: TerrainWitness;
fn witness_corner(index: u32) -> vec2<f32> {
    return array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0))[index];
}
fn witness_sample(corner: vec2<f32>) -> u32 {
    let levels = mix(terrain_witness.lower.xy, terrain_witness.upper.xy, corner.x);
    return u32(levels.x) | (u32(levels.y) << 4u);
}
"#;

const MODEL_VERTEX: &str = r#"
@vertex fn terrain_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = witness_corner(index);
    var out = invisible_vertex();
    out.clip_position = vec4(corner * 2.0 - vec2(1.0), 0.5, 1.0);
    out.uv = vec2(0.5);
    out.native_light_levels = terrain_light_levels(witness_sample(corner));
    out.native_ao_face = 1.0;
    out.tint_gamma = vec3(1.0);
    out.two_sided = 1u;
    out.visible = 1u;
    return out;
}
"#;

const CUBE_VERTEX: &str = r#"
@vertex fn terrain_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = witness_corner(index);
    var out: VertexOutput;
    out.clip_position = vec4(corner * 2.0 - vec2(1.0), 0.5, 1.0);
    out.uv = vec2(0.5);
    out.native_light_levels = terrain_light_levels(witness_sample(corner));
    out.native_ao_face = 1.0;
    return out;
}
"#;
