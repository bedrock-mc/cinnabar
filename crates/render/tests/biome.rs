use render::PackedBiomeRecord;
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

    assert_eq!(record.words(), &[1 << 8, 1042]);
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
    assert_eq!(&record.words()[1..129], packed.as_slice());
    assert_eq!(&record.words()[129..], &[70, 90]);
    assert_eq!(record.tint_index(1, 2, 3), Some(90));
    assert_eq!(record.tint_index(1, 3, 2), Some(70));
}

#[test]
fn fallback_record_is_a_valid_uniform_palette() {
    let record = PackedBiomeRecord::fallback();

    assert_eq!(record.words(), &[1 << 8, 0]);
    assert_eq!(record.tint_index(0, 0, 0), Some(0));
    assert_eq!(record.tint_index(16, 0, 0), None);
}
