use std::mem::size_of;

use render::{BlockClassifier, Face, Neighbourhood, PackedQuad, debug_color, mesh_sub_chunk};
use world::SubChunk;

const AIR: u32 = 12_530;

fn classifier() -> BlockClassifier {
    BlockClassifier::new(AIR)
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

#[test]
fn one_opaque_block_emits_six_packed_quads() {
    let sub = blocks(7, &[[1, 2, 3]]);
    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);

    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(mesh.quad_count(), 6);
    assert_eq!(mesh.quads().len(), 6);
    assert!(mesh.quads().iter().all(|quad| quad.origin() == [1, 2, 3]));
    assert!(mesh.quads().iter().all(|quad| quad.width() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.height() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.runtime_id() == 7));
    assert_eq!(mesh.quads()[0].face(), Face::NegativeX);
    assert_eq!(mesh.quads()[0].words(), [1 | (2 << 5) | (3 << 10), 7]);
}

#[test]
fn equal_adjacent_blocks_greedy_merge_into_six_prism_quads() {
    let sub = blocks(11, &[[0, 0, 0], [1, 0, 0]]);
    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);

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
    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);

    assert_eq!(mesh.quad_count(), 10);
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.runtime_id() == 13)
            .count(),
        5
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.runtime_id() == 17)
            .count(),
        5
    );
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

        let mesh = mesh_sub_chunk(&classifier(), &neighbourhood, &sub);

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

    let no_storage_mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &no_storage);
    let uniform_air_mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &uniform_air);

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

    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);
    let materials = mesh
        .quads()
        .iter()
        .map(PackedQuad::runtime_id)
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
    let empty_mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);

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
    let culled_mesh = mesh_sub_chunk(&classifier(), &neighbourhood, &sub);

    assert_eq!(culled_mesh.quad_count(), 5);
    assert!(
        culled_mesh
            .quads()
            .iter()
            .all(|quad| quad.face() != Face::PositiveX)
    );
}

#[test]
fn configured_high_bit_air_is_empty_in_every_storage_layer() {
    const HASHED_AIR: u32 = 0xdbf4_4120;
    let classifier = BlockClassifier::new(HASHED_AIR);
    let sub = sub_chunk(vec![
        uniform_storage(HASHED_AIR),
        uniform_storage(HASHED_AIR),
    ]);

    let mesh = mesh_sub_chunk(&classifier, &Neighbourhood::empty(), &sub);

    assert!(mesh.is_empty());
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn empty_tunnel_connects_only_the_two_faces_it_reaches() {
    let tunnel = (0..16).map(|x| ([x, 8, 8], 1)).collect::<Vec<_>>();
    let sub = sub_chunk(vec![packed_storage(1, &[43, AIR], &tunnel)]);

    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);
    let connectivity = mesh.connectivity();

    assert!(connectivity.is_connected(Face::NegativeX, Face::PositiveX));
    assert!(connectivity.is_connected(Face::PositiveX, Face::NegativeX));
    assert!(!connectivity.is_connected(Face::NegativeX, Face::NegativeY));
    assert!(!connectivity.is_connected(Face::PositiveX, Face::PositiveZ));
}

#[test]
fn sealed_empty_cavity_has_no_face_connectivity() {
    let sub = sub_chunk(vec![packed_storage(1, &[47, AIR], &[([8, 8, 8], 1)])]);

    let mesh = mesh_sub_chunk(&classifier(), &Neighbourhood::empty(), &sub);

    assert!(mesh.connectivity().is_empty());
}
