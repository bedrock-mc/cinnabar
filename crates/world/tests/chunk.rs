use world::{MeshNeighbourhood, MeshSample, SubChunk};

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
    let offsets = (-1_i8..=1)
        .flat_map(|x| (-1_i8..=1).flat_map(move |y| (-1_i8..=1).map(move |z| [x, y, z])))
        .filter(|offset| *offset != [0, 0, 0])
        .collect::<Vec<_>>();
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
