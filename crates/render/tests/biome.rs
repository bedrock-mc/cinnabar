use std::sync::Arc;

use render::{
    BIOME_NEIGHBOUR_SLOT_COUNT, MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord,
    biome_neighbour_index,
};
use world::DecodedBiomeColumn;

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

#[test]
fn uniform_record_remaps_only_the_palette() {
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(42));
    let storage = DecodedBiomeColumn::decode(-4, 1, &payload)
        .unwrap()
        .storage(-4)
        .unwrap();

    let record = PackedBiomeRecord::from_storage(&storage, |id| id + 1000);

    assert_eq!(&record.words()[11..], &[1 << 8, 1042]);
    assert_eq!(record.bits_per_index(), 0);
    assert_eq!(record.palette_len(), 1);
    assert_eq!(record.tint_index(15, 15, 15), Some(1042));
}

#[test]
fn packed_record_preserves_bedrock_words_and_xzy_lookup() {
    let mut payload = vec![3]; // Network palette, one bit per index.
    let mut packed = vec![0_u32; 128];
    let linear = (1_usize << 8) | (3_usize << 4) | 2;
    packed[linear / 32] |= 1 << (linear % 32);
    for word in &packed {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(7));
    payload.extend(zig_zag_i32(9));
    let storage = DecodedBiomeColumn::decode(0, 1, &payload)
        .unwrap()
        .storage(0)
        .unwrap();

    let record = PackedBiomeRecord::from_storage(&storage, |id| id * 10);

    assert_eq!(record.bits_per_index(), 1);
    assert_eq!(record.palette_len(), 2);
    assert_eq!(&record.words()[12..140], packed.as_slice());
    assert_eq!(&record.words()[140..], &[70, 90]);
    assert_eq!(record.tint_index(1, 2, 3), Some(90));
    assert_eq!(record.tint_index(1, 3, 2), Some(70));
}

#[test]
fn fallback_record_is_a_valid_uniform_palette() {
    let record = PackedBiomeRecord::fallback();

    assert_eq!(&record.words()[11..], &[1 << 8, 0]);
    assert_eq!(record.tint_index(0, 0, 0), Some(0));
    assert_eq!(record.tint_index(16, 0, 0), None);
}

fn uniform_storage(id: i32) -> Arc<world::BiomeStorage> {
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(id));
    DecodedBiomeColumn::decode(0, 1, &payload)
        .unwrap()
        .storage(0)
        .unwrap()
}

#[test]
fn neighbourhood_record_samples_all_cross_chunk_slots_without_flattening() {
    let center = uniform_storage(10);
    let east = uniform_storage(20);
    let north_west = uniform_storage(30);
    let mut halo: [Option<Arc<world::BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT] =
        std::array::from_fn(|_| None);
    halo[biome_neighbour_index(0, 0).unwrap()] = Some(center);
    halo[biome_neighbour_index(1, 0).unwrap()] = Some(east);
    halo[biome_neighbour_index(-1, -1).unwrap()] = Some(north_west);

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + 1_000);

    assert_eq!(record.tint_index_at([15, 7, 8]), Some(1_010));
    assert_eq!(record.tint_index_at([16, 7, 8]), Some(1_020));
    assert_eq!(record.tint_index_at([-1, 7, -1]), Some(1_030));
    assert_eq!(
        record.blend_tint_indices([15, 7, 8]),
        Some([
            1_010, 1_010, 1_020, 1_010, 1_010, 1_020, 1_010, 1_010, 1_020
        ])
    );
}

#[test]
fn every_horizontal_boundary_and_corner_uses_its_exact_neighbour_slot() {
    let halo = std::array::from_fn(|slot| Some(uniform_storage(slot as i32 + 1)));
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id);

    for dz in -1_i8..=1 {
        for dx in -1_i8..=1 {
            let coordinate = [
                match dx {
                    -1 => -1,
                    0 => 0,
                    _ => 16,
                },
                8,
                match dz {
                    -1 => -1,
                    0 => 0,
                    _ => 16,
                },
            ];
            let slot = biome_neighbour_index(dx, dz).unwrap();
            assert_eq!(record.tint_index_at(coordinate), Some(slot as u32 + 1));
        }
    }
}

#[test]
fn missing_neighbour_clamps_to_the_centres_nearest_edge() {
    let mut payload = vec![3];
    let mut packed = vec![0_u32; 128];
    for z in 0..16_usize {
        for y in 0..16_usize {
            let linear = (15 << 8) | (z << 4) | y;
            packed[linear / 32] |= 1 << (linear % 32);
        }
    }
    for word in packed {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(7));
    payload.extend(zig_zag_i32(9));
    let center = DecodedBiomeColumn::decode(0, 1, &payload)
        .unwrap()
        .storage(0)
        .unwrap();
    let mut halo = std::array::from_fn(|_| None);
    halo[biome_neighbour_index(0, 0).unwrap()] = Some(center);

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id * 10);

    assert_eq!(record.tint_index_at([-1, 4, 8]), Some(70));
    assert_eq!(record.tint_index_at([16, 4, 8]), Some(90));
}

#[test]
fn identical_neighbour_payloads_are_deduplicated_and_uniform_fast_path_is_recorded() {
    let storage = uniform_storage(42);
    let halo = std::array::from_fn(|_| Some(Arc::clone(&storage)));

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + 1_000);

    // Eleven descriptor words plus one two-word uniform packed payload.
    assert_eq!(record.words().len(), 13);
    assert_eq!(record.uniform_tint_index(), Some(1_042));
    assert_eq!(record.byte_len(), 52);

    let radius_16_overworld_subchunks = 33_u64 * 33 * 24;
    assert_eq!(record.byte_len() * radius_16_overworld_subchunks, 1_359_072);
    assert_eq!(MAX_PACKED_BIOME_RECORD_WORDS, 55_316);
}
