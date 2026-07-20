use std::{
    collections::HashSet,
    fs,
    mem::{size_of, size_of_val},
    path::Path,
    sync::OnceLock,
};

use asset_compiler::compile_pack as compile_pack_with_lights;
use assets::{
    AssetError, BlockFace, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets,
    DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT,
    MODEL_QUAD_FLAG_CULL_FACE_MASK, MODEL_QUAD_FLAG_FACE_MASK, MODEL_QUAD_FLAG_TWO_SIDED,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    Material, ModelFamily, ModelQuad, ModelStateField, ModelTemplate, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, RegistryRecord, RuntimeAssets, TextureArray, TextureMip,
    TexturePage, TextureRef, VisualKind, encode_blob, read_registry,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use meshing::{
    BlockClassifier, ChunkMesh, ContributorResolver, Face, FaceConnectivity, MeshLightSample,
    Neighbourhood, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad,
    PackedQuadLighting, debug_color, mesh_sub_chunk, mesh_sub_chunk_in_neighbourhood,
    mesh_sub_chunk_with_lighting,
};
use world::{MeshNeighbourhood, SubChunk};

const AIR: u32 = 12_530;
const OPAQUE_A: u32 = 7;
const OPAQUE_B: u32 = 13;
const DIAGNOSTIC: u32 = 54;
const LEAF_A: u32 = 55;
const LEAF_B: u32 = 56;
const CROSS: u32 = 57;
const ZERO_QUAD_CROSS: u32 = 58;
const LIQUID_A: u32 = 59;
const LIQUID_B: u32 = 60;
const UNSUPPORTED_ADDITIONAL: u32 = 61;
const KELP: u32 = 62;
const MODEL_32: u32 = 63;
const COMPOUND_40: u32 = 64;
const MIXED_ALPHA_MODEL: u32 = 65;
const MODEL_TEMPLATE_FLAG_COMPOUND_NEXT_TEST: u32 = 1 << 2;

fn compile_pack(root: &Path, records: &[RegistryRecord]) -> Result<CompiledAssets, AssetError> {
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap_or(0)
    ];
    compile_pack_with_lights(root, records, &lights)
}

#[test]
fn packed_stream_record_sizes() {
    assert!(size_of::<ChunkMesh>() <= 128);
    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(size_of::<PackedModelRef>(), 16);
    assert_eq!(size_of::<PackedModelDrawRef>(), 8);
    assert_eq!(size_of::<PackedQuadLighting>(), 8);
    assert_eq!(size_of::<PackedLiquidQuad>(), 16);

    let model = PackedModelRef::new(1, 2, 3, 0xa5a5_5a5a);
    let draw = PackedModelDrawRef::new(0, 7);
    let lighting = PackedQuadLighting::new([0x00f0, 0x01f0, 0x02f0, 0x03f0]);
    let liquid = PackedLiquidQuad::try_from_words([4, 5, 6, 7]).unwrap();
    let mesh = ChunkMesh::from_streams(
        Vec::new(),
        vec![model],
        vec![lighting],
        vec![draw],
        vec![liquid],
        vec![lighting],
        FaceConnectivity::all(),
    );

    assert!(mesh.cube_quads().is_empty());
    assert_eq!(mesh.model_refs(), &[model]);
    assert_eq!(mesh.model_lighting(), &[lighting]);
    assert_eq!(mesh.model_draw_refs(), &[draw]);
    assert_eq!(mesh.liquid_quads(), &[liquid]);
    assert_eq!(mesh.liquid_lighting(), &[lighting]);
    assert!(!mesh.is_empty());
}

fn classifier() -> BlockClassifier {
    BlockClassifier::new(AIR)
}

