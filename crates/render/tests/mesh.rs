use std::{
    collections::HashSet,
    fs,
    mem::{size_of, size_of_val},
    path::Path,
    sync::OnceLock,
};

use assets::{
    BlockFace, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, DIAGNOSTIC_MATERIAL,
    MATERIAL_FLAG_ALPHA_BLEND, MODEL_QUAD_FLAG_CULL_FACE_MASK, MODEL_QUAD_FLAG_FACE_MASK,
    MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, Material,
    ModelFamily, ModelQuad, ModelStateField, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    compile_pack, encode_blob, read_registry,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use render::{
    BlockClassifier, ChunkMesh, ContributorResolver, Face, FaceConnectivity, Neighbourhood,
    PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad, PackedQuadLighting,
    debug_color, mesh_sub_chunk,
};
use world::SubChunk;

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

struct CompiledVineFixture {
    assets: RuntimeAssets,
    air: u32,
    cube: u32,
    by_mask: [u32; 16],
}

fn write_vine_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create vine render fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(r#"{{"vine":{{"textures":"vine"}},"{cube_name}":{{"textures":"cube"}}}}"#),
    )
    .expect("write vine render block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"vine":{"textures":"textures/blocks/vine"},"cube":{"textures":"textures/blocks/cube"}}}"#,
    )
    .expect("write vine render terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty vine flipbook inventory");

    for (index, name) in ["vine", "cube"].into_iter().enumerate() {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let x = (pixel_index % 16) as u8;
            let y = (pixel_index / 16) as u8;
            pixel.copy_from_slice(&[20 + index as u8 * 90 + x, 40 + y, 80, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode vine render fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write vine render fixture PNG");
    }
}

fn compiled_vine_fixture() -> &'static CompiledVineFixture {
    static FIXTURE: OnceLock<CompiledVineFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let generated = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed vine registry");
        let air = generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("committed registry air record")
            .clone();
        let cube = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("committed full cube record")
            .clone();
        let mut vines = generated
            .iter()
            .filter(|record| record.model_family == ModelFamily::Vine)
            .cloned()
            .collect::<Vec<_>>();
        vines.sort_unstable_by_key(|record| {
            record
                .model_state
                .get(ModelStateField::Connections)
                .expect("vine direction bits")
        });
        assert_eq!(vines.len(), 16, "protocol-1001 vine state count");
        let by_mask = std::array::from_fn(|mask| {
            assert_eq!(
                vines[mask].model_state.get(ModelStateField::Connections),
                Some(mask as u32)
            );
            vines[mask].sequential_id
        });
        let cube_name = cube
            .name
            .strip_prefix("minecraft:")
            .expect("canonical cube namespace");
        let directory = tempfile::tempdir().expect("create compiled vine render fixture");
        write_vine_render_pack(directory.path(), cube_name);
        let mut records = Vec::with_capacity(18);
        records.push(air.clone());
        records.extend(vines);
        records.push(cube.clone());
        let compiled = compile_pack(directory.path(), &records)
            .expect("compile all vine states through assets compiler");
        let blob = encode_blob(&compiled).expect("encode compiled vine render fixture");
        CompiledVineFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode compiled vine render fixture"),
            air: air.sequential_id,
            cube: cube.sequential_id,
            by_mask,
        }
    })
}

fn mesh_compiled_vine(
    runtime_id: u32,
    coordinates: &[[u8; 3]],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_vine_fixture();
    let placements = coordinates
        .iter()
        .copied()
        .map(|coordinate| (coordinate, 1))
        .collect::<Vec<_>>();
    let center = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, runtime_id],
        &placements,
    )]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        &center,
    )
}

#[test]
fn compiled_vines_cover_all_masks_with_exact_cpu_model_streams_and_zero_mask_no_draw() {
    let fixture = compiled_vine_fixture();
    for (mask, &runtime_id) in fixture.by_mask.iter().enumerate() {
        let resolved = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, runtime_id);
        assert_eq!(resolved.kind(), VisualKind::Model, "mask {mask}");
        assert!(
            !resolved
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template_id = resolved.model_template().expect("compiled vine template");
        let quad_count = (mask as u32).count_ones();
        assert_eq!(
            fixture.assets.model_templates()[template_id as usize].quad_count,
            quad_count,
            "mask {mask}"
        );

        let mesh = mesh_compiled_vine(runtime_id, &[[7, 8, 9]], &Neighbourhood::empty());
        assert!(mesh.cube_quads().is_empty(), "mask {mask}");
        assert!(mesh.liquid_quads().is_empty(), "mask {mask}");
        if mask == 0 {
            assert!(mesh.model_refs().is_empty());
            assert!(mesh.model_lighting().is_empty());
            assert!(
                mesh.is_empty(),
                "zero-mask vine must allocate no draw stream"
            );
        } else {
            assert_eq!(mesh.model_refs().len(), 1, "mask {mask}");
            assert_eq!(
                mesh.model_refs()[0].words(),
                [
                    7 | (8 << 4) | (9 << 8),
                    template_id,
                    0,
                    (1_u32 << quad_count) - 1,
                ],
                "mask {mask}"
            );
            assert_eq!(
                mesh.model_lighting().len(),
                quad_count as usize,
                "mask {mask}"
            );
        }
        assert!(
            mesh.connectivity().is_all_connected(),
            "mask {mask}: vines must not close cave-connectivity faces"
        );
    }
}

#[test]
fn compiled_vines_remain_drawable_on_every_subchunk_boundary_next_to_full_occluders() {
    let fixture = compiled_vine_fixture();
    let coordinates = [
        [0, 8, 8],
        [15, 8, 8],
        [8, 0, 8],
        [8, 15, 8],
        [8, 8, 0],
        [8, 8, 15],
    ];
    let opaque = sub_chunk(vec![uniform_storage(fixture.cube)]);
    let neighbourhood = Neighbourhood::empty()
        .with_negative_x(&opaque)
        .with_positive_x(&opaque)
        .with_negative_y(&opaque)
        .with_positive_y(&opaque)
        .with_negative_z(&opaque)
        .with_positive_z(&opaque);
    let expected_origins = coordinates
        .iter()
        .map(|[x, y, z]| u32::from(*x) | (u32::from(*y) << 4) | (u32::from(*z) << 8))
        .collect::<HashSet<_>>();

    for (mask, &runtime_id) in fixture.by_mask.iter().enumerate() {
        let quad_count = (mask as u32).count_ones();
        let mesh = mesh_compiled_vine(runtime_id, &coordinates, &neighbourhood);
        assert!(mesh.cube_quads().is_empty(), "mask {mask}");
        if mask == 0 {
            assert!(mesh.model_refs().is_empty());
            assert!(mesh.model_lighting().is_empty());
            continue;
        }
        assert_eq!(mesh.model_refs().len(), coordinates.len(), "mask {mask}");
        assert_eq!(
            mesh.model_refs()
                .iter()
                .map(|reference| reference.words()[0])
                .collect::<HashSet<_>>(),
            expected_origins,
            "mask {mask}: boundary positions"
        );
        for (index, reference) in mesh.model_refs().iter().enumerate() {
            assert_eq!(
                reference.words()[2],
                index as u32 * quad_count,
                "mask {mask}"
            );
            assert_eq!(
                reference.words()[3],
                (1_u32 << quad_count) - 1,
                "mask {mask}: a full neighbour must not cull a two-sided attachment plane"
            );
        }
        assert_eq!(
            mesh.model_lighting().len(),
            coordinates.len() * quad_count as usize,
            "mask {mask}"
        );
        assert!(mesh.connectivity().is_all_connected(), "mask {mask}");
    }
}

struct CompiledSlabFixture {
    assets: RuntimeAssets,
    air: u32,
    lower: u32,
    upper: u32,
    full: u32,
    cube: u32,
}

