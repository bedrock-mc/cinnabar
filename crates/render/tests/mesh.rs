use std::{mem::size_of, sync::OnceLock};

use assets::{
    BlockFace, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, DIAGNOSTIC_MATERIAL,
    Material, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray,
    TextureMip, TexturePage, TextureRef, VisualKind, encode_blob,
};
use render::{
    BlockClassifier, ChunkMesh, Face, FaceConnectivity, Neighbourhood, PackedLiquidQuad,
    PackedModelRef, PackedQuad, PackedQuadLighting, debug_color, mesh_sub_chunk,
};
use world::SubChunk;

const AIR: u32 = 12_530;
const OPAQUE_A: u32 = 7;
const OPAQUE_B: u32 = 13;
const DIAGNOSTIC: u32 = 54;
const LEAF_A: u32 = 55;
const LEAF_B: u32 = 56;

#[test]
fn packed_stream_record_sizes() {
    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(size_of::<PackedModelRef>(), 16);
    assert_eq!(size_of::<PackedQuadLighting>(), 8);
    assert_eq!(size_of::<PackedLiquidQuad>(), 16);

    let model = PackedModelRef::new(1, 2, 3, 0xa5a5_5a5a);
    let lighting = PackedQuadLighting::new([0x00f0, 0x01f0, 0x02f0, 0x03f0]);
    let liquid = PackedLiquidQuad::new([4, 5, 6, 7]);
    let mesh = ChunkMesh::from_streams(
        Vec::new(),
        vec![model],
        vec![lighting],
        vec![liquid],
        vec![lighting],
        FaceConnectivity::all(),
    );

    assert!(mesh.cube_quads().is_empty());
    assert_eq!(mesh.model_refs(), &[model]);
    assert_eq!(mesh.model_lighting(), &[lighting]);
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
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(textures)].into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
        };
        let blob = encode_blob(&compiled).expect("encode synthetic mesher assets");
        RuntimeAssets::decode(&blob).expect("decode synthetic mesher assets")
    })
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
fn first_non_air_storage_layer_selects_the_debug_material() {
    let layer_zero = packed_storage(1, &[AIR, 29], &[([0, 0, 0], 1)]);
    let layer_one = packed_storage(1, &[AIR, 31], &[([0, 0, 0], 1), ([2, 0, 0], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    let materials = mesh
        .quads()
        .iter()
        .map(PackedQuad::material_id)
        .collect::<Vec<_>>();

    assert_eq!(mesh.quad_count(), 12);
    assert_eq!(materials.iter().filter(|&&id| id == 29).count(), 6);
    assert_eq!(materials.iter().filter(|&&id| id == 31).count(), 6);
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
fn first_non_air_palette_layer_controls_leaf_facts_and_face_material() {
    let layer_zero = packed_storage(1, &[AIR, LEAF_A], &[([1, 1, 1], 1)]);
    let layer_one = packed_storage(1, &[AIR, OPAQUE_A], &[([1, 1, 1], 1), ([2, 1, 1], 1)]);
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