fn runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let mut visuals = vec![
            BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
                kind: VisualKind::Diagnostic,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
            AIR as usize + 1
        ];
        visuals[AIR as usize].flags = BlockFlags::AIR;
        for runtime_id in [7, 11, 13, 17, 23, 29, 31, 37, 41, 43, 47] {
            visuals[runtime_id].faces = [runtime_id as u32; 6];
            visuals[runtime_id].flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
        }
        for runtime_id in [51, 52] {
            visuals[runtime_id].faces = [51; 6];
            visuals[runtime_id].flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
        }
        visuals[53] = BlockVisual {
            faces: [61, 62, 63, 64, 65, 66],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        // A non-full-cube record intentionally carries non-zero face IDs. The
        // mesher must still route it to the diagnostic material.
        visuals[54] = BlockVisual {
            faces: [66; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Diagnostic,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[LEAF_A as usize] = BlockVisual {
            faces: [LEAF_A; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            kind: VisualKind::Cube,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[LEAF_B as usize] = BlockVisual {
            faces: [LEAF_B; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            kind: VisualKind::Cube,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[CROSS as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Cross,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 2,
        };
        visuals[ZERO_QUAD_CROSS as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Cross,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 1,
            animation: NO_ANIMATION,
            variant: 0,
        };
        for runtime_id in [LIQUID_A, LIQUID_B] {
            visuals[runtime_id as usize] = BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
                kind: VisualKind::Liquid,
                contributor_role: assets::ContributorRole::LiquidAdditional,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
        }
        visuals[UNSUPPORTED_ADDITIONAL as usize] = BlockVisual {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Diagnostic,
            contributor_role: assets::ContributorRole::LiquidAdditional,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[KELP as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 2,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[MODEL_32 as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 3,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[COMPOUND_40 as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 4,
            animation: NO_ANIMATION,
            variant: 0,
        };
        visuals[MIXED_ALPHA_MODEL as usize] = BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 6,
            animation: NO_ANIMATION,
            variant: 0,
        };

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
        let light_properties = vec![assets::LightProperties::default(); visuals.len()];
        let compiled = CompiledAssets {
            visuals: visuals.into_boxed_slice(),
            // Hash 7 deliberately collides with sequential ID 7, but points
            // at the non-full-cube diagnostic record instead.
            hashed: vec![(7, 54), (0xdbf4_4120, 53)].into_boxed_slice(),
            materials: vec![
                Material {
                    texture: TextureRef::DIAGNOSTIC,
                    flags: 0,
                    animation: NO_ANIMATION
                };
                67
            ]
            .into_iter()
            .enumerate()
            .map(|(index, mut material)| {
                if index == 2 {
                    material.flags = MATERIAL_FLAG_ALPHA_BLEND;
                }
                material
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
            light_properties: light_properties.into_boxed_slice(),
            model_templates: vec![
                ModelTemplate {
                    quad_start: 0,
                    quad_count: 2,
                    flags: 0,
                },
                ModelTemplate {
                    quad_start: 2,
                    quad_count: 0,
                    flags: 0,
                },
                ModelTemplate {
                    quad_start: 2,
                    quad_count: 6,
                    flags: MODEL_TEMPLATE_FLAG_KELP,
                },
                ModelTemplate {
                    quad_start: 8,
                    quad_count: 32,
                    flags: 0,
                },
                ModelTemplate {
                    quad_start: 40,
                    quad_count: 24,
                    flags: MODEL_TEMPLATE_FLAG_COMPOUND_NEXT_TEST,
                },
                ModelTemplate {
                    quad_start: 64,
                    quad_count: 16,
                    flags: 0,
                },
                ModelTemplate {
                    quad_start: 80,
                    quad_count: 2,
                    flags: 0,
                },
            ]
            .into_boxed_slice(),
            model_quads: [
                ModelQuad {
                    positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
                    uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                    material: 1,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
                ModelQuad {
                    positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
                    uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                    material: 1,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
            ]
            .into_iter()
            .chain(
                std::iter::repeat_n(
                    ModelQuad {
                        positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
                        uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                        material: 1,
                        flags: 0,
                    },
                    6,
                )
                .enumerate()
                .map(|(index, mut quad)| {
                    if index >= 4 {
                        quad.flags = MODEL_QUAD_FLAG_TWO_SIDED;
                    }
                    quad
                }),
            )
            .chain(std::iter::repeat_n(
                ModelQuad {
                    positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
                    uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                    material: 1,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
                32,
            ))
            .chain(std::iter::repeat_n(
                ModelQuad {
                    positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
                    uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                    material: 1,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
                40,
            ))
            .chain([
                ModelQuad {
                    positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
                    uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                    material: 1,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
                ModelQuad {
                    positions: [[0, 0, 256], [0, 256, 256], [256, 256, 256], [256, 0, 256]],
                    uvs: [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
                    material: 2,
                    flags: MODEL_QUAD_FLAG_TWO_SIDED,
                },
            ])
            .collect::<Vec<_>>()
            .into_boxed_slice(),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(textures)].into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
        };
        let blob = encode_blob(&compiled).expect("encode synthetic mesher assets");
        RuntimeAssets::decode(&blob).expect("decode synthetic mesher assets")
    })
}

#[test]
fn mixed_alpha_model_partitions_exact_draw_refs_without_splitting_shared_records() {
    let center = blocks(MIXED_ALPHA_MODEL, &[[2, 3, 4]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &center,
    );

    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_lighting().len(), 2);
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 0]]
    );
    assert_eq!(
        mesh.transparent_model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 1]]
    );
}

#[test]
fn crossed_model_emits_compact_ref_visibility_transform_and_quad_lighting() {
    let center = blocks(CROSS, &[[2, 3, 4]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &center,
    );

    assert!(
        mesh.cube_quads().is_empty(),
        "cross plants must not emit cube faces"
    );
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(
        mesh.model_refs()[0].words(),
        [2 | (3 << 4) | (4 << 8), 0, 0, 0b11]
    );
    assert_eq!(mesh.model_lighting().len(), 2);
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 0], [0, 1]]
    );
    assert_eq!(mesh.model_lighting()[0].samples(), [0x00f0; 4]);
    assert_eq!(mesh.model_lighting()[1].samples(), [0x00f0; 4]);
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn crossed_model_visibility_is_conservative_next_to_full_occluder() {
    let center = adjacent_blocks(CROSS, OPAQUE_A);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &center,
    );
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b11);
}

#[test]
fn kelp_head_and_body_masks_are_selected_only_by_the_primary_above() {
    let isolated = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(KELP, &[[8, 8, 8]]),
    );
    assert_eq!(isolated.model_refs().len(), 1);
    assert_eq!(isolated.model_refs()[0].words()[3], 0b11_0000);
    assert_eq!(isolated.model_lighting().len(), 6);
    assert_eq!(
        isolated
            .model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 4], [0, 5]]
    );

    let stacked = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(KELP, &[[8, 8, 8], [8, 9, 8]]),
    );
    assert_eq!(stacked.model_refs().len(), 2);
    assert_eq!(stacked.model_refs()[0].words()[3], 0b1111);
    assert_eq!(stacked.model_refs()[1].words()[3], 0b11_0000);
    assert_eq!(stacked.model_lighting().len(), 12);
    assert_eq!(
        stacked
            .model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 0], [0, 1], [0, 2], [0, 3], [1, 4], [1, 5],]
    );

    let kelp = packed_storage(1, &[AIR, KELP], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 9, 8], 1)]);
    let liquid_above = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk(vec![kelp, water]),
    );
    assert_eq!(liquid_above.model_refs()[0].words()[3], 0b11_0000);

    let kelp = packed_storage(1, &[AIR, KELP], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let layered = sub_chunk(vec![kelp, water]);
    let contributors = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &layered,
    )
    .resolve([8, 8, 8]);
    assert_eq!(contributors.primary_network_value(), Some(KELP));
    assert_eq!(contributors.liquid_network_value(), Some(LIQUID_A));
    let layered = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &layered,
    );
    assert_eq!(layered.model_refs()[0].words()[3], 0b11_0000);

    let non_kelp_above = sub_chunk(vec![packed_storage(
        2,
        &[AIR, KELP, OPAQUE_A],
        &[([8, 8, 8], 1), ([8, 9, 8], 2)],
    )]);
    let non_kelp_above = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &non_kelp_above,
    );
    assert_eq!(non_kelp_above.model_refs()[0].words()[3], 0b11_0000);

    let kelp_layer = packed_storage(1, &[AIR, KELP], &[([8, 8, 8], 1), ([8, 9, 8], 1)]);
    let conflicting_primary = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 9, 8], 1)]);
    let conflict_above = sub_chunk(vec![kelp_layer, conflicting_primary]);
    let conflict_above = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &conflict_above,
    );
    assert_eq!(conflict_above.model_refs()[0].words()[3], 0b11_0000);
}