fn write_slab_render_pack(root: &Path, slab_name: &str, double_name: &str, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create slab render fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{"{slab_name}":{{"textures":{{"down":"slab_down","side":"slab_side","up":"slab_up"}}}},"{double_name}":{{"textures":{{"down":"slab_down","side":"slab_side","up":"slab_up"}}}},"{cube_name}":{{"textures":"cube_all"}}}}"#
        ),
    )
    .expect("write slab render block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"slab_down":{"textures":"textures/blocks/slab_down"},"slab_side":{"textures":"textures/blocks/slab_side"},"slab_up":{"textures":"textures/blocks/slab_up"},"cube_all":{"textures":"textures/blocks/cube_all"}}}"#,
    )
    .expect("write slab render terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty slab flipbook inventory");

    for (index, name) in ["slab_down", "slab_side", "slab_up", "cube_all"]
        .into_iter()
        .enumerate()
    {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[40 + index as u8 * 60, 80, 120, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode slab fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write slab fixture PNG");
    }
}

fn compiled_slab_fixture() -> &'static CompiledSlabFixture {
    static FIXTURE: OnceLock<CompiledSlabFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let generated = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed slab registry");
        let air = generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("committed registry air record")
            .clone();
        let lower = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Slab
                    && record.model_state.get(ModelStateField::Half) == Some(0)
            })
            .expect("committed lower slab record")
            .clone();
        let upper = generated
            .iter()
            .find(|record| {
                record.name == lower.name
                    && record.model_state.get(ModelStateField::Half) == Some(1)
            })
            .expect("matching committed upper slab record")
            .clone();
        let full = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Slab
                    && record.model_state.get(ModelStateField::Half) == Some(2)
            })
            .expect("committed full slab record")
            .clone();
        let cube = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("committed full cube record")
            .clone();
        let slab_name = lower
            .name
            .strip_prefix("minecraft:")
            .expect("canonical slab namespace")
            .to_owned();
        let double_name = full
            .name
            .strip_prefix("minecraft:")
            .expect("canonical double slab namespace")
            .to_owned();
        let cube_name = cube
            .name
            .strip_prefix("minecraft:")
            .expect("canonical cube namespace")
            .to_owned();
        let directory = tempfile::tempdir().expect("create compiled slab render fixture");
        write_slab_render_pack(directory.path(), &slab_name, &double_name, &cube_name);
        let ids = [
            air.sequential_id,
            lower.sequential_id,
            upper.sequential_id,
            full.sequential_id,
            cube.sequential_id,
        ];
        let compiled = compile_pack(directory.path(), &[air, lower, upper, full, cube])
            .expect("compile slab fixture through assets compiler");
        let blob = encode_blob(&compiled).expect("encode compiled slab render fixture");
        CompiledSlabFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode compiled slab render fixture"),
            air: ids[0],
            lower: ids[1],
            upper: ids[2],
            full: ids[3],
            cube: ids[4],
        }
    })
}

fn mesh_compiled_slab(runtime_id: u32, coordinates: &[[u8; 3]]) -> ChunkMesh {
    let fixture = compiled_slab_fixture();
    let placements = coordinates
        .iter()
        .copied()
        .map(|coordinate| (coordinate, 1))
        .collect::<Vec<_>>();
    let sub_chunk = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, runtime_id],
        &placements,
    )]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk,
    )
}

#[test]
fn compiled_slabs_emit_only_exact_bounded_model_and_lighting_streams() {
    let fixture = compiled_slab_fixture();
    for (runtime_id, expected_flags, full_occluder) in [
        (fixture.lower, [0x33, 0x44, 0x11, 0x02, 0x55, 0x66], false),
        (fixture.upper, [0x33, 0x44, 0x01, 0x22, 0x55, 0x66], false),
        (fixture.full, [0x33, 0x44, 0x11, 0x22, 0x55, 0x66], true),
    ] {
        let resolved = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, runtime_id);
        assert_eq!(resolved.kind(), VisualKind::Model);
        assert_eq!(
            resolved.flags().contains(BlockFlags::OCCLUDES_FULL_FACE),
            full_occluder
        );
        assert!(!resolved.flags().contains(BlockFlags::CUBE_GEOMETRY));
        let template_id = resolved.model_template().expect("compiled slab template");
        let template = fixture.assets.model_templates()[template_id as usize];
        assert_eq!(template.quad_count, 6);
        assert_eq!(template.flags, 0);
        let quads = &fixture.assets.model_quads()
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert_eq!(
            quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
            expected_flags
        );
        for (index, face) in BlockFace::ALL.into_iter().enumerate() {
            assert_eq!(quads[index].material, resolved.face(face).material_id());
            assert_eq!(quads[index].flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_ne!(quads[index].flags & MODEL_QUAD_FLAG_FACE_MASK, 0);
            assert_eq!(
                quads[index].flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK),
                0
            );
        }

        let mesh = mesh_compiled_slab(runtime_id, &[[2, 3, 4]]);
        assert!(mesh.cube_quads().is_empty());
        assert!(mesh.liquid_quads().is_empty());
        assert!(mesh.liquid_lighting().is_empty());
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(size_of::<PackedModelRef>(), 16);
        assert_eq!(
            mesh.model_refs()[0].words(),
            [2 | (3 << 4) | (4 << 8), template_id, 0, 0b11_1111]
        );
        assert_eq!(mesh.model_lighting().len(), 6);
        assert_eq!(size_of_val(mesh.model_lighting()), 6 * 8);
    }
}

#[test]
fn compiled_slabs_scale_one_ref_and_six_lighting_records_per_block() {
    let fixture = compiled_slab_fixture();
    let coordinates = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    let mesh = mesh_compiled_slab(fixture.lower, &coordinates);
    assert_eq!(mesh.model_refs().len(), 3);
    assert_eq!(size_of_val(mesh.model_refs()), 3 * 16);
    assert_eq!(mesh.model_lighting().len(), 18);
    assert_eq!(size_of_val(mesh.model_lighting()), 18 * 8);
    for (index, reference) in mesh.model_refs().iter().enumerate() {
        assert_eq!(reference.words()[2], (index * 6) as u32);
        assert_eq!(reference.words()[3], 0b11_1111);
    }
}

fn mesh_compiled_fixture<'a>(sub_chunk: &SubChunk, neighbours: &Neighbourhood<'a>) -> ChunkMesh {
    let fixture = compiled_slab_fixture();
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        sub_chunk,
    )
}

#[test]
fn compiled_full_slab_and_cube_cull_shared_model_and_cube_faces_in_subchunk() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.full, fixture.cube],
        &[([7, 8, 8], 1), ([8, 8, 8], 2)],
    )]);
    let mesh = mesh_compiled_fixture(&center, &Neighbourhood::empty());

    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_refs()[0].words()[2], 0);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b11_1101);
    assert_eq!(mesh.model_lighting().len(), 6);
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 0], [0, 2], [0, 3], [0, 4], [0, 5]]
    );
    assert_eq!(mesh.cube_quads().len(), 5);
    assert!(!has_face(&mesh, [8, 8, 8], Face::NegativeX));
    assert!(mesh.liquid_quads().is_empty());
}

#[test]
fn fully_occluded_model_emits_no_model_triplet_stream() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.full, fixture.cube],
        &[
            ([8, 8, 8], 1),
            ([7, 8, 8], 2),
            ([9, 8, 8], 2),
            ([8, 7, 8], 2),
            ([8, 9, 8], 2),
            ([8, 8, 7], 2),
            ([8, 8, 9], 2),
        ],
    )]);
    let mesh = mesh_compiled_fixture(&center, &Neighbourhood::empty());

    assert!(mesh.model_refs().is_empty());
    assert!(mesh.model_lighting().is_empty());
    assert!(mesh.model_draw_refs().is_empty());
}

