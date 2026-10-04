use std::mem::size_of;

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, Material, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    encode_blob,
};
use meshing::{
    BlockClassifier, Face, MeshLightSample, PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, PackedQuad,
    PackedQuadLighting, bake_quad_lighting, bake_quad_lighting_with_sampler,
    bake_template_lighting, bake_template_lighting_with_sampler, mesh_dependency_mask,
};
use world::{MeshNeighbourhood, RawBlockIds, SubChunk};

const AIR: u32 = 0;
const SOLID: u32 = 1;
const MODEL: u32 = 2;
const LIQUID: u32 = 3;
const LEAF: u32 = 4;
const EMITTING: u32 = 5;
const FULL_HEIGHT_SNOW: u32 = 6;
const TEST_HASH_BASE: u32 = 0x10000;

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn packed_storage(palette: &[u32], placements: &[[u8; 3]]) -> Vec<u8> {
    let mut words = vec![0_u32; 128];
    for &[x, y, z] in placements {
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        words[linear / 32] |= 1 << (linear % 32);
    }
    let mut bytes = vec![3];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(zig_zag_i32(palette.len() as i32));
    for &value in palette {
        bytes.extend(zig_zag_i32(value as i32));
    }
    bytes
}

fn blocks(placements: &[[u8; 3]]) -> SubChunk {
    let mut bytes = vec![9, 1, 0];
    bytes.extend(packed_storage(&[AIR, SOLID], placements));
    SubChunk::decode(&bytes, &RawBlockIds { air: AIR })
}

fn uniform_storage(runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

fn layered_uniform(runtime_ids: &[u32]) -> SubChunk {
    let mut bytes = vec![9, runtime_ids.len() as u8, 0];
    for &runtime_id in runtime_ids {
        bytes.extend(uniform_storage(runtime_id));
    }
    SubChunk::decode(&bytes, &RawBlockIds { air: AIR })
}

fn full_corner() -> [[i16; 3]; 4] {
    [[256, 256, 256]; 4]
}

fn model_quad(face_flag: u32) -> ModelQuad {
    ModelQuad {
        positions: full_corner(),
        uvs: [[0; 2]; 4],
        material: 0,
        flags: face_flag,
    }
}

fn runtime_assets() -> RuntimeAssets {
    runtime_assets_with_model_geometry(
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 3,
            flags: 0,
        }],
        // up, east, north in deliberately non-enum order
        vec![model_quad(2), model_quad(4), model_quad(5)],
    )
}

#[test]
fn lily_pad_planes_repeat_own_cell_light_without_neighbor_ao() {
    let mut top = model_quad(2);
    top.positions = [[0, 4, 256], [256, 4, 256], [256, 4, 0], [0, 4, 0]];
    let mut bottom = top;
    bottom.positions.reverse();
    bottom.flags = 1;
    let assets = runtime_assets_with_model_geometry(
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 2,
            flags: assets::MODEL_TEMPLATE_FLAG_LILY_PAD,
        }],
        vec![top, bottom],
    );
    let block = [8, 8, 8];
    let center = layered_uniform(&[SOLID]);
    let sampler = |coordinate| {
        if coordinate == block {
            MeshLightSample::try_new(3, 7).unwrap()
        } else {
            MeshLightSample::FULL_BRIGHT
        }
    };
    for rotation in 0..4 {
        let lighting = bake_template_lighting_with_sampler(
            &BlockClassifier::new(AIR),
            &assets,
            NetworkIdMode::Sequential,
            &MeshNeighbourhood::new(&center),
            &sampler,
            block,
            0,
            rotation,
        )
        .unwrap();
        assert_eq!(lighting.len(), 2);
        for quad in lighting {
            assert_eq!(quad.samples(), [3 | (7 << 4); 4]);
        }
    }
}