#[test]
fn kelp_body_selection_crosses_the_positive_y_subchunk_boundary() {
    let center = blocks(KELP, &[[8, 15, 8]]);
    let positive_y = blocks(KELP, &[[8, 0, 8]]);
    let neighbourhood = Neighbourhood::empty().with_positive_y(&positive_y);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &neighbourhood,
        &center,
    );
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b1111);
}

#[test]
fn mixed_zero_and_nonzero_templates_emit_only_drawable_model_streams() {
    let center = adjacent_blocks(ZERO_QUAD_CROSS, CROSS);
    let mixed_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &center,
    );

    assert_eq!(mixed_mesh.model_refs().len(), 1);
    assert_eq!(mixed_mesh.model_refs()[0].words()[1], 0);
    assert_eq!(mixed_mesh.model_lighting().len(), 2);
    assert_eq!(mixed_mesh.model_refs()[0].words()[3], 0b11);
    assert!(mixed_mesh.connectivity().is_all_connected());

    let zero_only = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::default(),
        &blocks(ZERO_QUAD_CROSS, &[[8, 8, 8]]),
    );
    assert!(zero_only.model_refs().is_empty());
    assert!(zero_only.model_lighting().is_empty());
    assert!(zero_only.model_draw_refs().is_empty());
    assert!(
        zero_only.is_empty(),
        "zero-only model chunks must be removed before allocation/presentation"
    );
}

