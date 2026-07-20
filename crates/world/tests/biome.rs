use std::sync::Arc;

use world::{DecodeError, DecodedBiomeColumn};

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

fn uniform(id: i32) -> Vec<u8> {
    let mut bytes = vec![0x01];
    bytes.extend(zig_zag_i32(id));
    bytes
}

#[test]
fn uniform_biome_storage_stays_palette_native() {
    let decoded = DecodedBiomeColumn::decode(-4, 1, &uniform(42)).unwrap();
    let storage = decoded.storage(-4).unwrap();

    assert_eq!(decoded.bytes_consumed(), 2);
    assert_eq!(storage.bits_per_index(), 0);
    assert!(storage.packed_words().is_empty());
    assert_eq!(storage.palette().values(), &[42]);
    assert_eq!(storage.biome_id(15, 15, 15), Some(42));
}

#[test]
fn rejects_biome_storage_count_beyond_the_client_limit() {
    // A storage count larger than the vertical sub-chunk ceiling must be
    // rejected up front so the decoder never pre-reserves capacity proportional
    // to a wire-supplied count. The empty payload proves the bound is enforced
    // before any storage byte is read.
    assert_eq!(
        DecodedBiomeColumn::decode(0, world::MAX_LEVEL_SUBCHUNKS + 1, &[]),
        Err(DecodeError::TooManyBiomeStorages {
            count: world::MAX_LEVEL_SUBCHUNKS + 1,
            max: world::MAX_LEVEL_SUBCHUNKS,
        })
    );
}

#[test]
fn copy_previous_reuses_arc_and_first_copy_is_rejected() {
    let mut bytes = uniform(7);
    bytes.extend_from_slice(&[0xff, 0xff]);
    let decoded = DecodedBiomeColumn::decode(0, 3, &bytes).unwrap();
    let first = decoded.storage(0).unwrap();
    assert!(Arc::ptr_eq(&first, &decoded.storage(1).unwrap()));
    assert!(Arc::ptr_eq(&first, &decoded.storage(2).unwrap()));

    assert_eq!(
        DecodedBiomeColumn::decode(0, 1, &[0xff]),
        Err(DecodeError::BiomeCopyWithoutPrevious { index: 0 })
    );
}

#[test]
fn padded_width_biome_storage_uses_xzy_order() {
    let mut bytes = vec![0x07]; // Network palette, three bits per index.
    let mut words = vec![0_u32; 410];
    let linear = (1_usize << 8) | (3_usize << 4) | 2;
    let values_per_word = 32 / 3;
    words[linear / values_per_word] |= 1 << ((linear % values_per_word) * 3);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(zig_zag_i32(2));
    bytes.extend(zig_zag_i32(11));
    bytes.extend(zig_zag_i32(22));

    let decoded = DecodedBiomeColumn::decode(4, 1, &bytes).unwrap();
    let storage = decoded.storage(4).unwrap();
    assert_eq!(storage.biome_id(1, 2, 3), Some(22));
    assert_eq!(storage.biome_id(1, 3, 2), Some(11));
}

#[test]
fn rejects_disk_header_unsupported_width_and_bad_palette_indices() {
    assert!(matches!(
        DecodedBiomeColumn::decode(0, 1, &[0xfe]),
        Err(DecodeError::DiskPaletteInNetworkData { header: 0xfe })
    ));
    assert_eq!(
        DecodedBiomeColumn::decode(0, 1, &[0x0f]),
        Err(DecodeError::UnsupportedBitsPerIndex(7))
    );

    let mut bad_index = vec![0x03];
    bad_index.extend_from_slice(&1_u32.to_le_bytes());
    bad_index.extend(std::iter::repeat_n(0_u8, 127 * 4));
    bad_index.extend(zig_zag_i32(1));
    bad_index.extend(zig_zag_i32(9));
    assert!(matches!(
        DecodedBiomeColumn::decode(0, 1, &bad_index),
        Err(DecodeError::PaletteIndexOutOfBounds {
            block_index: 0,
            palette_index: 1,
            palette_len: 1,
        })
    ));
}

#[test]
fn truncated_and_overlong_biome_varints_are_bounded() {
    assert!(matches!(
        DecodedBiomeColumn::decode(0, 1, &[0x01]),
        Err(DecodeError::UnexpectedEof { .. })
    ));
    assert_eq!(
        DecodedBiomeColumn::decode(0, 1, &[0x01, 0x80, 0x80, 0x80, 0x80, 0x80]),
        Err(DecodeError::VarIntTooLong {
            context: "palette entry"
        })
    );
}
