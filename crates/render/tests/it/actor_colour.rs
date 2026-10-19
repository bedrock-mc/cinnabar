//! Numeric native entity witnesses through the real production actor fragment.
use crate::gpu_snapshot;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

const TEXELS: [[u8; 4]; 10] = [
    [128, 192, 64, 255],
    [128, 192, 64, 128],
    [64, 96, 224, 128],
    [224, 96, 32, 64],
    [128, 192, 64, 0],
    [128, 192, 64, 25],
    [128, 192, 64, 26],
    [255, 255, 255, 255],
    [204, 204, 204, 255],
    [51, 204, 255, 255],
];

/// Original two-dimensional foil pixels distinguish rotation, origin and repeat wrap.
const FOIL_PATTERN: [[u8; 4]; 8] = [
    [40, 30, 10, 255],
    [100, 170, 40, 255],
    [20, 80, 180, 255],
    [200, 40, 90, 255],
    [50, 120, 10, 255],
    [130, 160, 170, 255],
    [40, 10, 80, 255],
    [180, 80, 20, 255],
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
    fog_amount: f32,
    sample: (u8, u8),
    gradient: bool,
    interpolate_normals: bool,
    material_word: u32,
    glint_texel: [u8; 4],
    item_glint_texel: [u8; 4],
    strength: f32,
    depth_probe: bool,
    foil_pattern: bool,
    uv: [f32; 2],
    offsets: [f32; 2],
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
            fog_amount: 0.0,
            sample: (2, 9),
            gradient: false,
            interpolate_normals: false,
            material_word: 0,
            glint_texel: [0; 4],
            item_glint_texel: [0; 4],
            strength: 1.0,
            depth_probe: false,
            foil_pattern: false,
            uv: [0.5; 2],
            offsets: [0.0; 2],
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
                tint: render_model::pack_overlay_rgba8([0.4, 0.7, 0.3, 1.0]),
                ..Default::default()
            },
            Case {
                name: "gamma dye mask",
                texture: 1,
                tint: render_model::pack_overlay_rgba8([0.4, 0.7, 0.3, 0.8]),
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
                overlay: render_model::pack_overlay_rgba8([0.8, 0.2, 0.1, 0.6]),
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
        if case.material[1] != 0
            || case.material_word & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG != 0
        {
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
    let enchanted = case.material_word & assets::EntityRenderMaterialState::KIND_MASK
        == assets::EntityRenderMaterial::Glint as u32;
    let legacy = case.material_word & render_model::LEGACY_COMBAT_MATERIAL_FLAG != 0;
    let foil: [[f32; 4]; 2] = std::array::from_fn(|pass| {
        let pixel = if case.foil_pattern && legacy {
            let uv = render_model::legacy_glint_uv(case.uv, case.offsets, pass);
            let x = (uv[0].rem_euclid(1.0) * 4.0).floor() as usize;
            let y = (uv[1].rem_euclid(1.0) * 2.0).floor() as usize;
            FOIL_PATTERN[y * 4 + x]
        } else if legacy {
            case.item_glint_texel
        } else {
            case.glint_texel
        };
        pixel.map(|byte| f32::from(byte) / 255.0)
    });
    for i in 0..3 {
        color[i] = color[i] * (1.0 - overlay[3]) + overlay[i] * overlay[3];
        if !case.unlit {
            color[i] *= shade * light[i];
        }
        let glint = if enchanted {
            let sum = (foil[0][i] + foil[1][i])
                * render_model::ActorGlint::TINT_RGB[i]
                * case.strength
                * light[i];
            sum * sum
        } else {
            0.0
        };
        if !legacy {
            color[i] += glint;
        }
        color[i] = color[i] * (1.0 - case.fog_amount) + fog[i] * case.fog_amount;
        if legacy {
            color[i] += glint;
        }
    }
    if legacy && enchanted {
        color[3] = (foil[0][0] + foil[1][0]) * case.strength;
    }
    if case.material_word
        & (render_model::LEGACY_COMBAT_MATERIAL_FLAG | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG)
        == (render_model::LEGACY_COMBAT_MATERIAL_FLAG
            | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG)
    {
        color[3] = 1.0;
    }
    color
}

#[test]
fn actual_actor_fragment_matches_native_colour_lightmap_and_material_order() {
    let Some(gpu) = Gpu::for_fixture("actual actor colour shader") else {
        return;
    };
    render_cases(&gpu, cases());
}

#[test]
fn dye_mask_pages_keep_zero_alpha_rgb_and_depth_in_both_profiles() {
    let Some(gpu) = Gpu::for_fixture("sheep and wolf dye-mask page contract") else {
        return;
    };
    let cases = [0, render_model::LEGACY_COMBAT_MATERIAL_FLAG]
        .into_iter()
        .flat_map(|profile| {
            [0, 1, 4].map(|texture| Case {
                name: "body dye mask alpha is not transparency",
                texture,
                material: [0, 1, 0, 0],
                material_word: profile,
                tint: render_model::pack_overlay_rgba8([0.2, 0.8, 0.4, 1.0]),
                depth_probe: true,
                ..Default::default()
            })
        })
        .collect();
    render_cases(&gpu, cases);
}

/// Creates an original one-pixel foil image with independent actor/item sampler contents.
fn glint_view(gpu: &Gpu, texel: [u8; 4], pattern: bool) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original constant foil texture"),
        size: wgpu::Extent3d {
            width: if pattern { 4 } else { 1 },
            height: if pattern { 2 } else { 1 },
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
        if pattern {
            bytemuck::cast_slice(&FOIL_PATTERN)
        } else {
            &texel
        },
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(if pattern { 16 } else { 4 }),
            rows_per_image: Some(if pattern { 2 } else { 1 }),
        },
        texture.size(),
    );
    texture.create_view(&Default::default())
}