fn runtime_assets_with_model_geometry(
    model_templates: Vec<ModelTemplate>,
    model_quads: Vec<ModelQuad>,
) -> RuntimeAssets {
    let textures = TextureArray {
        layers: 1,
        mips: [16_u32, 8, 4, 2, 1]
            .into_iter()
            .map(|size| TextureMip {
                size,
                rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    let mut visuals = vec![
        BlockVisual {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags: BlockFlags::AIR,
            kind: VisualKind::Invisible,
            support: assets::VisualSupport::Exact,
            contributor_role: ContributorRole::Air,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: assets::VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            support: assets::VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Liquid,
            support: assets::VisualSupport::Exact,
            contributor_role: ContributorRole::LiquidAdditional,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
    ];
    let mut leaf = visuals[SOLID as usize];
    leaf.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    visuals.push(leaf);
    let mut emitting = leaf;
    emitting.flags = BlockFlags::CUBE_GEOMETRY;
    visuals.push(emitting);
    let mut full_height_snow = visuals[SOLID as usize];
    full_height_snow.variant = assets::BLOCK_VISUAL_VARIANT_TOP_SNOW;
    visuals.push(full_height_snow);
    let mut light_properties = vec![assets::LightProperties::default(); visuals.len()];
    light_properties[EMITTING as usize] = assets::LightProperties::new(9, 0).unwrap();
    let hashed = (0..visuals.len() as u32)
        .map(|id| (TEST_HASH_BASE + id, id))
        .collect();
    let compiled = CompiledAssets {
        visuals: visuals.into_boxed_slice(),
        hashed,
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }]
        .into_boxed_slice(),
        light_properties: light_properties.into_boxed_slice(),
        model_templates: model_templates.into_boxed_slice(),
        model_quads: model_quads.into_boxed_slice(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(textures)].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).expect("encode lighting assets"))
        .expect("decode lighting assets")
}

include!("lighting/leaf_shade.rs");
include!("lighting/snow_solid_render.rs");

fn fixture() -> (RuntimeAssets, SubChunk) {
    // At the high corner of block 8,8,8, the up face sees both planar sides,
    // while the east face sees only their shared +X/+Y side.
    (runtime_assets(), blocks(&[[8, 8, 8], [9, 9, 8], [8, 9, 9]]))
}

#[test]
fn face_specific_ao_differs_at_shared_corner() {
    let (assets, center) = fixture();
    let neighbourhood = MeshNeighbourhood::new(&center);
    let classifier = BlockClassifier::new(AIR);
    let up = bake_quad_lighting(
        &classifier,
        &assets,
        NetworkIdMode::Sequential,
        &neighbourhood,
        [8, 8, 8],
        Face::PositiveY,
        full_corner(),
    );
    let east = bake_quad_lighting(
        &classifier,
        &assets,
        NetworkIdMode::Sequential,
        &neighbourhood,
        [8, 8, 8],
        Face::PositiveX,
        full_corner(),
    );

    assert_ne!((up.samples()[0] >> 8) & 0x3, (east.samples()[0] >> 8) & 0x3);
}

#[test]
fn phase26_light_defaults_are_explicit() {
    let (assets, center) = fixture();
    let lighting = bake_quad_lighting(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        [8, 8, 8],
        Face::PositiveY,
        full_corner(),
    );

    assert_eq!(PHASE26_BLOCK_LIGHT, 0);
    assert_eq!(PHASE26_SKY_LIGHT, 15);
    for sample in lighting.samples() {
        assert_eq!(sample & 0x000f, u16::from(PHASE26_BLOCK_LIGHT));
        assert_eq!((sample >> 4) & 0x000f, u16::from(PHASE26_SKY_LIGHT));
        assert_eq!(sample & 0xfc00, 0, "reserved light bits must remain zero");
    }
    assert_eq!(size_of::<PackedQuadLighting>(), 8);
    assert_eq!(size_of::<PackedQuad>(), 8);
}

#[test]
fn mesh_light_samples_are_bounded_independent_nibbles() {
    assert_eq!(size_of::<MeshLightSample>(), 1);
    let sample = MeshLightSample::try_new(3, 12).expect("bounded light nibbles");
    assert_eq!(sample.block(), 3);
    assert_eq!(sample.sky(), 12);
    assert!(MeshLightSample::try_new(16, 0).is_none());
    assert!(MeshLightSample::try_new(0, 16).is_none());
}

#[test]
fn sampler_drives_block_and_sky_without_overwriting_ao() {
    let (assets, center) = fixture();
    let sampler = |coordinate: [i32; 3]| {
        let (block, sky) = match coordinate {
            [8, 9, 8] => (2, 12),
            [9, 9, 8] => (6, 8),
            [8, 9, 9] => (10, 4),
            [9, 9, 9] => (14, 0),
            _ => (0, 0),
        };
        MeshLightSample::try_new(block, sky).unwrap()
    };
    let lighting = bake_quad_lighting_with_sampler(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        &sampler,
        [8, 8, 8],
        Face::PositiveY,
        full_corner(),
    );

    for sample in lighting.samples() {
        assert_eq!(
            sample & 0x000f,
            10,
            "block light takes the unobstructed channel maximum"
        );
        assert_eq!(
            (sample >> 4) & 0x000f,
            12,
            "sky light takes its own channel maximum"
        );
        assert_eq!((sample >> 8) & 0x0003, 3, "AO remains geometric");
        assert_eq!(sample & 0xfc00, 0, "reserved bits remain zero");
    }
}

#[test]
fn faceless_cross_quad_samples_the_block_cell_without_ao() {
    let assets = runtime_assets_with_model_geometry(
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![model_quad(0)],
    );
    let center = blocks(&[]);
    let sampler = |coordinate: [i32; 3]| {
        if coordinate == [8, 8, 8] {
            MeshLightSample::try_new(7, 5).unwrap()
        } else {
            MeshLightSample::try_new(0, 0).unwrap()
        }
    };
    let lighting = bake_template_lighting_with_sampler(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        &sampler,
        [8, 8, 8],
        0,
        0,
    )
    .expect("known faceless template");

    assert_eq!(lighting, [PackedQuadLighting::new([0x0057; 4])]);
}

#[test]
fn template_quad_lighting_order() {
    let (assets, center) = fixture();
    let neighbourhood = MeshNeighbourhood::new(&center);
    let classifier = BlockClassifier::new(AIR);
    let actual = bake_template_lighting(
        &classifier,
        &assets,
        NetworkIdMode::Sequential,
        &neighbourhood,
        [8, 8, 8],
        0,
        0,
    )
    .expect("known template");
    let mut expected = [Face::PositiveY, Face::PositiveX, Face::NegativeZ].map(|face| {
        bake_quad_lighting(
            &classifier,
            &assets,
            NetworkIdMode::Sequential,
            &neighbourhood,
            [8, 8, 8],
            face,
            full_corner(),
        )
    });

    // The negative-Z fixture is inset at Z=1 and therefore uses the block plane.
    expected[2] = PackedQuadLighting::new([0x01f0; 4]);
    assert_eq!(actual, expected);
}

fn rotate_test_position([x, y, z]: [i16; 3], rotation: u32) -> [i16; 3] {
    match rotation & 3 {
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => [x, y, z],
    }
}

fn rotate_test_face(face: Face, rotation: u32) -> Face {
    match (face, rotation & 3) {
        (Face::NegativeX, 1) => Face::NegativeZ,
        (Face::PositiveX, 1) => Face::PositiveZ,
        (Face::NegativeZ, 1) => Face::PositiveX,
        (Face::PositiveZ, 1) => Face::NegativeX,
        (Face::NegativeX, 2) => Face::PositiveX,
        (Face::PositiveX, 2) => Face::NegativeX,
        (Face::NegativeZ, 2) => Face::PositiveZ,
        (Face::PositiveZ, 2) => Face::NegativeZ,
        (Face::NegativeX, 3) => Face::PositiveZ,
        (Face::PositiveX, 3) => Face::NegativeZ,
        (Face::NegativeZ, 3) => Face::NegativeX,
        (Face::PositiveZ, 3) => Face::PositiveX,
        (face, _) => face,
    }
}

#[test]
fn stair_rotation_bakes_ao_from_rotated_faces_and_positions_for_both_halves() {
    let shader = include_str!("../../render/src/model.wgsl");
    for clause in [
        "case 1u: { rotated = vec3(-centered.z, centered.y, centered.x); }",
        "case 2u: { rotated = vec3(-centered.x, centered.y, -centered.z); }",
        "case 3u: { rotated = vec3(centered.z, centered.y, -centered.x); }",
    ] {
        assert!(
            shader.contains(clause),
            "WGSL/CPU rotation contract drifted: {clause}"
        );
    }
    assert!(shader.contains("f32(packed_u16(template_quad_base + 6u, uv_component))"));
    assert!(shader.contains("f32(packed_u16(template_quad_base + 6u, uv_component + 1u))"));
    let lower = [[0, 0, 32], [0, 224, 32], [0, 224, 192], [0, 0, 192]];
    let upper = lower.map(|[x, y, z]| [x, 256 - y, z]);
    let assets = runtime_assets_with_model_geometry(
        vec![
            ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            },
            ModelTemplate {
                quad_start: 1,
                quad_count: 1,
                flags: 0,
            },
        ],
        vec![
            ModelQuad {
                positions: lower,
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 3,
            },
            ModelQuad {
                positions: upper,
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 3,
            },
        ],
    );
    let center = blocks(&[
        [7, 7, 8],
        [7, 9, 8],
        [8, 7, 7],
        [8, 9, 9],
        [9, 8, 7],
        [9, 8, 9],
    ]);
    let neighbourhood = MeshNeighbourhood::new(&center);
    let classifier = BlockClassifier::new(AIR);
    for (half, positions) in [lower, upper].into_iter().enumerate() {
        for rotation in 0..4 {
            let actual = bake_template_lighting(
                &classifier,
                &assets,
                NetworkIdMode::Sequential,
                &neighbourhood,
                [8, 8, 8],
                half as u32,
                rotation,
            )
            .expect("known asymmetric stair template");
            let expected = bake_quad_lighting(
                &classifier,
                &assets,
                NetworkIdMode::Sequential,
                &neighbourhood,
                [8, 8, 8],
                rotate_test_face(Face::NegativeX, rotation),
                positions.map(|position| rotate_test_position(position, rotation)),
            );
            assert_eq!(actual, [expected], "half={half} rotation={rotation}");
        }
    }
}

