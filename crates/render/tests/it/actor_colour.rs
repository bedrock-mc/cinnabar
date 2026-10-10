//! Numeric native entity witnesses through the real production actor fragment.
use crate::gpu_snapshot;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

const TEXELS: [[u8; 4]; 8] = [
    [128, 192, 64, 255],
    [128, 192, 64, 128],
    [64, 96, 224, 128],
    [224, 96, 32, 64],
    [128, 192, 64, 0],
    [128, 192, 64, 25],
    [128, 192, 64, 26],
    [255, 255, 255, 255],
];

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    normal: [f32; 3],
    light: [f32; 3],
    texture: u32,
    tint: u32,
    overlay: u32,
    material: [u32; 4],
    unlit: bool,
    emissive: bool,
    fog_amount: f32,
    sample: (u8, u8),
    gradient: bool,
    interpolate_normals: bool,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            name: "ordinary",
            normal: [0.0, 1.0, 0.0],
            light: [1.0; 3],
            texture: 0,
            tint: 0,
            overlay: 0,
            material: [0; 4],
            unlit: false,
            emissive: false,
            fog_amount: 0.0,
            sample: (2, 9),
            gradient: false,
            interpolate_normals: false,
        }
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for light in [[1.0; 3], [0.13, 0.15, 0.25]] {
        for (name, normal) in [
            ("up", [0.0, 1.0, 0.0]),
            ("down", [0.0, -1.0, 0.0]),
            ("x", [1.0, 0.0, 0.0]),
            ("z", [0.0, 0.0, 1.0]),
            ("diagonal", [1.0, 2.0, 3.0].map(|n| n / 14.0_f32.sqrt())),
        ] {
            cases.push(Case {
                name,
                normal,
                light,
                ..Default::default()
            });
        }
        for case in [
            Case {
                name: "white skin",
                texture: 7,
                ..Default::default()
            },
            Case {
                name: "gamma dye",
                tint: render::pack_overlay_rgba8([0.4, 0.7, 0.3, 1.0]),
                ..Default::default()
            },
            Case {
                name: "gamma dye mask",
                texture: 1,
                tint: render::pack_overlay_rgba8([0.4, 0.7, 0.3, 0.8]),
                material: [0, 1, 0, 0],
                ..Default::default()
            },
            Case {
                name: "gamma multitexture",
                texture: 1,
                material: [0, 0, 1, 0],
                ..Default::default()
            },
            Case {
                name: "overlay before light",
                overlay: render::pack_overlay_rgba8([0.8, 0.2, 0.1, 0.6]),
                ..Default::default()
            },
            Case {
                name: "gamma fog",
                fog_amount: 0.4,
                ..Default::default()
            },
            Case {
                name: "unlit",
                normal: [0.0, -1.0, 0.0],
                unlit: true,
                ..Default::default()
            },
            Case {
                name: "vertex lighting interpolation",
                interpolate_normals: true,
                ..Default::default()
            },
        ] {
            cases.push(Case { light, ..case });
        }
    }
    for (name, texture, class) in [
        ("opaque zero alpha", 4, 0),
        ("opaque below cutoff", 5, 0),
        ("opaque above cutoff", 6, 0),
        ("transparent zero alpha", 4, 1),
        ("transparent low alpha", 5, 1),
        ("transparent half alpha", 1, 1),
    ] {
        cases.push(Case {
            name,
            texture,
            material: [class, 0, 0, 0],
            ..Default::default()
        });
    }
    for sample in [(0, 0), (1, 1), (7, 12), (15, 15)] {
        cases.push(Case {
            name: "native lightmap bilinear",
            sample,
            gradient: true,
            ..Default::default()
        });
    }
    cases
}

fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn unpack(word: u32) -> [f32; 4] {
    std::array::from_fn(|i| ((word >> (i * 8)) & 255) as f32 / 255.0)
}

fn table(case: Case) -> Vec<f32> {
    (0..256)
        .flat_map(|index| {
            let rgb = if case.gradient {
                let block = (index & 15) as f32;
                let sky = (index >> 4) as f32;
                [
                    0.01 + block / 20.0,
                    0.02 + sky / 20.0,
                    0.03 + block * sky / 320.0,
                ]
            } else {
                case.light
            };
            [rgb[0], rgb[1], rgb[2], 1.0]
        })
        .collect()
}