#[test]
fn compiled_model_cull_faces_cross_subchunk_boundaries_without_reindexing_lighting() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.lower, fixture.full],
        &[([2, 3, 4], 1), ([15, 8, 8], 2)],
    )]);
    let positive_x = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, fixture.cube],
        &[([0, 8, 8], 1)],
    )]);
    let neighbourhood = Neighbourhood::empty().with_positive_x(&positive_x);
    let mesh = mesh_compiled_fixture(&center, &neighbourhood);

    assert_eq!(mesh.model_refs().len(), 2);
    assert_eq!(mesh.model_refs()[0].words()[2], 0);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b11_1111);
    assert_eq!(mesh.model_refs()[1].words()[2], 6);
    assert_eq!(mesh.model_refs()[1].words()[3], 0b11_1101);
    assert_eq!(mesh.model_lighting().len(), 12);
    assert!(mesh.cube_quads().is_empty());

    let without_neighbour = mesh_compiled_fixture(&center, &Neighbourhood::empty());
    assert_eq!(without_neighbour.model_refs()[1].words()[3], 0b11_1111);
    assert_eq!(without_neighbour.model_refs()[1].words()[2], 6);
    assert_eq!(without_neighbour.model_lighting().len(), 12);
}

#[test]
fn compiled_model_cull_faces_map_all_six_subchunk_boundaries_for_cube_and_model_occluders() {
    let fixture = compiled_slab_fixture();
    for (quad_index, face, current, remote) in [
        (0, Face::NegativeX, [0, 8, 8], [15, 8, 8]),
        (1, Face::PositiveX, [15, 8, 8], [0, 8, 8]),
        (2, Face::NegativeY, [8, 0, 8], [8, 15, 8]),
        (3, Face::PositiveY, [8, 15, 8], [8, 0, 8]),
        (4, Face::NegativeZ, [8, 8, 0], [8, 8, 15]),
        (5, Face::PositiveZ, [8, 8, 15], [8, 8, 0]),
    ] {
        let center = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, fixture.full],
            &[(current, 1)],
        )]);
        for occluder in [fixture.cube, fixture.full] {
            let neighbour = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, occluder],
                &[(remote, 1)],
            )]);
            let neighbourhood = neighbourhood_for(face, &neighbour);
            let mesh = mesh_compiled_fixture(&center, &neighbourhood);
            assert_eq!(mesh.model_refs().len(), 1);
            assert_eq!(
                mesh.model_refs()[0].words()[3],
                0b11_1111 & !(1 << quad_index),
                "face={face:?} occluder={occluder}"
            );
            assert_eq!(mesh.model_refs()[0].words()[2], 0);
            assert_eq!(mesh.model_lighting().len(), 6);
        }
    }
}

#[test]
fn compiled_partial_slab_walls_are_cave_open_but_full_slab_walls_occlude() {
    let fixture = compiled_slab_fixture();
    let wall = (0..16)
        .flat_map(|y| (0..16).map(move |z| [8, y, z]))
        .collect::<Vec<_>>();
    for runtime_id in [fixture.lower, fixture.upper] {
        let mesh = mesh_compiled_slab(runtime_id, &wall);
        assert!(
            mesh.connectivity().is_all_connected(),
            "partial slabs must remain conservatively cave-open"
        );
    }

    let full = mesh_compiled_slab(fixture.full, &wall);
    assert!(
        !full
            .connectivity()
            .is_connected(Face::NegativeX, Face::PositiveX),
        "a complete wall of full slabs must separate opposite cave faces"
    );
}

struct CompiledConnectedFixture {
    assets: RuntimeAssets,
    air: u32,
    pane: u32,
    other_pane: u32,
    wood_fence: u32,
    nether_fence: u32,
    gate_facing_z: u32,
    gate_facing_x: u32,
    wall: u32,
    cube: u32,
}

fn write_connected_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create connected fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "glass_pane":{{"textures":{{"west":"pane_body","east":"pane_edge","down":"pane_edge","up":"pane_edge","north":"pane_body","south":"pane_body"}}}},
                "white_stained_glass_pane":{{"textures":{{"west":"other_pane_body","east":"other_pane_edge","down":"other_pane_edge","up":"other_pane_edge","north":"other_pane_body","south":"other_pane_body"}}}},
                "oak_fence":{{"textures":"oak_fence"}},
                "nether_brick_fence":{{"textures":"nether_fence"}},
                "fence_gate":{{"textures":"oak_fence_gate"}},
                "cobblestone_wall":{{"textures":"wall"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write connected block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "pane_body":{"textures":"textures/blocks/pane_body"},
            "pane_edge":{"textures":"textures/blocks/pane_edge"},
            "other_pane_body":{"textures":"textures/blocks/other_pane_body"},
            "other_pane_edge":{"textures":"textures/blocks/other_pane_edge"},
            "oak_fence":{"textures":"textures/blocks/oak_fence"},
            "nether_fence":{"textures":"textures/blocks/nether_fence"},
            "oak_fence_gate":{"textures":"textures/blocks/oak_fence_gate"},
            "wall":{"textures":"textures/blocks/wall"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write connected terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write connected empty flipbooks");
    for (index, name) in [
        "pane_body",
        "pane_edge",
        "other_pane_body",
        "other_pane_edge",
        "oak_fence",
        "nether_fence",
        "oak_fence_gate",
        "wall",
        "cube",
    ]
    .into_iter()
    .enumerate()
    {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20 + index as u8 * 25, 70, 110, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode connected fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write connected fixture PNG");
    }
}

fn compiled_connected_fixture() -> &'static CompiledConnectedFixture {
    static FIXTURE: OnceLock<CompiledConnectedFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode connected registry");
        let named = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing {name}"))
                .clone()
        };
        let air = named("minecraft:air");
        let pane = named("minecraft:glass_pane");
        let other_pane = named("minecraft:white_stained_glass_pane");
        let wood_fence = named("minecraft:oak_fence");
        let nether_fence = named("minecraft:nether_brick_fence");
        let gate = |orientation| {
            records
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:fence_gate"
                        && record.model_state.get(ModelStateField::Orientation) == Some(orientation)
                        && record.model_state.get(ModelStateField::Open) == Some(0)
                        && record.model_state.get(ModelStateField::Flags) == Some(0)
                })
                .unwrap_or_else(|| panic!("missing closed oak gate orientation {orientation}"))
                .clone()
        };
        let gate_facing_z = gate(0);
        let gate_facing_x = gate(1);
        let wall = named("minecraft:cobblestone_wall");
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("full cube")
            .clone();
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("connected fixture directory");
        write_connected_render_pack(directory.path(), cube_name);
        let ids = [
            air.sequential_id,
            pane.sequential_id,
            other_pane.sequential_id,
            wood_fence.sequential_id,
            nether_fence.sequential_id,
            gate_facing_z.sequential_id,
            gate_facing_x.sequential_id,
            wall.sequential_id,
            cube.sequential_id,
        ];
        let compiled = compile_pack(
            directory.path(),
            &[
                air,
                pane,
                other_pane,
                wood_fence,
                nether_fence,
                gate_facing_z,
                gate_facing_x,
                wall,
                cube,
            ],
        )
        .expect("compile connected fixture");
        let blob = encode_blob(&compiled).expect("encode connected fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode connected fixture");
        let pane_base = assets
            .resolve(NetworkIdMode::Sequential, ids[1])
            .model_template()
            .unwrap();
        let wood_base = assets
            .resolve(NetworkIdMode::Sequential, ids[3])
            .model_template()
            .unwrap();
        let nether_base = assets
            .resolve(NetworkIdMode::Sequential, ids[4])
            .model_template()
            .unwrap();
        assert_eq!(
            assets.model_templates()[pane_base as usize].flags,
            MODEL_TEMPLATE_FLAG_PANE
        );
        assert_eq!(
            assets.model_templates()[wood_base as usize].flags,
            MODEL_TEMPLATE_FLAG_FENCE_WOOD
        );
        assert_eq!(
            assets.model_templates()[nether_base as usize].flags,
            MODEL_TEMPLATE_FLAG_FENCE_NETHER
        );
        CompiledConnectedFixture {
            assets,
            air: ids[0],
            pane: ids[1],
            other_pane: ids[2],
            wood_fence: ids[3],
            nether_fence: ids[4],
            gate_facing_z: ids[5],
            gate_facing_x: ids[6],
            wall: ids[7],
            cube: ids[8],
        }
    })
}