#[test]
fn legacy_fragment_keeps_leather_dye_red_lighting_and_untinted_glint_after_fog() {
    let Some(gpu) = Gpu::for_fixture("legacy combat shader fixture") else {
        return;
    };
    let legacy_flag = render_model::LEGACY_COMBAT_MATERIAL_FLAG;
    let mask_flag = render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG;
    let glint_kind = assets::EntityRenderMaterial::Glint as u32;
    let overlay = render_model::pack_overlay_rgba8(render_api::BEDROCK_112.hurt_overlay);
    let mut cases = Vec::new();
    for profile in [
        render_api::CombatVisuals::Modern,
        render_api::CombatVisuals::Legacy112,
    ] {
        let flag = if profile == render_api::CombatVisuals::Legacy112 {
            legacy_flag
        } else {
            0
        };
        for normal in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            for texture in [8, 9] {
                cases.push(Case {
                    name: "flat red and directional brightening",
                    normal,
                    texture,
                    overlay,
                    material_word: flag,
                    ..Default::default()
                });
            }
            for texture in [0, 1, 3, 4] {
                cases.push(Case {
                    name: "leather dye before red",
                    normal,
                    texture,
                    overlay,
                    tint: render_model::pack_overlay_rgba8([0.2, 0.8, 0.4, 1.0]),
                    material_word: flag | mask_flag,
                    depth_probe: true,
                    ..Default::default()
                });
            }
        }
        for fog_amount in [0.0, 0.8, 1.0] {
            for overlay in [0, overlay] {
                for strength in [0.0, 0.5, 1.0] {
                    cases.push(Case {
                        name: "raster selection and summed foil after fog",
                        texture: 8,
                        overlay,
                        fog_amount,
                        strength,
                        material_word: flag | glint_kind,
                        glint_texel: [20, 80, 140, 255],
                        item_glint_texel: [70, 35, 210, 255],
                        depth_probe: true,
                        ..Default::default()
                    });
                }
            }
        }
        for texture in [0, 1, 3, 4] {
            cases.push(Case {
                name: "ordinary armour cutout",
                texture,
                overlay,
                depth_probe: true,
                material_word: flag
                    | assets::EntityRenderMaterial::Default.word(Some(
                        assets::EntityRenderMaterialState {
                            alpha_test: true,
                            cull: false,
                            ..Default::default()
                        },
                    )),
                ..Default::default()
            });
        }
        for texture in [0, 1, 3, 4] {
            cases.push(Case {
                name: "enchanted leather keeps mask and foil",
                texture,
                overlay,
                fog_amount: 0.8,
                tint: render_model::pack_overlay_rgba8([0.2, 0.8, 0.4, 1.0]),
                material_word: flag | mask_flag | glint_kind,
                glint_texel: [20, 80, 140, 255],
                item_glint_texel: [70, 35, 210, 255],
                depth_probe: true,
                ..Default::default()
            });
        }
        for texture in [0, 1, 3, 4] {
            cases.push(Case {
                name: "enchanted holes and depth",
                texture,
                overlay,
                fog_amount: 0.8,
                material_word: flag | glint_kind,
                glint_texel: [20, 80, 140, 255],
                item_glint_texel: [70, 35, 210, 255],
                depth_probe: true,
                ..Default::default()
            });
        }
    }
    for uv in [
        [13.0 / 64.0, 7.0 / 32.0],
        [35.0 / 64.0, 18.0 / 32.0],
        [0.72, 0.43],
    ] {
        cases.push(Case {
            name: "legacy UV rotation on a non-square foil",
            uv,
            offsets: [-0.37, 0.62],
            texture: 8,
            overlay,
            fog_amount: 0.2,
            material_word: legacy_flag | glint_kind,
            foil_pattern: true,
            ..Default::default()
        });
    }
    render_cases(&gpu, cases);
}