#[test]
fn thirty_two_quad_model_emits_exact_ascending_draw_refs() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(MODEL_32, &[[1, 2, 3]]),
    );

    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_refs()[0].words()[3], u32::MAX);
    assert_eq!(mesh.model_lighting().len(), 32);
    assert_eq!(mesh.model_draw_refs().len(), 32);
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        (0..32)
            .map(|quad_index| [0, quad_index])
            .collect::<Vec<_>>()
    );
}

#[test]
fn compound_model_emits_two_bounded_refs_with_contiguous_lighting_and_draws() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(COMPOUND_40, &[[1, 2, 3]]),
    );

    assert_eq!(mesh.model_refs().len(), 2);
    assert_eq!(
        mesh.model_refs()
            .iter()
            .copied()
            .map(PackedModelRef::words)
            .collect::<Vec<_>>(),
        [
            [1 | (2 << 4) | (3 << 8), 4, 0, 0x00ff_ffff],
            [1 | (2 << 4) | (3 << 8), 5, 24, 0x0000_ffff],
        ]
    );
    assert_eq!(mesh.model_lighting().len(), 40);
    assert!(
        mesh.model_lighting()
            .iter()
            .all(|lighting| lighting.samples() == [0x00f0; 4])
    );
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        (0..24)
            .map(|quad| [0, quad])
            .chain((0..16).map(|quad| [1, quad]))
            .collect::<Vec<_>>()
    );
}