const CONNECTED_OFFSETS: [([u8; 3], u32); 4] = [
    ([8, 8, 7], 1),
    ([9, 8, 8], 2),
    ([8, 8, 9], 4),
    ([7, 8, 8], 8),
];

fn mesh_connected(center_id: u32, neighbours: &[(usize, u32)]) -> ChunkMesh {
    let fixture = compiled_connected_fixture();
    let mut palette = vec![fixture.air, center_id];
    let mut placements = vec![([8, 8, 8], 1)];
    for &(direction, id) in neighbours {
        let palette_index = palette
            .iter()
            .position(|&value| value == id)
            .unwrap_or_else(|| {
                palette.push(id);
                palette.len() - 1
            });
        placements.push((CONNECTED_OFFSETS[direction].0, palette_index));
    }
    let sub = sub_chunk(vec![packed_storage(4, &palette, &placements)]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    )
}

#[test]
fn panes_select_all_sixteen_palette_native_connection_masks() {
    let fixture = compiled_connected_fixture();
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.pane)
        .model_template()
        .unwrap();
    for mask in 0_u32..16 {
        let neighbours = CONNECTED_OFFSETS
            .iter()
            .enumerate()
            .filter(|(_, (_, bit))| mask & bit != 0)
            .map(|(direction, _)| (direction, fixture.pane))
            .collect::<Vec<_>>();
        let mesh = mesh_connected(fixture.pane, &neighbours);
        let center = center_stair_ref(&mesh, [8, 8, 8]);
        assert_eq!(center.words()[1], base + mask, "mask={mask:#06b}");
        assert_eq!(mesh.model_refs().len(), 1 + neighbours.len());
    }
    let all_cubes = (0..4)
        .map(|direction| (direction, fixture.cube))
        .collect::<Vec<_>>();
    assert_eq!(
        center_stair_ref(&mesh_connected(fixture.pane, &all_cubes), [8, 8, 8]).words()[1],
        base + 15
    );
    assert_eq!(
        center_stair_ref(
            &mesh_connected(fixture.pane, &[(0, fixture.wall)]),
            [8, 8, 8]
        )
        .words()[1],
        base + 1,
        "thin panes connect to wall models"
    );
}

#[test]
fn equal_panes_suppress_only_internal_edge_caps_and_different_materials_retain_them() {
    let fixture = compiled_connected_fixture();
    let same = mesh_connected(fixture.pane, &[(0, fixture.pane)]);
    let different = mesh_connected(fixture.pane, &[(0, fixture.other_pane)]);
    let same_ref = center_stair_ref(&same, [8, 8, 8]);
    let different_ref = center_stair_ref(&different, [8, 8, 8]);
    assert_eq!(same_ref.words()[1], different_ref.words()[1]);
    assert_eq!(
        different_ref.words()[3].count_ones(),
        same_ref.words()[3].count_ones() + 1,
        "only the equal-material boundary cap is suppressed"
    );
    assert_eq!(
        different_ref.words()[3].count_ones(),
        fixture.assets.model_templates()[different_ref.words()[1] as usize].quad_count
    );
}

#[test]
fn fences_select_bounded_post_plus_arm_refs_and_preserve_connection_class() {
    let fixture = compiled_connected_fixture();
    for (fence, same, different, flag) in [
        (
            fixture.wood_fence,
            fixture.wood_fence,
            fixture.nether_fence,
            MODEL_TEMPLATE_FLAG_FENCE_WOOD,
        ),
        (
            fixture.nether_fence,
            fixture.nether_fence,
            fixture.wood_fence,
            MODEL_TEMPLATE_FLAG_FENCE_NETHER,
        ),
    ] {
        let base = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, fence)
            .model_template()
            .unwrap();
        assert_eq!(fixture.assets.model_templates()[base as usize].flags, flag);
        for mask in 0_u32..16 {
            let neighbours = CONNECTED_OFFSETS
                .iter()
                .enumerate()
                .filter(|(_, (_, bit))| mask & bit != 0)
                .map(|(direction, _)| (direction, same))
                .collect::<Vec<_>>();
            let mesh = mesh_connected(fence, &neighbours);
            let center_refs = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .collect::<Vec<_>>();
            assert_eq!(center_refs.len(), if mask == 0 { 1 } else { 2 });
            assert_eq!(center_refs[0].words()[1], base);
            if mask != 0 {
                assert_eq!(center_refs[1].words()[1], base + 1 + mask);
            }
        }
        let different_mesh = mesh_connected(fence, &[(0, different)]);
        assert_eq!(
            different_mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count(),
            1,
            "wood and nether fences do not connect"
        );
        let cube_mesh = mesh_connected(fence, &[(0, fixture.cube)]);
        assert_eq!(
            cube_mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count(),
            2,
            "full support face connects"
        );
    }
}

#[test]
fn fences_connect_only_to_the_sides_of_axis_aligned_fence_gates() {
    let fixture = compiled_connected_fixture();
    for (gate, connecting_directions) in [
        (fixture.gate_facing_z, [1_usize, 3]),
        (fixture.gate_facing_x, [0_usize, 2]),
    ] {
        for direction in 0..4 {
            let mesh = mesh_connected(fixture.wood_fence, &[(direction, gate)]);
            let center_ref_count = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count();
            assert_eq!(
                center_ref_count,
                if connecting_directions.contains(&direction) {
                    2
                } else {
                    1
                },
                "gate={gate} direction={direction}"
            );
        }
    }
}

#[test]
fn connected_models_cross_all_four_horizontal_subchunk_boundaries() {
    let fixture = compiled_connected_fixture();
    let pane_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.pane)
        .model_template()
        .unwrap();
    let fence_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.wood_fence)
        .model_template()
        .unwrap();
    for (center, remote, face, bit) in [
        ([8, 8, 0], [8, 8, 15], Face::NegativeZ, 1_u32),
        ([15, 8, 8], [0, 8, 8], Face::PositiveX, 2),
        ([8, 8, 15], [8, 8, 0], Face::PositiveZ, 4),
        ([0, 8, 8], [15, 8, 8], Face::NegativeX, 8),
    ] {
        for (block, base, expected_refs) in [
            (fixture.pane, pane_base, 1_usize),
            (fixture.wood_fence, fence_base, 2),
        ] {
            let center_sub = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, block],
                &[(center, 1)],
            )]);
            let remote_sub = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, block],
                &[(remote, 1)],
            )]);
            let mesh = mesh_sub_chunk(
                &BlockClassifier::new(fixture.air),
                &fixture.assets,
                NetworkIdMode::Sequential,
                &neighbourhood_for(face, &remote_sub),
                &center_sub,
            );
            let center_word =
                u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8);
            let refs = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == center_word)
                .collect::<Vec<_>>();
            assert_eq!(refs.len(), expected_refs, "face={face:?} block={block}");
            if block == fixture.pane {
                assert_eq!(refs[0].words()[1], base + bit, "face={face:?}");
            } else {
                assert_eq!(refs[0].words()[1], base, "face={face:?}");
                assert_eq!(refs[1].words()[1], base + 1 + bit, "face={face:?}");
            }
            let missing = mesh_sub_chunk(
                &BlockClassifier::new(fixture.air),
                &fixture.assets,
                NetworkIdMode::Sequential,
                &Neighbourhood::empty(),
                &center_sub,
            );
            let missing_refs = missing
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == center_word)
                .count();
            assert_eq!(missing_refs, 1, "missing face={face:?} block={block}");
        }
    }
}