fn sampled_light(case: Case, table: &[f32]) -> [f32; 3] {
    let (block, sky) = case.sample;
    let lower_block = block.saturating_sub(1);
    let lower_sky = sky.saturating_sub(1);
    std::array::from_fn(|channel| {
        [
            (block, sky),
            (lower_block, sky),
            (block, lower_sky),
            (lower_block, lower_sky),
        ]
        .into_iter()
        .map(|(block, sky)| {
            let index = usize::from(sky) * 16 + usize::from(block);
            (table[index * 4 + channel].clamp(0.0, 1.0) * 255.0).floor() / 255.0
        })
        .sum::<f32>()
            * 0.25
    })
}

fn expected(case: Case, table: &[f32], fog: [f32; 3]) -> [f32; 4] {
    let texel = TEXELS[case.texture as usize].map(|byte| f32::from(byte) / 255.0);
    let mut color = texel;
    if case.tint != 0 {
        let tint = unpack(case.tint);
        if case.material[1] != 0 {
            for i in 0..3 {
                color[i] *= 1.0 + (tint[i] - 1.0) * texel[3];
            }
            color[3] *= tint[3];
        } else if case.material[2] == 0 && color[3] > 0.99 {
            for i in 0..3 {
                color[i] *= tint[i];
            }
        }
    }
    if case.material[2] != 0 {
        for overlay in [TEXELS[2], TEXELS[3]] {
            let overlay = overlay.map(|byte| f32::from(byte) / 255.0);
            for i in 0..3 {
                color[i] = color[i] * (1.0 - overlay[3]) + overlay[i] * overlay[3];
            }
        }
    }
    let overlay = unpack(case.overlay);
    // Native Actor FancyOn: vertex-stage world-normal polynomial, with overlay bonus.
    let shade = if case.interpolate_normals {
        // Centre readback is half a pixel right/below the geometric triangle centre.
        [
            ([0.0, 1.0, 0.0], 0.5),
            ([1.0, 0.0, 0.0], 0.25 + 0.5 / 512.0),
            ([0.0, -1.0, 0.0], 0.25 - 0.5 / 512.0),
        ]
        .into_iter()
        .map(|(normal, weight)| render_api::fancy_actor_shade(normal, overlay[3]) * weight)
        .sum()
    } else {
        render_api::fancy_actor_shade(case.normal, overlay[3])
    };
    let light = sampled_light(case, table);
    for i in 0..3 {
        color[i] = color[i] * (1.0 - overlay[3]) + overlay[i] * overlay[3];
        if !case.unlit {
            let lighting = shade * light[i];
            color[i] *= if case.emissive {
                1.0 + (lighting - 1.0) * texel[3]
            } else {
                lighting
            };
        }
        color[i] = color[i] * (1.0 - case.fog_amount) + fog[i] * case.fog_amount;
    }
    color
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn actual_actor_fragment_matches_native_colour_lightmap_and_material_order() {
    assert_actor_colors(cases(), false);
}

#[test]
fn msaa_actor_emissive_lighting_uses_authored_alpha_instead_of_coverage() {
    assert_actor_colors(
        vec![Case {
            name: "half-opacity emissive cutout with full coverage",
            texture: 1,
            light: [0.13, 0.15, 0.25],
            emissive: true,
            ..Default::default()
        }],
        true,
    );
}

/// Compares the production actor fragment with numeric lighting and opacity expectations.
fn assert_actor_colors(cases: Vec<Case>, coverage: bool) {
    let Some(gpu) = Gpu::for_fixture("actor color and MSAA emissive alpha") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("actor native colour witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: TEXELS.len() as u32,
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
        bytemuck::cast_slice(&TEXELS),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let skin = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let actor = include_str!("../../src/actor.wgsl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    // Test-only resource remapping lets the shared single-group readback helper run
    // production actor_fragment unchanged, including its actual imported helpers.
    let defs: &[&str] = if coverage { &["ALPHA_TO_COVERAGE"] } else { &[] };
    let actor = shader_source::standalone(&actor, defs)
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    let source = format!("{actor}\n{}", crate::material_shader::source(VERTEX));
    let view = gpu.buffer(
        &gpu_snapshot::view(bevy::math::Mat4::IDENTITY, bevy::math::Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let fog = [0.5, 0.6, 0.7];
    let mut atmosphere = [0.0; 32];
    atmosphere[16..19].copy_from_slice(&fog.map(linear));
    atmosphere[20] = 100.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    // The fragment reads instance words and the glint image only for glint materials.
    let instance = gpu.buffer(
        &[0.0; render::ACTOR_GPU_INSTANCE_WORDS],
        wgpu::BufferUsages::STORAGE,
    );
    let glint = gpu.blank_texture_view();
    let mut failures = Vec::new();
    for case in cases {
        let table = table(case);
        let lightmap = gpu.buffer(&table, wgpu::BufferUsages::UNIFORM);
        let material = gpu.buffer(
            &case.material.map(f32::from_bits),
            wgpu::BufferUsages::UNIFORM,
        );
        let light = if case.unlit {
            0
        } else {
            render::pack_actor_light(case.sample.0, case.sample.1)
        };
        let mut words = case.normal.to_vec();
        words.push(f32::from(case.interpolate_normals));
        words.extend([case.texture, case.tint, case.overlay, light].map(f32::from_bits));
        words.extend([
            case.fog_amount * 100.0,
            f32::from_bits(u32::from(case.material[2] != 0)),
            f32::from(case.emissive),
            0.0,
        ]);
        let fixture = gpu.buffer(&words, wgpu::BufferUsages::UNIFORM);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instance.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&skin),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: material.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&skin),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&skin),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(&skin),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(&glint),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 20,
                resource: lightmap.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 21,
                resource: atmosphere.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 22,
                resource: fixture.as_entire_binding(),
            },
        ];
        let draws = [Draw {
            fragment: "actor_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }];
        let pixels = if coverage {
            gpu.render_with_samples(&source, "actor_witness_vertex", &draws, 4)
        } else {
            gpu.render_srgb(&source, "actor_witness_vertex", &draws)
        };
        let offset = (128 * 256 + 128) * 4;
        let alpha = f32::from(TEXELS[case.texture as usize][3]) / 255.0;
        let discard = case.material[1] == 0
            && case.material[2] == 0
            && ((case.material[0] == 0 && alpha < 0.1) || (case.material[0] == 1 && alpha == 0.0));
        let mut expected = if discard {
            let [r, g, b] = [0.12, 0.18, 0.25].map(gamma);
            [r, g, b, 1.0]
        } else {
            expected(case, &table, fog)
        };
        if coverage {
            for channel in &mut expected[..3] {
                *channel = linear(*channel);
            }
            expected[3] = 1.0;
        }
        for channel in 0..4 {
            let target = expected[channel].clamp(0.0, 1.0) * 255.0;
            if (f32::from(pixels[offset + channel]) - target).abs() > 2.0 {
                failures.push(format!(
                    "{}, light {:?}, sample {:?}, channel {channel}: got {}, native {target}",
                    case.name,
                    case.light,
                    case.sample,
                    pixels[offset + channel]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const VERTEX: &str = r#"
struct ActorWitnessCase { normal: vec4<f32>, words: vec4<u32>, distance_multi: vec4<f32> }
@group(0) @binding(22) var<uniform> actor_witness: ActorWitnessCase;
@vertex fn actor_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: VertexOutput;
    out.position = vec4(corners[index], 0.5, 1.0);
    out.uv = vec2(0.5);
    out.skin_layer = actor_witness.words.x;
    out.valid = 1u;
    out.material = select(0u, ACTOR_MATERIAL_AUTHORED_FLAG | ACTOR_MATERIAL_EMISSIVE_FLAG | ACTOR_MATERIAL_ALPHA_TEST_FLAG, actor_witness.distance_multi.z != 0.0);
    let normals = array(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0));
    out.world_normal = select(actor_witness.normal.xyz, normals[index], actor_witness.normal.w != 0.0);
    out.back_uv = vec2(0.5);
    out.tint = actor_witness.words.y;
    out.overlay = unpack4x8unorm(actor_witness.words.z);
    out.uv_wrap = 0u;
    out.light = actor_witness.words.w;
    out.world_position = vec3(actor_witness.distance_multi.x, 0.0, 0.0);
    out.multitexture_layers = select(vec2(0xffffffffu), vec2(2u, 3u), bitcast<u32>(actor_witness.distance_multi.y) != 0u);
    out.native_lighting = actor_lighting(out.light, out.world_normal, out.overlay.a);
    return out;
}
"#;
