use std::sync::Arc;

use world::{BiomeIds, DecodedBiomeColumn, RawBiomeIds};

const DEFAULT: u32 = 1;
const IDS: RawBiomeIds = RawBiomeIds {
    default_biome: DEFAULT,
};

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

fn packed(bits: u8, words: &[u32], palette_count: i32, palette: &[i32]) -> Vec<u8> {
    let mut bytes = vec![(bits << 1) | 1];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(zig_zag_i32(palette_count));
    for &id in palette {
        bytes.extend(zig_zag_i32(id));
    }
    bytes
}

fn biome_at(column: &DecodedBiomeColumn, y: i32, x: u8, local_y: u8, z: u8) -> Option<u32> {
    column.storage(y)?.biome_id(x, local_y, z)
}

#[test]
fn uniform_biome_storage_stays_palette_native() {
    let decoded = DecodedBiomeColumn::decode(-4, 1, &uniform(42), &IDS);
    let storage = decoded.storage(-4).unwrap();

    assert_eq!(decoded.bytes_consumed(), 2);
    assert_eq!(storage.bits_per_index(), 0);
    assert!(storage.packed_words().is_empty());
    assert_eq!(storage.palette().values(), &[42]);
    assert_eq!(storage.biome_id(15, 15, 15), Some(42));
}

#[test]
fn padded_width_biome_storage_uses_xzy_order() {
    let mut words = vec![0_u32; 410];
    let linear = (1_usize << 8) | (3_usize << 4) | 2;
    let values_per_word = 32 / 3;
    words[linear / values_per_word] |= 1 << ((linear % values_per_word) * 3);
    let bytes = packed(3, &words, 2, &[11, 22]);

    let decoded = DecodedBiomeColumn::decode(4, 1, &bytes, &IDS);
    let storage = decoded.storage(4).unwrap();
    assert_eq!(storage.biome_id(1, 2, 3), Some(22));
    assert_eq!(storage.biome_id(1, 3, 2), Some(11));
}

#[test]
fn out_of_range_palette_indices_resolve_to_entry_zero() {
    // Live crash: two bits per index, two palette entries, indices up to 3.
    let words = vec![0xe4e4_e4e4_u32; 256];
    let bytes = packed(2, &words, 2, &[7, 9]);
    let column = DecodedBiomeColumn::decode(0, 1, &bytes, &IDS);
    assert_eq!(column.bytes_consumed(), bytes.len());
    let storage = column.storage(0).unwrap();
    assert_eq!(storage.palette().values(), &[7, 9]);
    assert_eq!(
        (0..4)
            .map(|y| storage.biome_id(0, y, 0))
            .collect::<Vec<_>>(),
        [Some(7), Some(9), Some(7), Some(7)]
    );

    let mut words = vec![0_u32; 128];
    words[0] = 1;
    let column = DecodedBiomeColumn::decode(0, 1, &packed(1, &words, 1, &[9]), &IDS);
    assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(9));
}

#[test]
fn skip_header_first_slot_takes_the_default_biome() {
    for header in [0xfe, 0xff] {
        let mut bytes = vec![header];
        bytes.extend(uniform(7));
        let column = DecodedBiomeColumn::decode(0, 2, &bytes, &IDS);
        assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(DEFAULT));
        assert_eq!(biome_at(&column, 1, 0, 0, 0), Some(7));
        assert_eq!(column.bytes_consumed(), bytes.len());
    }
}

#[test]
fn empty_slots_take_the_default_below_and_the_top_layer_above() {
    // Column 0 changes biome at y=15 so extrusion must repeat only that layer.
    let mut words = vec![0_u32; 128];
    words[0] = 1 << 15;
    let mut bytes = uniform(3);
    bytes.push(0xff);
    bytes.extend(packed(1, &words, 2, &[4, 5]));
    bytes.push(0xff);
    let column = DecodedBiomeColumn::decode(0, 5, &bytes, &IDS);

    assert_eq!(column.len(), 5);
    assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(3));
    assert_eq!(biome_at(&column, 1, 0, 0, 0), Some(DEFAULT));
    assert_eq!(biome_at(&column, 2, 0, 14, 0), Some(4));
    assert_eq!(biome_at(&column, 2, 0, 15, 0), Some(5));
    for y in [3, 4] {
        assert_eq!(biome_at(&column, y, 0, 0, 0), Some(5));
        assert_eq!(biome_at(&column, y, 0, 15, 0), Some(5));
        assert_eq!(biome_at(&column, y, 1, 0, 0), Some(4));
    }
    assert!(Arc::ptr_eq(
        &column.storage(3).unwrap(),
        &column.storage(4).unwrap()
    ));
}

#[test]
fn eof_slots_extrude_the_last_storage_and_no_storage_means_default() {
    let column = DecodedBiomeColumn::decode(0, 3, &uniform(7), &IDS);
    assert_eq!(column.len(), 3);
    assert_eq!(biome_at(&column, 2, 5, 5, 5), Some(7));

    let column = DecodedBiomeColumn::decode(-4, 2, &[], &IDS);
    assert_eq!(column.len(), 2);
    assert_eq!(biome_at(&column, -4, 0, 0, 0), Some(DEFAULT));
    assert_eq!(biome_at(&column, -3, 0, 0, 0), Some(DEFAULT));
    assert_eq!(column.bytes_consumed(), 0);
}

#[test]
fn invalid_width_empties_its_slot_and_drains_the_payload() {
    let mut bytes = uniform(7);
    bytes.push((7 << 1) | 1);
    bytes.extend(uniform(8));
    let column = DecodedBiomeColumn::decode(0, 3, &bytes, &IDS);
    assert_eq!(column.bytes_consumed(), bytes.len());
    for y in 0..3 {
        assert_eq!(biome_at(&column, y, 0, 0, 0), Some(7));
    }
}

/// Resolves only the listed ids, like a registry that lacks custom biomes.
struct KnownBiomes(&'static [u16]);

impl BiomeIds for KnownBiomes {
    fn default_biome(&self) -> u32 {
        DEFAULT
    }

    fn resolve(&self, biome_id: u16) -> u32 {
        if self.0.contains(&biome_id) {
            u32::from(biome_id)
        } else {
            DEFAULT
        }
    }
}

#[test]
fn biome_ids_truncate_to_u16_and_unknown_ids_take_the_default() {
    let column = DecodedBiomeColumn::decode(0, 1, &uniform(0x1_0005), &KnownBiomes(&[5]));
    assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(5));
    let column = DecodedBiomeColumn::decode(0, 1, &uniform(99), &KnownBiomes(&[5]));
    assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(DEFAULT));
}

#[test]
fn truncated_and_overlong_biome_varints_read_as_zero() {
    for bytes in [&[0x01][..], &[0x01, 0x80, 0x80, 0x80, 0x80, 0x80]] {
        let column = DecodedBiomeColumn::decode(0, 1, bytes, &IDS);
        assert_eq!(biome_at(&column, 0, 0, 0, 0), Some(0));
        assert_eq!(column.bytes_consumed(), bytes.len());
    }
}

#[test]
fn biome_neighbour_arrival_invalidates_vertical_and_diagonal_consumers() {
    let source = world::SubChunkKey::new(0, 0, 4, 0);
    let dependents = source.biome_mesh_dependents().collect::<Vec<_>>();
    assert_eq!(dependents.len(), 27);
    assert!(dependents.contains(&world::SubChunkKey::new(0, -1, 3, 1)));
    assert!(dependents.contains(&world::SubChunkKey::new(0, 1, 5, -1)));
}
