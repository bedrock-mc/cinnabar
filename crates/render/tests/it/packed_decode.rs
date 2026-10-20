//! Read production packed decoders and vertices without depending on their source spelling.
use crate::{gpu_snapshot, shader_source};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::Gpu;
use meshing::{Face, PackedLiquidQuad, PackedModelRef, PackedQuad};

const OUTPUT_BINDING: u32 = 28;
const LIGHTMAP_BINDING: u32 = 27;
const CUBE_SHADER: &str = include_str!("../../src/chunk.wesl");
const MODEL_SHADER: &str = include_str!("../../src/model.wesl");
const LIQUID_SHADER: &str = include_str!("../../src/liquid.wesl");

/// Composes a production module with a compute witness and single-group readback bindings.
fn source(shader: &str, witness: &str) -> String {
    let production = shader_source::standalone(shader, &["OPAQUE_OVERDRAW"]).replace(
        "@group(1) @binding(0)",
        &format!("@group(0) @binding({LIGHTMAP_BINDING})"),
    );
    format!(
        "{production}\n@group(0) @binding({OUTPUT_BINDING}) var<storage, read_write> results: array<vec4<u32>>;\n{witness}"
    )
}

#[test]
fn packed_decoder_witnesses_validate() {
    for (shader, witness) in [
        (CUBE_SHADER, CUBE_WITNESS),
        (MODEL_SHADER, MODEL_WITNESS),
        (LIQUID_SHADER, LIQUID_WITNESS),
    ] {
        let module = naga::front::wgsl::parse_str(&source(shader, witness)).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn packed_cube_vertex_preserves_section_biome_and_light_addresses() {
    let Some(gpu) = Gpu::for_fixture("packed cube decoder") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let quad = PackedQuad::new([3, 4, 5], Face::PositiveY, 7, 9, 1);
    let quads = gpu.words(
        &[
            PackedQuad::new([0; 3], Face::NegativeY, 1, 1, 0).words(),
            quad.words(),
        ]
        .concat(),
        storage,
    );
    let origins = gpu.words(
        &[0; 8]
            .into_iter()
            .chain([16, 64, (-16_i32) as u32, 37, 1, 2, 0, 0])
            .collect::<Vec<_>>(),
        storage,
    );
    let streams = gpu.words(&[0, 0, 0, 0, 0x0234_0123, 0x0846_0345], storage);
    let materials = gpu.words(&[0, 0, assets::NO_ANIMATION, 0, 0, 0].repeat(2), storage);
    let view = gpu.buffer(&gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO), uniform);
    let empty = gpu.words(&[0; 8], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let lightmap = gpu.lightmap(bytemuck::cast_slice(
        &render_model::LightmapInputs::default().build(),
    ));
    let bindings = [
        (0, view.as_entire_binding()),
        (1, quads.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (9, empty.as_entire_binding()),
        (10, empty.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (13, streams.as_entire_binding()),
        (LIGHTMAP_BINDING, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let values = gpu.compute_readback(
        &source(CUBE_SHADER, CUBE_WITNESS),
        &bindings,
        OUTPUT_BINDING,
        5,
        1,
    );
    assert_eq!(values[0], [3, 4, 5, Face::PositiveY as u32]);
    assert_eq!(values[1], [7, 9, 2, 1]);
    assert_eq!(values[2], [16, 64, (-16_i32) as u32, 37]);
    assert_eq!(values[3], [0x0123, 0x0234, 0x0345, 0x0846]);
    assert_eq!(
        values[4], [37; 4],
        "every vertex keeps the selected section's biome record"
    );
}

#[test]
fn packed_model_rejects_masked_padded_and_invalid_references_before_vertex_use() {
    let Some(gpu) = Gpu::for_fixture("packed model decoder") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let origins = gpu.words(&[16, 64, (-16_i32) as u32, 37, 0, 0, 0, 0], storage);
    let materials = gpu.words(
        &[
            [0, 0, assets::NO_ANIMATION, 0, 0, 0],
            [0, 0, assets::NO_ANIMATION, 2, 1, 0],
            [0, 0, assets::NO_ANIMATION, 0, 0, 0],
        ]
        .concat(),
        storage,
    );
    let view = gpu.buffer(&gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO), uniform);
    let empty = gpu.words(&[0; 8], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let lightmap = gpu.lightmap(bytemuck::cast_slice(
        &render_model::LightmapInputs::default().build(),
    ));
    let source = source(MODEL_SHADER, MODEL_WITNESS);
    let flags = assets::MODEL_QUAD_FLAG_TWO_SIDED | 2;
    let mut template = vec![1, 0, 1, 0];
    template.extend([0; 10]);
    template.extend([1, flags]);
    let packed_position = 3 | (4 << 4) | (5 << 8);
    let mut geometry = vec![1, 0, 0, 0];
    geometry.extend(PackedModelRef::new(packed_position, 0, 4, 1).words());
    geometry.extend([0x0234_0123, 0x0846_0345]);
    let mut cases = vec![("visible", geometry.clone(), template.clone(), 0, true)];
    for (name, word, value) in [
        ("masked", 7, 0),
        ("padded", 1, 1),
        ("quad overflow", 1, 32),
        ("reference overflow", 0, 0x4000_0000),
        ("missing reference", 0, 3),
        ("missing template", 5, 1),
        ("missing lighting", 6, 5),
    ] {
        let mut changed = geometry.clone();
        changed[word] = value;
        cases.push((name, changed, template.clone(), 0, false));
    }
    for (name, word, value) in [
        ("empty template", 2, 0),
        ("missing quad", 1, 1),
        ("descriptor overflow", 0, 100),
    ] {
        let mut changed = template.clone();
        changed[word] = value;
        cases.push((name, geometry.clone(), changed, 0, false));
    }
    cases.push(("short template", geometry.clone(), vec![0], 0, false));
    cases.push((
        "instance overflow",
        geometry.clone(),
        template.clone(),
        0x8000_0000,
        false,
    ));
    cases.push(("missing draw", geometry, template, 5, false));
    for (name, geometry, template, instance, valid) in cases {
        let geometry = gpu.words(&geometry, storage);
        let template = gpu.words(&template, storage);
        let query = gpu.words(&[instance, 0, 0, 0], uniform);
        let bindings = [
            (0, view.as_entire_binding()),
            (2, origins.as_entire_binding()),
            (3, materials.as_entire_binding()),
            (9, empty.as_entire_binding()),
            (10, empty.as_entire_binding()),
            (11, clock.as_entire_binding()),
            (12, template.as_entire_binding()),
            (13, geometry.as_entire_binding()),
            (LIGHTMAP_BINDING, lightmap.as_entire_binding()),
            (29, query.as_entire_binding()),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
        let values = gpu.compute_readback(&source, &bindings, OUTPUT_BINDING, 7, 1);
        assert_eq!(values[0][0], u32::from(valid), "{name}");
        if valid {
            assert_eq!(values[0], [1, 1, 2, flags], "material and authored flags");
            assert_eq!(values[1], [3, 4, 5, 37]);
            assert_eq!(values[2], [0x0123, 0x0234, 0x0345, 0x0846]);
            for vertex in &values[3..] {
                assert_eq!(*vertex, [1, 1, 37, 0]);
            }
        } else {
            let clips = values[3..]
                .iter()
                .map(|vertex| vertex.map(f32::from_bits))
                .collect::<Vec<_>>();
            assert!(
                (0..6).any(|plane| clips.iter().all(|clip| match plane {
                    0 => clip[0] < -clip[3],
                    1 => clip[0] > clip[3],
                    2 => clip[1] < -clip[3],
                    3 => clip[1] > clip[3],
                    4 => clip[2] < 0.0,
                    _ => clip[2] > clip[3],
                })),
                "{name}: rejected quad must lie outside a common clip plane: {clips:?}"
            );
        }
    }
}

#[test]
fn packed_liquid_vertices_mask_route_flags_and_use_the_records_section_tint() {
    let Some(gpu) = Gpu::for_fixture("packed liquid decoder") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let record = liquid_biome_record();
    let expected_tint = liquid_expected_tint(&record);
    assert!(
        expected_tint[0] > 0.0 && expected_tint[2] > 0.0,
        "fixture must blend the edge biomes"
    );
    let biome_offset = 17;
    let mut biome_words = vec![0; biome_offset];
    biome_words.extend(record.words());
    let biomes = gpu.words(&biome_words, storage);
    let mut tints = vec![0; 2 * (8 + assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT * 4)];
    let second = tints.len() / 2;
    tints[5] = 0xff;
    tints[7] = 1.0_f32.to_bits();
    tints[second + 5] = 0xff0000;
    tints[second + 7] = 0.625_f32.to_bits();
    let tints = gpu.words(&tints, storage);
    let origins = gpu.words(
        &[0; 8]
            .into_iter()
            .chain([16, 64, (-16_i32) as u32, biome_offset as u32, 0, 0, 1, 20])
            .collect::<Vec<_>>(),
        storage,
    );
    let mut words = vec![0; 56];
    let route_flags = [
        meshing::liquid::LIQUID_DEPTH_WRITE_BIT,
        meshing::liquid::LIQUID_TOP_INSET_BIT,
        meshing::liquid::LIQUID_TWO_SIDED_BIT,
    ];
    for case in 0..8 {
        let mut packed = PackedLiquidQuad::try_pack(
            [0, 4, 5],
            Face::PositiveY,
            [0, 64, 128, 255],
            1,
            0,
            [0; 2],
            false,
        )
        .unwrap()
        .words();
        for (bit, flag) in route_flags.into_iter().enumerate() {
            if case & (1 << bit) != 0 {
                packed[2] |= flag;
            }
        }
        packed[3] = 1;
        words[(case + 1) * 4..][..4].copy_from_slice(&packed);
        words[40 + case * 2..][..2].copy_from_slice(&[0x0234_0123, 0x0846_0345]);
    }
    let geometry = gpu.words(&words, storage);
    let materials = gpu.words(
        &[
            [0, 0, assets::NO_ANIMATION, 0, 0, 0],
            [0, 0, assets::NO_ANIMATION, 2, 1, 0],
            [0, 0, assets::NO_ANIMATION, 0, 0, 0],
        ]
        .concat(),
        storage,
    );
    let view = gpu.buffer(&gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO), uniform);
    let empty = gpu.words(&[0; 8], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let atmosphere = gpu.words(&[0; 36], uniform);
    let tables = gpu.words(&meshing::biome_lattice::query_table_words(), uniform);
    let lightmap = gpu.lightmap(bytemuck::cast_slice(
        &render_model::LightmapInputs::default().build(),
    ));
    let bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (7, biomes.as_entire_binding()),
        (8, tints.as_entire_binding()),
        (9, empty.as_entire_binding()),
        (10, empty.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (15, atmosphere.as_entire_binding()),
        (
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            tables.as_entire_binding(),
        ),
        (LIGHTMAP_BINDING, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let values = gpu.compute_readback(
        &source(LIQUID_SHADER, LIQUID_WITNESS),
        &bindings,
        OUTPUT_BINDING,
        8 * 4 * 4,
        32,
    );
    for case in 0..8 {
        for corner in 0..4 {
            let data = &values[(case * 4 + corner) * 4..];
            assert_eq!(
                data[0],
                [0, 4, 5, 2],
                "flags must not change material selection"
            );
            assert_eq!(data[1], [16, 64, (-16_i32) as u32, biome_offset as u32]);
            assert_eq!(
                data[2],
                [
                    [0x0123, 0x0234, 0x0345, 0x0846][corner],
                    (case & 1) as u32,
                    u32::from(case & 4 != 0),
                    0
                ]
            );
            let actual = data[3].map(f32::from_bits);
            assert!(
                actual
                    .into_iter()
                    .zip(expected_tint)
                    .all(|(actual, expected)| (actual - expected).abs() < 1.0e-5),
                "vertex tint uses the record's section and owning block for every corner: {actual:?} != {expected_tint:?}"
            );
        }
    }
}

/// Computes linear RGB and blended opacity at the liquid fixture's owning block.
fn liquid_expected_tint(record: &meshing::PackedBiomeRecord) -> [f32; 4] {
    let gamma = record.blend_samples([0, 4, 5]).unwrap().into_iter().fold(
        [0.0_f32; 4],
        |mut colour, sample| {
            let tint = if sample.tint_index == 1 {
                [0.0, 0.0, 1.0, 0.625]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            };
            for channel in 0..4 {
                colour[channel] += tint[channel] * sample.weight;
            }
            colour
        },
    );
    let [red, green, blue] = render_model::celestial::rgb_to_linear([gamma[0], gamma[1], gamma[2]]);
    [red, green, blue, gamma[3]]
}

#[test]
fn liquid_tint_oracle_converts_blended_rgb_without_changing_opacity() {
    let record = liquid_biome_record();
    let blue_gamma: f32 = record
        .blend_samples([0, 4, 5])
        .unwrap()
        .into_iter()
        .filter(|sample| sample.tint_index == 1)
        .map(|sample| sample.weight)
        .sum();
    assert!(blue_gamma > 0.0 && blue_gamma < 1.0);
    let expected = liquid_expected_tint(&record);
    assert!(
        expected[0] + 1.0e-4 < 1.0 - blue_gamma,
        "mixed red must be converted from gamma to linear"
    );
    assert!(
        expected[2] + 1.0e-4 < blue_gamma,
        "mixed blue must be converted from gamma to linear"
    );
    assert_eq!(expected[1], 0.0);
    assert!(
        (expected[3] - (1.0 - 0.375 * blue_gamma)).abs() < 1.0e-6,
        "RGB transfer must preserve blended opacity"
    );
}

/// Encodes a biome-one section whose absent neighbours contribute biome zero.
fn liquid_biome_record() -> meshing::PackedBiomeRecord {
    let storage =
        world::DecodedBiomeColumn::decode(0, 1, &[1, 2], &world::RawBiomeIds { default_biome: 0 })
            .storage(0)
            .unwrap();
    let record = meshing::PackedBiomeRecord::from_storage(&storage, |id| id);
    assert_eq!(record.uniform_tint_index(), None);
    record
}

const CUBE_WITNESS: &str = r#"
@compute @workgroup_size(1) fn witness() {
    let quad = cube_quad(1u, 1u);
    results[0] = vec4(vec3<u32>(quad.local_origin), quad.face);
    results[1] = vec4(u32(quad.width), u32(quad.height), quad.lighting_record_index, quad.selected_material_id);
    results[2] = bitcast<vec4<u32>>(quad.chunk_origin);
    for (var corner = 0u; corner < 4u; corner++) {
        results[3][corner] = cube_light_sample(quad.lighting_record_index, corner);
        results[4][corner] = sealed_cube_vertex(4u + corner, 1u, 0.0).biome_record;
    }
}
"#;

const MODEL_WITNESS: &str = r#"
@group(0) @binding(29) var<uniform> query: vec4<u32>;
@compute @workgroup_size(1) fn witness() {
    let quad = model_quad(query.x, 0u);
    results[0].x = u32(quad.valid);
    if (quad.valid) {
        results[0] = vec4(1u, quad.material_id, quad.selected_material_id, quad.quad_flags);
        results[1] = vec4(vec3<u32>(quad.block_position), u32(quad.chunk_origin.w));
        for (var corner = 0u; corner < 4u; corner++) {
            results[2][corner] = model_light_sample(quad, corner);
        }
    }
    for (var corner = 0u; corner < 4u; corner++) {
        let vertex = model_vertex(corner, query.x);
        if (quad.valid) { results[3u + corner] = vec4(vertex.visibility, vertex.biome_record, 0u); }
        else { results[3u + corner] = bitcast<vec4<u32>>(vertex.clip_position); }
    }
}
"#;

const LIQUID_WITNESS: &str = r#"
@compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
    let record = 1u + id.x / 4u;
    let corner = id.x % 4u;
    let decoded = liquid_record_corner(record, corner);
    let vertex = vertex_for_record(record, corner);
    let base = id.x * 4u;
    results[base] = vec4(decoded.block_coordinate, decoded.selected_material_id);
    results[base + 1u] = bitcast<vec4<u32>>(decoded.chunk_origin);
    results[base + 2u] = vec4(decoded.light_sample, vertex.depth_write_route, vertex.two_sided, 0u);
    results[base + 3u] = bitcast<vec4<u32>>(vertex.water_tint);
}
"#;
