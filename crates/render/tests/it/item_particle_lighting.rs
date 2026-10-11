//! Exercise the production item and particle shaders on an sRGB render target.
use crate::gpu_snapshot;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};
use particles::ParticleInstance;
use render::LightmapInputs;

const TEXEL: [u8; 4] = [128, 192, 64, 255];
const TINT: [f32; 4] = [0.75, 0.5, 1.0, 1.0];

// Independent native /16, clamp-linear, byte-texture reference. This deliberately
// samples four neighbours instead of addressing the unquantized table directly.
fn native_light(table: &[[f32; 4]; 256], block: u8, sky: u8) -> [f32; 3] {
    let blocks = [block.saturating_sub(1), block];
    let skies = [sky.saturating_sub(1), sky];
    std::array::from_fn(|channel| {
        blocks
            .into_iter()
            .flat_map(|b| skies.map(move |s| usize::from(b) + usize::from(s) * 16))
            .map(|index| (table[index][channel].clamp(0.0, 1.0) * 255.0).floor() / 255.0)
            .sum::<f32>()
            * 0.25
    })
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn items_and_particles_compose_native_rgb_in_darkness_and_daylight() {
    let gpu = Gpu::new().expect("native GPU");
    let view = gpu.buffer(
        &gpu_snapshot::view(bevy::math::Mat4::IDENTITY, bevy::math::Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 100.0;
    atmosphere[20] = 200.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let sampler = gpu.device.create_sampler(&Default::default());
    let mut item_source =
        shader_source::standalone(include_str!("../../src/dropped_item.wesl"), &[])
            .replace("@vertex\nfn item_vertex", "fn item_vertex");
    item_source.push_str(ITEM_VERTEX);
    let mut particle_source =
        shader_source::standalone(include_str!("../../src/particles.wesl"), &[])
            .replace("@vertex\nfn particle_vertex", "fn particle_vertex")
            .replace("@builtin(vertex_index) vertex_index:", "vertex_index:")
            .replace(
                "@builtin(instance_index) instance_index:",
                "instance_index:",
            );
    particle_source.push_str(PARTICLE_VERTEX);
    // The snapshot harness binds a single group; retain the actual shared uniforms.
    let flatten = |source: String| {
        source
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
            .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
    };
    let item_source = flatten(item_source);
    let particle_source = flatten(particle_source);
    for (scenario, inputs) in [
        (
            "day",
            LightmapInputs {
                ambient_adjustment: true,
                brightness: 0.5,
                ..Default::default()
            },
        ),
        (
            "night",
            LightmapInputs {
                sky_darken: 0.0,
                ambient_adjustment: true,
                brightness: 0.5,
                ..Default::default()
            },
        ),
        (
            "minimum-brightness",
            LightmapInputs {
                ambient_adjustment: true,
                ..Default::default()
            },
        ),
        (
            "maximum-brightness",
            LightmapInputs {
                ambient_adjustment: true,
                brightness: 1.0,
                ..Default::default()
            },
        ),
        (
            "night-vision",
            LightmapInputs {
                sky_darken: 0.0,
                ambient_adjustment: true,
                night_vision: 0.8,
                ..Default::default()
            },
        ),
    ] {
        let table = inputs.build();
        let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
        for (block, sky) in [(0_u8, 0_u8), (0, 15), (10, 0), (15, 15)] {
            let light = native_light(&table, block, sky);
            for (particle, lit) in [(false, true), (true, true), (true, false)] {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("item/particle lighting texel"),
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
                    &TEXEL,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4),
                        rows_per_image: Some(1),
                    },
                    texture.size(),
                );
                let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(if particle {
                        wgpu::TextureViewDimension::D2
                    } else {
                        wgpu::TextureViewDimension::D2Array
                    }),
                    ..Default::default()
                });
                let record = ParticleInstance {
                    center_light: [0.0, 0.0, 0.5, f32::from(block | (sky << 4))],
                    axis_x: [1.0, 0.0, 0.0, 0.0],
                    axis_y: [0.0, 1.0, 0.0, f32::from(lit)],
                    uv: [0.0, 0.0, 1.0, 1.0],
                    color: TINT,
                };
                let records =
                    gpu.buffer(bytemuck::cast_slice(&[record]), wgpu::BufferUsages::STORAGE);
                let case = gpu.buffer(
                    &[
                        f32::from(block),
                        f32::from(sky),
                        0.0,
                        0.0,
                        TINT[0],
                        TINT[1],
                        TINT[2],
                        TINT[3],
                    ],
                    wgpu::BufferUsages::UNIFORM,
                );
                let (source, vertex, fragment, bindings) = if particle {
                    (
                        &particle_source,
                        "particle_witness_vertex",
                        "particle_fragment",
                        vec![
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: view.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: records.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(&atlas),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 20,
                                resource: lightmap.as_entire_binding(),
                            },
                        ],
                    )
                } else {
                    (
                        &item_source,
                        "item_witness_vertex",
                        "item_fragment",
                        vec![
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: view.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&atlas),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 19,
                                resource: case.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 20,
                                resource: lightmap.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 21,
                                resource: atmosphere.as_entire_binding(),
                            },
                        ],
                    )
                };
                let pixels = gpu.render_srgb(
                    source,
                    vertex,
                    &[Draw {
                        fragment,
                        vertices: 0..6,
                        bindings: &bindings,
                        blend: None,
                        write_depth: true,
                    }],
                );
                let offset = (128 * 256 + 128) * 4;
                for channel in 0..3 {
                    let expected = f32::from(TEXEL[channel])
                        * TINT[channel]
                        * if lit { light[channel] } else { 1.0 };
                    assert!(
                        (f32::from(pixels[offset + channel]) - expected).abs() <= 2.0,
                        "{scenario}, particle={particle}, lit={lit}, block={block}, sky={sky}, channel={channel}: got {}, expected {expected}",
                        pixels[offset + channel],
                    );
                }
                assert_eq!(pixels[offset + 3], TEXEL[3]);
                if block == 0 && sky == 0 {
                    gpu_snapshot::save(
                        &format!(
                            "{scenario}-{}-{lit}",
                            if particle { "particle" } else { "item" }
                        ),
                        &pixels,
                    );
                }
            }
        }
    }
}

const ITEM_VERTEX: &str = r#"
@group(0) @binding(19) var<uniform> witness: array<vec4<f32>, 2>;
@vertex fn item_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    return item_vertex(VertexInput(
        vec3(corners[index], 0.5), vec2(0.5), vec3(0.0, 1.0, 0.0),
        vec4(1.0, 0.0, 0.0, 0.0), vec4(0.0, 1.0, 0.0, 0.0), vec4(0.0, 0.0, 1.0, 0.0),
        vec4(0u, u32(witness[0].x), u32(witness[0].y), 0u), 0u, witness[1],
    ));
}
"#;

const PARTICLE_VERTEX: &str = r#"
@vertex fn particle_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    return particle_vertex(index, 0u);
}
"#;