#[test]
fn fence_gate_axis_connections_cross_all_four_horizontal_subchunk_boundaries() {
    let fixture = compiled_connected_fixture();
    let fence_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.wood_fence)
        .model_template()
        .unwrap();
    for (center, remote, face, bit, gate) in [
        (
            [8, 8, 0],
            [8, 8, 15],
            Face::NegativeZ,
            1_u32,
            fixture.gate_facing_x,
        ),
        (
            [15, 8, 8],
            [0, 8, 8],
            Face::PositiveX,
            2,
            fixture.gate_facing_z,
        ),
        (
            [8, 8, 15],
            [8, 8, 0],
            Face::PositiveZ,
            4,
            fixture.gate_facing_x,
        ),
        (
            [0, 8, 8],
            [15, 8, 8],
            Face::NegativeX,
            8,
            fixture.gate_facing_z,
        ),
    ] {
        let center_sub = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, fixture.wood_fence],
            &[(center, 1)],
        )]);
        let remote_sub = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, gate],
            &[(remote, 1)],
        )]);
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air),
            &fixture.assets,
            NetworkIdMode::Sequential,
            &neighbourhood_for(face, &remote_sub),
            &center_sub,
        );
        let center_word =
            u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8);
        let refs = mesh
            .model_refs()
            .iter()
            .filter(|reference| reference.words()[0] & 0xfff == center_word)
            .collect::<Vec<_>>();
        assert_eq!(refs.len(), 2, "face={face:?}");
        assert_eq!(refs[0].words()[1], fence_base, "face={face:?}");
        assert_eq!(refs[1].words()[1], fence_base + 1 + bit, "face={face:?}");
    }
}

struct CompiledStairFixture {
    assets: RuntimeAssets,
    air: u32,
    ids: [[u32; 4]; 2],
    cube: u32,
}

fn compiled_stair_fixture() -> &'static CompiledStairFixture {
    static FIXTURE: OnceLock<CompiledStairFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode stair registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .unwrap()
            .clone();
        let stairs = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
            .cloned()
            .collect::<Vec<_>>();
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .unwrap()
            .clone();
        assert_eq!(stairs.len(), 8);
        let mut ids = [[0; 4]; 2];
        for record in &stairs {
            ids[record.model_state.get(ModelStateField::Half).unwrap() as usize][record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap()
                as usize] = record.sequential_id;
        }
        let directory = tempfile::tempdir().expect("create stair render fixture");
        write_slab_render_pack(
            directory.path(),
            "oak_stairs",
            "unused_double",
            "unused_cube",
        );
        let compiled = compile_pack(
            directory.path(),
            &std::iter::once(air.clone())
                .chain(stairs)
                .chain(std::iter::once(cube.clone()))
                .collect::<Vec<_>>(),
        )
        .expect("compile stair render fixture");
        let blob = encode_blob(&compiled).expect("encode stair render fixture");
        CompiledStairFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode stair render fixture"),
            air: air.sequential_id,
            ids,
            cube: cube.sequential_id,
        }
    })
}

fn mesh_stair_placements(
    placements: &[([u8; 3], u32)],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_stair_fixture();
    let mut palette = vec![fixture.air];
    let indexed = placements
        .iter()
        .map(|&(coordinate, id)| {
            let index = palette
                .iter()
                .position(|&value| value == id)
                .unwrap_or_else(|| {
                    palette.push(id);
                    palette.len() - 1
                });
            (coordinate, index)
        })
        .collect::<Vec<_>>();
    let storage = packed_storage(4, &palette, &indexed);
    let sub = sub_chunk(vec![storage]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        &sub,
    )
}

#[test]
fn compiled_stairs_select_all_dragonfly_neighbor_shapes_before_lighting() {
    let fixture = compiled_stair_fixture();
    let north = fixture.ids[0][2];
    let east = fixture.ids[0][3];
    let west = fixture.ids[0][1];
    let center = [8, 8, 8];
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, north)
        .model_template()
        .unwrap();
    assert_eq!(
        fixture.assets.model_templates()[base as usize].flags,
        MODEL_TEMPLATE_FLAG_STAIR
    );
    for (name, neighbour, expected_shape) in [
        ("straight", None, 0),
        ("right inner", Some(([8, 8, 9], east)), 1),
        ("left inner", Some(([8, 8, 9], west)), 2),
        ("right outer", Some(([8, 8, 7], west)), 3),
        ("left outer", Some(([8, 8, 7], east)), 4),
    ] {
        let mut placements = vec![(center, north)];
        if let Some(neighbour) = neighbour {
            placements.push(neighbour);
        }
        let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
        let reference = mesh
            .model_refs()
            .iter()
            .find(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
            .unwrap();
        assert_eq!(reference.words()[1], base + expected_shape, "{name}");
        let quad_count =
            fixture.assets.model_templates()[(base + expected_shape) as usize].quad_count as usize;
        let lighting_start = reference.words()[2] as usize;
        let lighting_end = mesh
            .model_refs()
            .iter()
            .map(|reference| reference.words()[2] as usize)
            .filter(|&start| start > lighting_start)
            .min()
            .unwrap_or(mesh.model_lighting().len());
        assert_eq!(
            lighting_end - lighting_start,
            quad_count,
            "{name} selected lighting span"
        );
        assert!(
            mesh.cube_quads().is_empty(),
            "{name} never enters cube stream"
        );
    }
}

const fn stair_offset(facing: usize) -> [i8; 3] {
    match facing {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        _ => [1, 0, 0],
    }
}

fn offset_coordinate([x, y, z]: [u8; 3], [dx, dy, dz]: [i8; 3]) -> [u8; 3] {
    [
        (x as i16 + dx as i16) as u8,
        (y as i16 + dy as i16) as u8,
        (z as i16 + dz as i16) as u8,
    ]
}

fn center_stair_ref(mesh: &ChunkMesh, center: [u8; 3]) -> PackedModelRef {
    *mesh
        .model_refs()
        .iter()
        .find(|reference| {
            reference.words()[0] & 0xfff
                == u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8)
        })
        .unwrap()
}

#[test]
fn stair_all_orientations_upside_states_shapes_and_dragonfly_suppression() {
    let fixture = compiled_stair_fixture();
    let center = [8, 8, 8];
    for half in 0..2 {
        for facing in 0..4 {
            let current = fixture.ids[half][facing];
            let right = (facing + 1) & 3;
            let left = (facing + 3) & 3;
            let front = offset_coordinate(center, stair_offset(facing));
            let back = offset_coordinate(center, stair_offset((facing + 2) & 3));
            let right_side = offset_coordinate(center, stair_offset(right));
            let base = fixture
                .assets
                .resolve(NetworkIdMode::Sequential, current)
                .model_template()
                .unwrap();
            for (shape, neighbour) in [
                (0, None),
                (1, Some((back, fixture.ids[half][right]))),
                (2, Some((back, fixture.ids[half][left]))),
                (3, Some((front, fixture.ids[half][left]))),
                (4, Some((front, fixture.ids[half][right]))),
            ] {
                let mut placements = vec![(center, current)];
                if let Some(neighbour) = neighbour {
                    placements.push(neighbour);
                }
                let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
                let reference = center_stair_ref(&mesh, center);
                assert_eq!(
                    reference.words()[1],
                    base + shape,
                    "half={half} facing={facing} shape={shape}"
                );
                let count =
                    fixture.assets.model_templates()[(base + shape) as usize].quad_count as usize;
                let start = reference.words()[2] as usize;
                let end = mesh
                    .model_refs()
                    .iter()
                    .map(|reference| reference.words()[2] as usize)
                    .filter(|&next| next > start)
                    .min()
                    .unwrap_or(mesh.model_lighting().len());
                assert_eq!(
                    end - start,
                    count,
                    "half={half} facing={facing} shape={shape}"
                );
            }
            for placements in [
                vec![
                    (center, current),
                    (back, fixture.ids[half][right]),
                    (right_side, current),
                ],
                vec![
                    (center, current),
                    (front, fixture.ids[half][left]),
                    (right_side, current),
                ],
                vec![(center, current), (front, fixture.ids[1 - half][right])],
            ] {
                let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
                assert_eq!(
                    center_stair_ref(&mesh, center).words()[1],
                    base,
                    "suppression/half mismatch half={half} facing={facing}"
                );
            }
        }
    }
}