/// Compares production fragment arithmetic and depth against scalar expectations.
fn render_cases(gpu: &Gpu, cases: Vec<Case>) {
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
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        ..Default::default()
    });
    let actor = include_str!("../../src/actor.wesl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render_model::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    // Test-only resource remapping lets the shared single-group readback helper run
    // production actor_fragment unchanged, including its actual imported helpers.
    let source = shader_source::standalone(&format!("{actor}\n{VERTEX}"), &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
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

    let mut failures = Vec::new();
    for case in cases {
        let mut instance_words = [0.0; render_model::ACTOR_GPU_INSTANCE_WORDS];
        instance_words[30] = case.offsets[0];
        instance_words[31] = case.offsets[1];
        instance_words[32] = case.strength;
        instance_words[33..35].fill(1.0);
        let instance = gpu.buffer(&instance_words, wgpu::BufferUsages::STORAGE);
        let glint = glint_view(gpu, case.glint_texel, false);
        let item_glint = glint_view(gpu, case.item_glint_texel, case.foil_pattern);
        let table = table(case);
        let lightmap = gpu.lightmap(&table);
        let material = gpu.buffer(
            &case.material.map(f32::from_bits),
            wgpu::BufferUsages::UNIFORM,
        );
        let light = if case.unlit {
            0
        } else {
            render_model::pack_actor_light(case.sample.0, case.sample.1)
        };
        let mut words = case.normal.to_vec();
        words.push(f32::from(case.interpolate_normals));
        words.extend([case.texture, case.tint, case.overlay, light].map(f32::from_bits));
        words.extend([
            case.fog_amount * 100.0,
            f32::from_bits(u32::from(case.material[2] != 0)),
            f32::from_bits(case.material_word),
            0.5,
        ]);
        words.extend([case.uv[0], case.uv[1], 0.0, 0.0]);
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
                binding: 14,
                resource: wgpu::BindingResource::TextureView(&item_glint),
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
        let mut behind_words = words.clone();
        behind_words[4] = f32::from_bits(7);
        behind_words[5] = 0.0;
        behind_words[6] = f32::from_bits(render_model::pack_overlay_rgba8([0.0, 0.0, 1.0, 1.0]));
        behind_words[7] = 0.0;
        behind_words[8] = 0.0;
        behind_words[9] = 0.0;
        behind_words[10] = 0.0;
        behind_words[11] = 0.25;
        let behind_fixture = gpu.buffer(&behind_words, wgpu::BufferUsages::UNIFORM);
        let mut behind_bindings = bindings.to_vec();
        behind_bindings
            .iter_mut()
            .find(|entry| entry.binding == 22)
            .unwrap()
            .resource = behind_fixture.as_entire_binding();
        let mut draws = vec![Draw {
            fragment: "actor_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }];
        if case.depth_probe {
            draws.push(Draw {
                fragment: "actor_fragment",
                vertices: 0..3,
                bindings: &behind_bindings,
                blend: None,
                write_depth: true,
            });
        }
        let mask_flag = render_model::LEGACY_COMBAT_MATERIAL_FLAG
            | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG;
        let mut state = gpu_snapshot::RasterState::default();
        if case.material_word & mask_flag == mask_flag {
            state.write_mask = wgpu::ColorWrites::COLOR;
        }
        let pixels = gpu.render_srgb_with_state(&source, "actor_witness_vertex", &draws, state);
        let offset = (128 * 256 + 128) * 4;
        let alpha = f32::from(TEXELS[case.texture as usize][3]) / 255.0;
        let legacy = case.material_word & render_model::LEGACY_COMBAT_MATERIAL_FLAG != 0;
        let mask = case.material_word & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG != 0;
        let enchanted = case.material_word & assets::EntityRenderMaterialState::KIND_MASK
            == assets::EntityRenderMaterial::Glint as u32;
        let authored_cutout = case.material_word
            & (assets::EntityRenderMaterialState::AUTHORED
                | assets::EntityRenderMaterialState::ALPHA_TEST)
            == (assets::EntityRenderMaterialState::AUTHORED
                | assets::EntityRenderMaterialState::ALPHA_TEST);
        let discard =
            (authored_cutout && !(legacy && mask) && alpha < assets::ENTITY_ALPHA_TEST_THRESHOLD)
                || (legacy && mask && alpha == 0.0)
                || (enchanted && !(legacy && mask) && alpha < assets::ENTITY_ALPHA_TEST_THRESHOLD)
                || (case.material[1] == 0
                    && !mask
                    && case.material[2] == 0
                    && ((case.material[0] == 0 && alpha < 0.1)
                        || (case.material[0] == 1 && alpha == 0.0)));
        let expected = if discard {
            if case.depth_probe {
                [0.0, 0.0, 1.0, 1.0]
            } else {
                let [r, g, b] = [0.12, 0.18, 0.25].map(gamma);
                [r, g, b, 1.0]
            }
        } else {
            expected(case, &table, fog)
        };
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
struct ActorWitnessCase { normal: vec4<f32>, words: vec4<u32>, distance_multi: vec4<f32>, glint_uv: vec4<f32> }
@group(0) @binding(22) var<uniform> actor_witness: ActorWitnessCase;
@vertex fn actor_witness_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: VertexOutput;
    out.skin_info.z = select(0xffffffffu, 0u, (bitcast<u32>(actor_witness.distance_multi.z) & ACTOR_MATERIAL_KIND_MASK) == ACTOR_MATERIAL_GLINT);
    out.position = vec4(corners[index], actor_witness.distance_multi.w, 1.0);
    out.uv = vec4(actor_witness.glint_uv.xy, vec2(0.0));
    out.skin_info.x = actor_witness.words.x;
    out.skin_info.y = 1u;
    let normals = array(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0));
    out.world_normal = select(actor_witness.normal.xyz, normals[index], actor_witness.normal.w != 0.0);
    out.back_uv = actor_witness.glint_uv.xy;
    out.tint = actor_witness.words.y;
    out.overlay = unpack4x8unorm(actor_witness.words.z);
    out.skin_info.w = 0u;
    out.light = actor_witness.words.w;
    out.world_position = vec3(actor_witness.distance_multi.x, 0.0, 0.0);
    out.multitexture_layers = select(vec2(0xffffffffu), vec2(2u, 3u), bitcast<u32>(actor_witness.distance_multi.y) != 0u);
    out.material_parameters.z = bitcast<u32>(actor_witness.distance_multi.z);
    if ((out.material_parameters.z & ACTOR_MATERIAL_KIND_MASK) == ACTOR_MATERIAL_GLINT) { out.multitexture_layers.x = 0u; }
    out.native_lighting = actor_lighting(out.light, out.world_normal, out.overlay.a);
    return out;
}
"#;
