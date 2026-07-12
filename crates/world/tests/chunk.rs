use std::collections::BTreeSet;

use world::{MeshNeighbourhood, MeshSample, SubChunk, SubChunkKey};

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

fn uniform(runtime_id: u32) -> SubChunk {
    let mut bytes = vec![9, 1, 0, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    SubChunk::decode(&bytes).expect("decode uniform test sub-chunk")
}

#[test]
fn mesh_neighbourhood_reaches_all_26_adjacent_subchunks() {
    let center = uniform(100);
    let offsets = MeshNeighbourhood::adjacent_offsets().collect::<Vec<_>>();
    assert_eq!(MeshNeighbourhood::ADJACENT_SUB_CHUNK_COUNT, 26);
    assert_eq!(offsets.len(), 26);
    assert_eq!(offsets.iter().copied().collect::<BTreeSet<_>>().len(), 26);
    assert!(!offsets.contains(&[0, 0, 0]));
    let neighbours = (0..offsets.len())
        .map(|index| uniform(200 + index as u32))
        .collect::<Vec<_>>();
    let mut neighbourhood = MeshNeighbourhood::new(&center);

    for (&offset, neighbour) in offsets.iter().zip(&neighbours) {
        assert!(neighbourhood.insert(offset, neighbour));
    }

    assert_eq!(
        neighbourhood
            .sub_chunk([0, 0, 0])
            .and_then(|sub_chunk| sub_chunk.runtime_id(0, 0, 0, 0)),
        Some(100)
    );
    for (index, &offset) in offsets.iter().enumerate() {
        assert_eq!(
            neighbourhood
                .sub_chunk(offset)
                .and_then(|sub_chunk| sub_chunk.runtime_id(0, 0, 0, 0)),
            Some(200 + index as u32),
            "offset {offset:?} must retain its exact adjacent sub-chunk"
        );
    }
    assert!(!neighbourhood.insert([2, 0, 0], &neighbours[0]));
}

#[test]
fn missing_boundary_samples_use_explicit_open_fallback() {
    let center = uniform(77);
    let neighbourhood = MeshNeighbourhood::new(&center);

    assert_eq!(neighbourhood.sample(0, [15, 4, 9]), MeshSample::Block(77));
    assert_eq!(neighbourhood.sample(0, [16, 4, 9]), MeshSample::Open);
    assert_eq!(neighbourhood.sample(0, [-1, 4, 9]), MeshSample::Open);
    assert_eq!(neighbourhood.sample(0, [32, 4, 9]), MeshSample::Open);
}

#[test]
fn liquid_sample_offsets_are_the_exact_deduplicated_nineteen_subchunks() {
    let offsets = MeshNeighbourhood::liquid_sample_offsets().collect::<Vec<_>>();
    assert_eq!(MeshNeighbourhood::LIQUID_SAMPLE_SUB_CHUNK_COUNT, 19);
    let unique = offsets.iter().copied().collect::<BTreeSet<_>>();
    let expected = std::iter::once([0, -1, 0])
        .chain((-1_i8..=1).flat_map(|x| {
            [0_i8, 1]
                .into_iter()
                .flat_map(move |y| (-1_i8..=1).map(move |z| [x, y, z]))
        }))
        .collect::<BTreeSet<_>>();

    assert_eq!(offsets.len(), 19);
    assert_eq!(unique.len(), offsets.len());
    assert_eq!(unique, expected);
    assert_eq!(offsets.iter().filter(|offset| offset[1] == -1).count(), 1);
    assert_eq!(offsets.iter().filter(|offset| offset[1] == 0).count(), 9);
    assert_eq!(offsets.iter().filter(|offset| offset[1] == 1).count(), 9);
    assert!(unique.contains(&[0, -1, 0]));
    assert!(unique.contains(&[-1, 0, -1]));
    assert!(unique.contains(&[1, 0, 1]));
    assert!(unique.contains(&[-1, 1, -1]));
    assert!(unique.contains(&[1, 1, 1]));
    assert!(!unique.contains(&[1, -1, 0]));
    assert!(!unique.contains(&[0, -1, 1]));
}

#[test]
fn liquid_accessors_reach_only_the_bounded_sample_set() {
    let center = uniform(100);
    let offsets = MeshNeighbourhood::liquid_sample_offsets().collect::<Vec<_>>();
    let neighbours = (0..offsets.len() - 1)
        .map(|index| uniform(200 + index as u32))
        .collect::<Vec<_>>();
    let mut neighbourhood = MeshNeighbourhood::new(&center);
    let mut neighbour_index = 0;
    for &offset in &offsets {
        if offset == [0, 0, 0] {
            continue;
        }
        assert!(neighbourhood.insert(offset, &neighbours[neighbour_index]));
        neighbour_index += 1;
    }
    let outside = uniform(999);
    assert!(neighbourhood.insert([1, -1, 0], &outside));

    let retained = neighbourhood.liquid_sub_chunks().collect::<Vec<_>>();
    assert_eq!(retained.len(), 19);
    assert_eq!(
        retained
            .iter()
            .map(|(offset, _)| *offset)
            .collect::<Vec<_>>(),
        offsets
    );
    assert!(retained.iter().all(|(_, sub_chunk)| sub_chunk.is_some()));
    let center_only = MeshNeighbourhood::new(&center);
    let missing = center_only.liquid_sub_chunks().collect::<Vec<_>>();
    assert_eq!(missing.len(), 19);
    assert_eq!(
        missing
            .iter()
            .filter(|(_, sub_chunk)| sub_chunk.is_some())
            .count(),
        1
    );
    assert_eq!(
        neighbourhood.liquid_sample(0, [-1, 8, -1]),
        MeshSample::Block(
            neighbourhood
                .sub_chunk([-1, 0, -1])
                .unwrap()
                .runtime_id(0, 15, 8, 15)
                .unwrap()
        )
    );
    assert!(neighbourhood.liquid_block_source([16, 16, 16]).is_some());
    assert_eq!(
        neighbourhood.liquid_sample(0, [8, -1, 8]),
        MeshSample::Block(
            neighbourhood
                .sub_chunk([0, -1, 0])
                .unwrap()
                .runtime_id(0, 8, 15, 8)
                .unwrap()
        )
    );
    assert_eq!(
        neighbourhood.liquid_sample(0, [16, -1, 8]),
        MeshSample::Open
    );
    assert!(neighbourhood.liquid_block_source([16, -1, 8]).is_none());
}

#[test]
fn liquid_mesh_dependents_are_the_checked_inverse_sample_set() {
    let source = SubChunkKey::new(7, 20, -4, -30);
    let actual = source.liquid_mesh_dependents().collect::<Vec<_>>();
    let unique = actual.iter().copied().collect::<BTreeSet<_>>();
    let expected_offsets = std::iter::once([0_i8, -1, 0]).chain((-1_i8..=1).flat_map(|x| {
        [0_i8, 1]
            .into_iter()
            .flat_map(move |y| (-1_i8..=1).map(move |z| [x, y, z]))
    }));
    let expected = expected_offsets
        .map(|[dx, dy, dz]| {
            SubChunkKey::new(
                source.dimension,
                source.x - i32::from(dx),
                source.y - i32::from(dy),
                source.z - i32::from(dz),
            )
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(actual.len(), 19);
    assert_eq!(unique.len(), actual.len());
    assert_eq!(unique, expected);
    assert_eq!(
        SubChunkKey::new(7, i32::MAX, i32::MIN, i32::MAX)
            .liquid_mesh_dependents()
            .count(),
        5
    );
    assert_eq!(
        SubChunkKey::new(7, i32::MIN, i32::MAX, i32::MIN)
            .liquid_mesh_dependents()
            .count(),
        8
    );
}