#[test]
fn stair_topology_crosses_all_horizontal_subchunk_boundaries_for_both_halves() {
    let fixture = compiled_stair_fixture();
    for half in 0..2 {
        for (facing, center, remote, boundary) in [
            (0, [8, 8, 15], [8, 8, 0], Face::PositiveZ),
            (1, [0, 8, 8], [15, 8, 8], Face::NegativeX),
            (2, [8, 8, 0], [8, 8, 15], Face::NegativeZ),
            (3, [15, 8, 8], [0, 8, 8], Face::PositiveX),
        ] {
            let current = fixture.ids[half][facing];
            let right = fixture.ids[half][(facing + 1) & 3];
            let base = fixture
                .assets
                .resolve(NetworkIdMode::Sequential, current)
                .model_template()
                .unwrap();
            let neighbour = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, right],
                &[(remote, 1)],
            )]);
            let mesh = mesh_stair_placements(
                &[(center, current)],
                &neighbourhood_for(boundary, &neighbour),
            );
            assert_eq!(
                center_stair_ref(&mesh, center).words()[1],
                base + 4,
                "half={half} facing={facing}"
            );
        }
    }
}

#[test]
fn stair_rotated_boundary_cull_faces_preserve_lighting_addresses() {
    let fixture = compiled_stair_fixture();
    let center = [8, 8, 8];
    for half in 0..2 {
        for facing in 0..4 {
            let current = fixture.ids[half][facing];
            let neighbour = offset_coordinate(center, stair_offset(facing));
            let open = mesh_stair_placements(&[(center, current)], &Neighbourhood::empty());
            let culled = mesh_stair_placements(
                &[(center, current), (neighbour, fixture.cube)],
                &Neighbourhood::empty(),
            );
            let open_ref = center_stair_ref(&open, center);
            let culled_ref = center_stair_ref(&culled, center);
            assert_eq!(culled_ref.words()[1], open_ref.words()[1]);
            assert_eq!(culled_ref.words()[2], 0);
            assert_ne!(
                culled_ref.words()[3],
                open_ref.words()[3],
                "half={half} facing={facing} transformed cull face"
            );
            assert_eq!(
                open_ref.words()[3].count_ones() - culled_ref.words()[3].count_ones(),
                4,
                "half={half} facing={facing} full stair side must map all four canonical half-cell faces"
            );
            let count = fixture.assets.model_templates()[culled_ref.words()[1] as usize].quad_count
                as usize;
            assert_eq!(
                culled.model_lighting().len(),
                count,
                "half={half} facing={facing} lighting address span"
            );
        }
    }
}

#[test]
fn stair_topology_crosses_horizontal_subchunk_boundaries_and_missing_is_straight() {
    let fixture = compiled_stair_fixture();
    let north = fixture.ids[1][2];
    let east = fixture.ids[1][3];
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, north)
        .model_template()
        .unwrap();
    let remote = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, east],
        &[([8, 8, 15], 1)],
    )]);
    let center = [8, 8, 0];
    let crossed = mesh_stair_placements(
        &[(center, north)],
        &Neighbourhood::empty().with_negative_z(&remote),
    );
    assert_eq!(
        crossed.model_refs()[0].words()[1],
        base + 4,
        "upside-down left outer across -Z"
    );
    let missing = mesh_stair_placements(&[(center, north)], &Neighbourhood::empty());
    assert_eq!(
        missing.model_refs()[0].words()[1],
        base,
        "missing neighbour remains conservative straight"
    );
    assert!(
        missing.connectivity().is_all_connected(),
        "stairs remain conservative partial connectivity"
    );
}

fn mesh<'a>(
    classifier: &BlockClassifier,
    mode: NetworkIdMode,
    neighbours: &Neighbourhood<'a>,
    sub_chunk: &SubChunk,
) -> render::ChunkMesh {
    mesh_sub_chunk(classifier, runtime_assets(), mode, neighbours, sub_chunk)
}

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

fn packed_storage(bits_per_index: u8, palette: &[u32], placements: &[([u8; 3], usize)]) -> Vec<u8> {
    assert!(bits_per_index > 0);
    let values_per_word = 32 / usize::from(bits_per_index);
    let word_count = 4096_usize.div_ceil(values_per_word);
    let mut words = vec![0_u32; word_count];
    let mask = (1_u32 << bits_per_index) - 1;

    for &([x, y, z], palette_index) in placements {
        assert!(x < 16 && y < 16 && z < 16);
        assert!(palette_index < palette.len());
        assert!((palette_index as u32) <= mask);
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        let shift = (linear % values_per_word) * usize::from(bits_per_index);
        words[linear / values_per_word] |= (palette_index as u32) << shift;
    }

    let mut encoded = vec![(bits_per_index << 1) | 1];
    for word in words {
        encoded.extend_from_slice(&word.to_le_bytes());
    }
    encoded.extend(zig_zag_i32(palette.len() as i32));
    for &runtime_id in palette {
        encoded.extend(zig_zag_i32(runtime_id as i32));
    }
    encoded
}

fn uniform_storage(runtime_id: u32) -> Vec<u8> {
    let mut encoded = vec![1];
    encoded.extend(zig_zag_i32(runtime_id as i32));
    encoded
}

fn sub_chunk(storages: Vec<Vec<u8>>) -> SubChunk {
    let mut encoded = vec![9, storages.len() as u8, 0];
    for storage in storages {
        encoded.extend(storage);
    }
    SubChunk::decode(&encoded).expect("decode test sub-chunk")
}

fn blocks(runtime_id: u32, coordinates: &[[u8; 3]]) -> SubChunk {
    let placements = coordinates
        .iter()
        .copied()
        .map(|coordinate| (coordinate, 1))
        .collect::<Vec<_>>();
    sub_chunk(vec![packed_storage(1, &[AIR, runtime_id], &placements)])
}

fn uniform(runtime_id: u32) -> SubChunk {
    sub_chunk(vec![uniform_storage(runtime_id)])
}

fn adjacent_blocks(left: u32, right: u32) -> SubChunk {
    sub_chunk(vec![packed_storage(
        2,
        &[AIR, left, right],
        &[([7, 8, 8], 1), ([8, 8, 8], 2)],
    )])
}

fn slab(runtime_id: u32) -> SubChunk {
    let placements = (0..16)
        .flat_map(|y| (0..16).map(move |z| ([8, y, z], 1)))
        .collect::<Vec<_>>();
    sub_chunk(vec![packed_storage(1, &[AIR, runtime_id], &placements)])
}

fn has_face(mesh: &render::ChunkMesh, origin: [u8; 3], face: Face) -> bool {
    mesh.quads()
        .iter()
        .any(|quad| quad.origin() == origin && quad.face() == face)
}

fn neighbourhood_for<'a>(face: Face, neighbour: &'a SubChunk) -> Neighbourhood<'a> {
    match face {
        Face::NegativeX => Neighbourhood::empty().with_negative_x(neighbour),
        Face::PositiveX => Neighbourhood::empty().with_positive_x(neighbour),
        Face::NegativeY => Neighbourhood::empty().with_negative_y(neighbour),
        Face::PositiveY => Neighbourhood::empty().with_positive_y(neighbour),
        Face::NegativeZ => Neighbourhood::empty().with_negative_z(neighbour),
        Face::PositiveZ => Neighbourhood::empty().with_positive_z(neighbour),
    }
}

