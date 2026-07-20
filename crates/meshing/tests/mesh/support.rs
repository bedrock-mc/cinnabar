fn mesh<'a>(
    classifier: &BlockClassifier,
    mode: NetworkIdMode,
    neighbours: &Neighbourhood<'a>,
    sub_chunk: &SubChunk,
) -> meshing::ChunkMesh {
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

fn has_face(mesh: &meshing::ChunkMesh, origin: [u8; 3], face: Face) -> bool {
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
