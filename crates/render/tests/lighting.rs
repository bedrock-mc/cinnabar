use std::mem::size_of;

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, Material, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    encode_blob,
};
use render::{
    BlockClassifier, Face, PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, PackedQuad, PackedQuadLighting,
    bake_quad_lighting, bake_template_lighting, mesh_dependency_mask,
};
use world::{MeshNeighbourhood, SubChunk};

const AIR: u32 = 0;
const SOLID: u32 = 1;
const MODEL: u32 = 2;
const LIQUID: u32 = 3;

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
    SubChunk::decode(&bytes).expect("decode lighting fixture")
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
    SubChunk::decode(&bytes).expect("decode layered lighting fixture")
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
    let visuals = vec![
        BlockVisual {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags: BlockFlags::AIR,
            kind: VisualKind::Invisible,
            contributor_role: ContributorRole::Air,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Liquid,
            contributor_role: ContributorRole::LiquidAdditional,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
    ];
    let light_properties = vec![assets::LightProperties::default(); visuals.len()];
    let compiled = CompiledAssets {
        visuals: visuals.into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        light_properties: light_properties.into_boxed_slice(),
        model_templates: model_templates.into_boxed_slice(),
        model_quads: model_quads.into_boxed_slice(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(textures)].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    RuntimeAssets::decode(&encode_blob(&compiled).expect("encode lighting assets"))
        .expect("decode lighting assets")
}

fn fixture() -> (RuntimeAssets, SubChunk) {
    // At the high corner of block 8,8,8, the up face sees both planar sides,
    // while the east face sees only their shared +X/+Y side.
    (runtime_assets(), blocks(&[[9, 9, 8], [8, 9, 9]]))
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
    let expected = [Face::PositiveY, Face::PositiveX, Face::NegativeZ].map(|face| {
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
    let shader = include_str!("../src/model.wgsl");
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