#[test]
fn one_opaque_block_emits_six_packed_quads() {
    let sub = blocks(7, &[[1, 2, 3]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(mesh.quad_count(), 6);
    assert_eq!(mesh.quads().len(), 6);
    assert!(mesh.quads().iter().all(|quad| quad.origin() == [1, 2, 3]));
    assert!(mesh.quads().iter().all(|quad| quad.width() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.height() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.material_id() == 7));
    assert_eq!(mesh.quads()[0].face(), Face::NegativeX);
    assert_eq!(mesh.quads()[0].words(), [1 | (2 << 5) | (3 << 10), 7]);
}

#[test]
fn equal_adjacent_blocks_greedy_merge_into_six_prism_quads() {
    let sub = blocks(11, &[[0, 0, 0], [1, 0, 0]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert_eq!(
        mesh.quads().iter().filter(|quad| quad.width() == 2).count(),
        4,
        "top, bottom, front, and back should span both X cells"
    );
}

#[test]
fn different_materials_split_coplanar_runs_but_still_cull_internal_faces() {
    let placements = [([0, 0, 0], 1), ([1, 0, 0], 2)];
    let sub = sub_chunk(vec![packed_storage(2, &[AIR, 13, 17], &placements)]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 10);
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == 13)
            .count(),
        5
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == 17)
            .count(),
        5
    );
}

#[test]
fn asymmetric_internal_culling_uses_ordered_occluder_and_leaf_facts() {
    let cases = [
        (OPAQUE_A, OPAQUE_B, false, false, 10),
        (OPAQUE_A, LEAF_A, true, false, 11),
        (LEAF_A, OPAQUE_A, false, true, 11),
        (LEAF_A, LEAF_B, false, false, 10),
        (DIAGNOSTIC, LEAF_A, true, true, 12),
        (DIAGNOSTIC, OPAQUE_A, false, true, 11),
    ];

    for (source, neighbour, source_face, neighbour_face, total) in cases {
        let sub = adjacent_blocks(source, neighbour);
        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &sub,
        );

        assert_eq!(
            has_face(&mesh, [7, 8, 8], Face::PositiveX),
            source_face,
            "source={source} neighbour={neighbour}"
        );
        assert_eq!(
            has_face(&mesh, [8, 8, 8], Face::NegativeX),
            neighbour_face,
            "source={source} neighbour={neighbour}"
        );
        assert_eq!(
            mesh.quad_count(),
            total,
            "source={source} neighbour={neighbour}"
        );
    }
}

#[test]
fn asymmetric_boundary_culling_matches_internal_semantics_on_every_face() {
    let boundaries = [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
    ];
    let pairs = [
        (OPAQUE_A, OPAQUE_B, 5),
        (OPAQUE_A, LEAF_A, 6),
        (LEAF_A, OPAQUE_A, 5),
        (LEAF_A, LEAF_B, 5),
        (DIAGNOSTIC, OPAQUE_A, 5),
        (DIAGNOSTIC, LEAF_A, 6),
        (DIAGNOSTIC, DIAGNOSTIC, 6),
        (OPAQUE_A, DIAGNOSTIC, 6),
        (LEAF_A, DIAGNOSTIC, 6),
    ];

    for (face, current_coordinate, neighbour_coordinate) in boundaries {
        for (source, neighbour_value, expected) in pairs {
            let sub = blocks(source, &[current_coordinate]);
            let neighbour = blocks(neighbour_value, &[neighbour_coordinate]);
            let neighbourhood = neighbourhood_for(face, &neighbour);
            let mesh = mesh(
                &classifier(),
                NetworkIdMode::Sequential,
                &neighbourhood,
                &sub,
            );

            assert_eq!(
                mesh.quad_count(),
                expected,
                "face={face:?} source={source} neighbour={neighbour_value}"
            );
            assert_eq!(
                has_face(&mesh, current_coordinate, face),
                expected == 6,
                "face={face:?} source={source} neighbour={neighbour_value}"
            );
        }
    }
}

#[test]
fn every_boundary_face_culls_against_its_cross_sub_chunk_neighbour() {
    let cases = [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
    ];

    for (face, current_coordinate, neighbour_coordinate) in cases {
        let sub = blocks(23, &[current_coordinate]);
        let neighbour = blocks(23, &[neighbour_coordinate]);
        let neighbourhood = match face {
            Face::NegativeX => Neighbourhood::empty().with_negative_x(&neighbour),
            Face::PositiveX => Neighbourhood::empty().with_positive_x(&neighbour),
            Face::NegativeY => Neighbourhood::empty().with_negative_y(&neighbour),
            Face::PositiveY => Neighbourhood::empty().with_positive_y(&neighbour),
            Face::NegativeZ => Neighbourhood::empty().with_negative_z(&neighbour),
            Face::PositiveZ => Neighbourhood::empty().with_positive_z(&neighbour),
        };

        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &neighbourhood,
            &sub,
        );

        assert_eq!(mesh.quad_count(), 5, "failed to cull {face:?}");
        assert!(
            mesh.quads().iter().all(|quad| quad.face() != face),
            "retained cross-boundary {face:?}"
        );
    }
}

#[test]
fn zero_storage_and_uniform_air_emit_no_geometry() {
    let no_storage = sub_chunk(Vec::new());
    let uniform_air = uniform(AIR);

    let no_storage_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &no_storage,
    );
    let uniform_air_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform_air,
    );

    assert!(no_storage_mesh.is_empty());
    assert!(uniform_air_mesh.is_empty());
    for face in Face::ALL {
        for other in Face::ALL {
            assert!(no_storage_mesh.connectivity().is_connected(face, other));
            assert!(uniform_air_mesh.connectivity().is_connected(face, other));
        }
    }
}

#[test]
fn layered_solid_and_water_are_both_resolved() {
    let solid = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![solid, water]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    );
    assert_eq!(resolved.palette_entry_count(), 4);
    let resolved = resolved.resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn layered_aquatic_cross_and_water_emit_model_without_diagnostic_cube() {
    let seagrass = packed_storage(1, &[AIR, CROSS], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![seagrass, water]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(CROSS));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert!(mesh.cube_quads().is_empty());
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_lighting().len(), 2);
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn uniform_solid_and_water_resolve_all_layers() {
    let sub = sub_chunk(vec![uniform_storage(OPAQUE_A), uniform_storage(LIQUID_A)]);
    let resolver = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    );
    assert_eq!(resolver.palette_entry_count(), 2);
    let resolved = resolver.resolve([15, 15, 15]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

#[test]
fn contributor_resolver_rejects_out_of_bounds_coordinates_consistently() {
    let uniform = uniform(OPAQUE_A);
    let mixed = blocks(OPAQUE_A, &[[8, 8, 8]]);
    for sub_chunk in [&uniform, &mixed] {
        let resolved = ContributorResolver::new(
            classifier(),
            runtime_assets(),
            NetworkIdMode::Sequential,
            sub_chunk,
        )
        .resolve([16, 0, 0]);
        assert_eq!(resolved.primary_network_value(), None);
        assert_eq!(resolved.liquid_network_value(), None);
        assert_eq!(resolved.diagnostic_network_value(), Some(0));
    }
}

#[test]
fn liquid_before_solid_is_order_independent() {
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let solid = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![water, solid]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

#[test]
fn liquid_only_is_retained_without_diagnostic_cube() {
    let water = blocks(LIQUID_A, &[[8, 8, 8]]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &water,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &water,
    );

    assert!(mesh.cube_quads().is_empty());
    assert!(mesh.model_refs().is_empty());
}

#[test]
fn duplicate_liquid_collapses() {
    let first = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let duplicate = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![first, duplicate]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.cube_quads().is_empty());
    assert!(mesh.model_refs().is_empty());
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn two_primary_layers_fail_closed() {
    for second_runtime_id in [OPAQUE_A, OPAQUE_B] {
        let first = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
        let second = packed_storage(1, &[AIR, second_runtime_id], &[([8, 8, 8], 1)]);
        let sub = sub_chunk(vec![first, second]);

        let resolved = ContributorResolver::new(
            classifier(),
            runtime_assets(),
            NetworkIdMode::Sequential,
            &sub,
        )
        .resolve([8, 8, 8]);
        assert_eq!(resolved.primary_network_value(), None);
        assert_eq!(resolved.liquid_network_value(), None);
        assert_eq!(resolved.diagnostic_network_value(), Some(second_runtime_id));

        assert_single_diagnostic_voxel(&sub);
    }
}

#[test]
fn distinct_liquids_fail_closed() {
    let first = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let second = packed_storage(1, &[AIR, LIQUID_B], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![first, second]);

    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), None);
    assert_eq!(resolved.diagnostic_network_value(), Some(LIQUID_B));

    assert_single_diagnostic_voxel(&sub);
}