#[test]
fn dependency_mask_is_palette_native_and_asset_aware() {
    let assets = runtime_assets();
    let sub_chunk = layered_uniform(&[MODEL, LIQUID]);

    let mask = mesh_dependency_mask(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &sub_chunk,
    );

    assert!(mask.diagonal_ao);
    assert!(mask.liquid);
    assert_eq!(sub_chunk.storages().len(), 2);
    assert!(
        sub_chunk
            .storages()
            .iter()
            .all(|storage| storage.is_uniform())
    );
}

#[test]
fn corner_light_uses_independent_channel_maxima() {
    let assets = runtime_assets();
    let center = blocks(&[]);
    let sampler = |[x, _, z]: [i32; 3]| {
        MeshLightSample::try_new(if x == 9 { 15 } else { 0 }, if z == 9 { 15 } else { 0 }).unwrap()
    };
    let baked = bake_quad_lighting_with_sampler(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        &sampler,
        [8, 8, 8],
        Face::PositiveY,
        [[256, 256, 256]; 4],
    );
    assert_eq!(baked.samples(), [0x00ff; 4], "MAX per nibble");
}

#[test]
fn two_solid_sides_exclude_the_bright_diagonal() {
    let (assets, center) = fixture();
    let sampler = |p| MeshLightSample::try_new(if p == [9, 9, 9] { 15 } else { 2 }, 0).unwrap();
    let baked = bake_quad_lighting_with_sampler(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        &sampler,
        [8, 8, 8],
        Face::PositiveY,
        [[256, 256, 256]; 4],
    );
    assert!(
        baked.samples().into_iter().all(|sample| sample & 15 == 2),
        "the diagonal is replaced by a side"
    );
}

#[test]
fn cube_only_mesh_depends_on_diagonal_ao() {
    let assets = runtime_assets();
    assert!(
        mesh_dependency_mask(
            &BlockClassifier::new(AIR),
            &assets,
            NetworkIdMode::Sequential,
            &blocks(&[[8, 8, 8]])
        )
        .diagonal_ao
    );
}

#[test]
fn inset_face_samples_its_own_plane() {
    let center = blocks(&[]);
    let sampler =
        |[_, y, _]: [i32; 3]| MeshLightSample::try_new(if y == 8 { 11 } else { 0 }, 0).unwrap();
    for height in [1, 128, 255] {
        let assets = runtime_assets_with_model_geometry(
            vec![ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            }],
            vec![ModelQuad {
                positions: [[256, height, 256]; 4],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 2,
            }],
        );
        let baked = bake_template_lighting_with_sampler(
            &BlockClassifier::new(AIR),
            &assets,
            NetworkIdMode::Sequential,
            &MeshNeighbourhood::new(&center),
            &sampler,
            [8, 8, 8],
            0,
            0,
        )
        .unwrap();
        assert_eq!(baked[0].samples(), [11; 4], "inset y={height}");
    }
}