#[test]
fn unsupported_additional_layer_fails_closed() {
    let unsupported = blocks(UNSUPPORTED_ADDITIONAL, &[[8, 8, 8]]);

    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &unsupported,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), None);
    assert_eq!(
        resolved.diagnostic_network_value(),
        Some(UNSUPPORTED_ADDITIONAL)
    );

    assert_single_diagnostic_voxel(&unsupported);
}

#[test]
fn sixteen_storage_layers_resolve_without_flattening() {
    let mut layers = vec![uniform_storage(AIR); world::MAX_STORAGE_COUNT - 1];
    layers.push(packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]));
    let sub = sub_chunk(layers);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

fn assert_single_diagnostic_voxel(sub_chunk: &SubChunk) {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        sub_chunk,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.model_refs().is_empty());
    assert!(mesh.liquid_quads().is_empty());
}

#[test]
fn debug_colours_are_deterministic_distinct_and_opaque() {
    assert_eq!(debug_color(0xdead_beef), debug_color(0xdead_beef));
    assert_ne!(debug_color(7), debug_color(8));
    assert_eq!(debug_color(7)[3], 255);
    assert_eq!(debug_color(u32::MAX)[3], 255);
}

#[test]
fn uniform_solid_fast_path_merges_planes_and_respects_boundary_neighbours() {
    let sub = uniform(37);
    let empty_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(empty_mesh.quad_count(), 6);
    assert!(
        empty_mesh
            .quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(empty_mesh.connectivity().is_empty());

    let positive_x = uniform(41);
    let neighbourhood = Neighbourhood::empty().with_positive_x(&positive_x);
    let culled_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &neighbourhood,
        &sub,
    );

    assert_eq!(culled_mesh.quad_count(), 5);
    assert!(
        culled_mesh
            .quads()
            .iter()
            .all(|quad| quad.face() != Face::PositiveX)
    );
}

#[test]
fn uniform_leaf_meshes_outer_planes_but_is_cave_open() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform(LEAF_A),
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(mesh.quads().iter().all(|quad| quad.material_id() == LEAF_A));
    assert!(mesh.connectivity().is_all_connected());
    assert_eq!(size_of::<PackedQuad>(), 8);
}

#[test]
fn uniform_diagnostic_emits_each_unculled_slice_and_is_cave_open() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform(DIAGNOSTIC),
    );

    assert_eq!(mesh.quad_count(), 96);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn leaf_slab_is_cave_open_while_opaque_slab_separates_opposite_faces() {
    let leaf = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &slab(LEAF_A),
    );
    assert!(leaf.connectivity().is_all_connected());

    let opaque = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &slab(OPAQUE_A),
    );
    assert!(
        !opaque
            .connectivity()
            .is_connected(Face::NegativeX, Face::PositiveX)
    );
}

#[test]
fn separate_primary_layers_resolve_per_coordinate() {
    let layer_zero = packed_storage(1, &[AIR, LEAF_A], &[([1, 1, 1], 1)]);
    let layer_one = packed_storage(1, &[AIR, OPAQUE_A], &[([2, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 11);
    assert!(!has_face(&mesh, [1, 1, 1], Face::PositiveX));
    assert!(has_face(&mesh, [2, 1, 1], Face::NegativeX));
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == LEAF_A)
            .count(),
        5
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == OPAQUE_A)
            .count(),
        6
    );
}

#[test]
fn classifier_air_collision_with_known_opaque_visual_remains_air_in_mixed_storage() {
    let collision_classifier = BlockClassifier::new(OPAQUE_A);
    let layer_zero = packed_storage(1, &[OPAQUE_A, OPAQUE_B], &[([8, 8, 8], 1)]);
    let layer_one = packed_storage(1, &[OPAQUE_A, LEAF_A], &[([1, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &collision_classifier,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 12);
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == LEAF_A)
            .count(),
        6
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == OPAQUE_B)
            .count(),
        6
    );
}

#[test]
fn classifier_non_air_collision_with_air_visual_stays_diagnostic_and_owns_the_voxel() {
    let collision_classifier = BlockClassifier::new(AIR - 1);
    let layer_zero = packed_storage(1, &[AIR - 1, AIR], &[([1, 1, 1], 1)]);
    let layer_one = packed_storage(1, &[AIR - 1, OPAQUE_A], &[([1, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &collision_classifier,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn configured_high_bit_air_is_empty_in_every_storage_layer() {
    const HASHED_AIR: u32 = 0xdbf4_4120;
    let classifier = BlockClassifier::new(HASHED_AIR);
    let sub = sub_chunk(vec![
        uniform_storage(HASHED_AIR),
        uniform_storage(HASHED_AIR),
    ]);

    let mesh = mesh(
        &classifier,
        NetworkIdMode::Hashed,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.is_empty());
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn empty_tunnel_connects_only_the_two_faces_it_reaches() {
    let tunnel = (0..16).map(|x| ([x, 8, 8], 1)).collect::<Vec<_>>();
    let sub = sub_chunk(vec![packed_storage(1, &[43, AIR], &tunnel)]);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    let connectivity = mesh.connectivity();

    assert!(connectivity.is_connected(Face::NegativeX, Face::PositiveX));
    assert!(connectivity.is_connected(Face::PositiveX, Face::NegativeX));
    assert!(!connectivity.is_connected(Face::NegativeX, Face::NegativeY));
    assert!(!connectivity.is_connected(Face::PositiveX, Face::PositiveZ));
}

#[test]
fn sealed_empty_cavity_has_no_face_connectivity() {
    let sub = sub_chunk(vec![packed_storage(1, &[47, AIR], &[([8, 8, 8], 1)])]);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.connectivity().is_empty());
}

#[test]
fn explicit_network_mode_preserves_high_hashes_and_isolates_low_collisions() {
    let high_hash = runtime_assets().resolve(NetworkIdMode::Hashed, 0xdbf4_4120);
    assert!(high_hash.is_known());
    assert_eq!(high_hash.face(BlockFace::Up).material_id(), 64);

    let sequential = runtime_assets().resolve(NetworkIdMode::Sequential, 7);
    let colliding_hash = runtime_assets().resolve(NetworkIdMode::Hashed, 7);
    assert_eq!(sequential.face(BlockFace::West).material_id(), 7);
    assert_eq!(colliding_hash.face(BlockFace::West).material_id(), 66);

    let sub = blocks(7, &[[1, 2, 3]]);
    let hashed_mesh = mesh(
        &BlockClassifier::new(0xdbf4_4120),
        NetworkIdMode::Hashed,
        &Neighbourhood::empty(),
        &sub,
    );
    assert!(
        hashed_mesh
            .quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
}

#[test]
fn greedy_merge_identity_is_face_material_not_network_value() {
    let same_material = sub_chunk(vec![packed_storage(
        2,
        &[AIR, 51, 52],
        &[([0, 0, 0], 1), ([1, 0, 0], 2)],
    )]);
    let merged = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &same_material,
    );
    assert_eq!(merged.quad_count(), 6);
    assert!(merged.quads().iter().all(|quad| quad.material_id() == 51));

    let different_materials = sub_chunk(vec![packed_storage(
        2,
        &[AIR, 13, 17],
        &[([0, 0, 0], 1), ([1, 0, 0], 2)],
    )]);
    let split = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &different_materials,
    );
    assert_eq!(split.quad_count(), 10);
}

#[test]
fn exact_face_materials_and_diagnostic_fallback_are_packed() {
    let face_mapped = blocks(53, &[[4, 5, 6]]);
    let face_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &face_mapped,
    );
    let expected = [61, 62, 63, 64, 65, 66];
    for face in Face::ALL {
        let quad = face_mesh
            .quads()
            .iter()
            .find(|quad| quad.face() == face)
            .expect("one quad per face");
        assert_eq!(quad.material_id(), expected[face as usize]);
    }

    for runtime_id in [54, 50_000] {
        let sub = blocks(runtime_id, &[[4, 5, 6]]);
        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &sub,
        );
        assert!(
            mesh.quads()
                .iter()
                .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL),
            "runtime value {runtime_id} bypassed diagnostic material"
        );
    }
}
