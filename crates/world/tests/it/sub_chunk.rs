use std::{fs, path::PathBuf};

use serde::Deserialize;
use world::{BlockIds, MAX_STORAGE_COUNT, RawBlockIds, SubChunk};

const AIR: u32 = 134;
const IDS: RawBlockIds = RawBlockIds { air: AIR };

#[derive(Debug, Deserialize)]
struct Manifest {
    source: Source,
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Source {
    module: String,
    version: String,
    commit: String,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    file: String,
    version: u8,
    y_index: i8,
    storages: Vec<ExpectedStorage>,
    samples: Vec<Sample>,
}

#[derive(Debug, Deserialize)]
struct ExpectedStorage {
    bits_per_index: u8,
    palette_len: usize,
}

#[derive(Debug, Deserialize)]
struct Sample {
    name: String,
    layer: usize,
    x: u8,
    y: u8,
    z: u8,
    runtime_id: u32,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn load_manifest() -> Manifest {
    let path = fixtures_dir().join("manifest.json");
    serde_json::from_slice(&fs::read(path).expect("read fixture manifest"))
        .expect("decode fixture manifest")
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

fn uniform(version: u8, y_index: Option<i8>, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![version];
    if version >= 8 {
        bytes.push(1);
    }
    if version >= 9 {
        bytes.push(y_index.expect("version 9 requires an index") as u8);
    }
    bytes.push(1); // Zero bits per index + network palette flag.
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

fn one_bit_storage(word: u32, palette_count: i32, palette: &[u32]) -> Vec<u8> {
    let mut bytes = vec![3]; // One bit per index + network palette flag.
    for _ in 0..128 {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(zig_zag_i32(palette_count));
    for &runtime_id in palette {
        bytes.extend(zig_zag_i32(runtime_id as i32));
    }
    bytes
}

#[test]
fn decodes_every_dragonfly_golden_without_flattening() {
    let manifest = load_manifest();
    assert_eq!(manifest.source.module, "github.com/df-mc/dragonfly");
    assert_eq!(
        manifest.source.version,
        "v0.10.15-0.20260709170650-b85c56ffea6b"
    );
    assert_eq!(
        manifest.source.commit,
        "b85c56ffea6b306798a935f14cc941c76618be52"
    );

    for fixture in manifest.fixtures {
        let bytes = fs::read(fixtures_dir().join(&fixture.file)).expect("read golden fixture");
        let sub_chunk = SubChunk::decode(&bytes, &IDS);

        assert_eq!(sub_chunk.version(), fixture.version, "{}", fixture.file);
        assert_eq!(
            sub_chunk.y_index(),
            Some(fixture.y_index),
            "{}",
            fixture.file
        );
        assert_eq!(
            sub_chunk.storages().len(),
            fixture.storages.len(),
            "{}",
            fixture.file
        );

        for (storage, expected) in sub_chunk.storages().iter().zip(&fixture.storages) {
            assert_eq!(
                storage.bits_per_index(),
                expected.bits_per_index,
                "{}",
                fixture.file
            );
            assert_eq!(
                storage.palette().len(),
                expected.palette_len,
                "{}",
                fixture.file
            );
            let expected_words = if expected.bits_per_index == 0 {
                0
            } else {
                let values_per_word = 32 / usize::from(expected.bits_per_index);
                4096_usize.div_ceil(values_per_word)
            };
            assert_eq!(storage.words().len(), expected_words, "{}", fixture.file);
            assert!(
                storage.words().len() < 4096,
                "{} expanded its indices into a flat per-block array",
                fixture.file
            );
            if storage.is_uniform() {
                assert!(
                    storage.words().is_empty(),
                    "uniform storage must not retain index words"
                );
            }
        }

        for sample in fixture.samples {
            assert_eq!(
                sub_chunk.runtime_id(sample.layer, sample.x, sample.y, sample.z),
                Some(sample.runtime_id),
                "{} sample {}",
                fixture.file,
                sample.name
            );
        }
    }
}

#[test]
fn decodes_version_one_and_eight_compatibility_layouts() {
    for (version, bytes) in [(1, uniform(1, None, 99)), (8, uniform(8, None, 99))] {
        let decoded = SubChunk::decode(&bytes, &IDS);
        assert_eq!(decoded.version(), version);
        assert_eq!(decoded.y_index(), None);
        assert_eq!(decoded.storages().len(), 1);
        assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(99));
        assert_eq!(decoded.runtime_id(0, 15, 15, 15), Some(99));
    }
}

#[test]
fn zero_storage_version_nine_sub_chunk_is_all_air_without_allocation() {
    let decoded = SubChunk::decode(&[9, 0, (-4_i8) as u8], &IDS);
    assert_eq!(decoded.y_index(), Some(-4));
    assert!(decoded.has_no_storages());
    assert!(decoded.storages().is_empty());
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), None);
}

#[test]
fn preserves_high_bit_block_network_hashes() {
    for network_value in [0xdead_beef_u32, u32::MAX] {
        let decoded = SubChunk::decode(&uniform(9, Some(-4), network_value), &IDS);
        assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(network_value));
        assert_eq!(decoded.storages()[0].palette().values(), &[network_value]);
    }
}

#[test]
fn runtime_lookup_checks_layer_and_coordinate_bounds() {
    let decoded = SubChunk::decode(&uniform(9, Some(-4), 7), &IDS);
    assert_eq!(decoded.runtime_id(1, 0, 0, 0), None);
    assert_eq!(decoded.runtime_id(0, 16, 0, 0), None);
    assert_eq!(decoded.runtime_id(0, 0, 16, 0), None);
    assert_eq!(decoded.runtime_id(0, 0, 0, 16), None);
}

/// Resolves only the listed ids, like a registry that lacks custom blocks.
struct KnownIds(&'static [u32]);

impl BlockIds for KnownIds {
    fn air(&self) -> u32 {
        AIR
    }

    fn resolve(&self, network_id: u32) -> u32 {
        if self.0.contains(&network_id) {
            network_id
        } else {
            AIR
        }
    }
}

#[test]
fn unknown_block_ids_resolve_to_air() {
    let decoded = SubChunk::decode(&uniform(9, Some(0), 77), &KnownIds(&[5]));
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(AIR));

    let mut bytes = vec![9, 1, 0];
    bytes.extend(one_bit_storage(0xaaaa_aaaa, 2, &[5, 77]));
    let decoded = SubChunk::decode(&bytes, &KnownIds(&[5]));
    assert_eq!(decoded.storages()[0].palette().values(), &[5, AIR]);
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(5));
    assert_eq!(decoded.runtime_id(0, 0, 1, 0), Some(AIR));
}

#[test]
fn legacy_versions_consume_the_fixed_payload_and_read_as_empty() {
    for version in [0, 2, 7] {
        let mut bytes = vec![version];
        bytes.extend(std::iter::repeat_n(0x11, 6144));
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
        assert_eq!(decoded.version(), version);
        assert!(decoded.has_no_storages(), "version {version}");
        assert_eq!(consumed, 6145, "version {version}");
    }
}

#[test]
fn eof_header_fields_read_as_zero() {
    let (empty, consumed) = SubChunk::decode_prefix(&[], &IDS);
    assert!(empty.has_no_storages());
    assert_eq!(consumed, 0);

    let (no_count, consumed) = SubChunk::decode_prefix(&[8], &IDS);
    assert!(no_count.has_no_storages());
    assert_eq!(consumed, 1);

    // Missing Y reads 0; the missing storage header reads a persistent uniform entry.
    let (no_y, consumed) = SubChunk::decode_prefix(&[9, 1], &IDS);
    assert_eq!(no_y.y_index(), Some(0));
    assert_eq!(no_y.storages().len(), 1);
    assert_eq!(no_y.runtime_id(0, 0, 0, 0), Some(AIR));
    assert_eq!(consumed, 2);
}

#[test]
fn versions_past_nine_read_uniform_air_layers_and_nothing_else() {
    let bytes = [10, 3, 5, 0xde, 0xad];
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.y_index(), Some(5));
    assert_eq!(decoded.storages().len(), 2);
    assert!(
        decoded
            .storages()
            .iter()
            .all(|storage| storage.uniform_runtime_id() == Some(AIR))
    );
    assert_eq!(consumed, 3);
}

#[test]
fn storage_counts_past_two_leave_later_layers_unread() {
    for count in [3, MAX_STORAGE_COUNT + 1] {
        let mut bytes = vec![9, count as u8, 0];
        let mut expected = 3;
        for runtime_id in 0..count as u32 {
            let layer = uniform(1, None, 10 + runtime_id);
            if runtime_id < 2 {
                expected += layer.len() - 1;
            }
            bytes.extend_from_slice(&layer[1..]);
        }
        let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
        assert_eq!(decoded.storages().len(), 2);
        assert_eq!(decoded.runtime_id(1, 0, 0, 0), Some(11));
        assert_eq!(consumed, expected);
    }
}

#[test]
fn unsupported_width_reads_uniform_air_after_the_header_only() {
    let bytes = [9, 1, 0, (7 << 1) | 1, 0xaa, 0xbb];
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.storages().len(), 1);
    assert_eq!(decoded.storages()[0].uniform_runtime_id(), Some(AIR));
    assert_eq!(consumed, 4);
}

#[test]
fn persistent_palette_entries_resolve_names_and_states_without_shifting_the_stream() {
    struct NamedIds;
    impl BlockIds for NamedIds {
        fn air(&self) -> u32 {
            AIR
        }
        fn resolve(&self, id: u32) -> u32 {
            id
        }
        fn resolve_persistent(&self, entry: &world::NbtCompound) -> u32 {
            assert_eq!(entry.string("name"), Some("stone"));
            assert_eq!(
                entry.compound("states").unwrap().integer("test_state"),
                Some(3)
            );
            42
        }
    }
    let mut states = world::NbtCompound::default();
    states.insert("test_state", world::NbtValue::Int(3));
    let mut entry = world::NbtCompound::default();
    entry.insert("name", world::NbtValue::String("stone".into()));
    entry.insert("states", world::NbtValue::Compound(states));
    let mut bytes = vec![8, 1, 0];
    bytes.extend(entry.encode_root().unwrap());
    let end = bytes.len();
    bytes.extend(uniform(8, None, 7));
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &NamedIds);
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(42));
    assert_eq!(consumed, end);
    assert_eq!(
        SubChunk::decode(&bytes[consumed..], &NamedIds).runtime_id(0, 0, 0, 0),
        Some(7)
    );
}

#[test]
fn persistent_palette_entries_skip_their_nbt_and_read_as_air() {
    let stone = [&[10, 0, 8, 4][..], b"name", &[15], b"minecraft:stone", &[0]].concat();
    let mut bytes = vec![9, 1, 0, 0];
    bytes.extend_from_slice(&stone);
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.storages()[0].uniform_runtime_id(), Some(AIR));
    assert_eq!(consumed, bytes.len());

    // A non-compound root consumes one byte per entry.
    let mut bytes = vec![9, 1, 0, 1 << 1];
    bytes.extend(std::iter::repeat_n(0, 128 * 4));
    bytes.extend(zig_zag_i32(2));
    bytes.extend_from_slice(&stone);
    bytes.push(3);
    bytes.push(0xaa);
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.storages()[0].palette().values(), &[AIR, AIR]);
    assert_eq!(consumed, bytes.len() - 1);
}

#[test]
fn truncated_words_read_as_zero_and_park_at_the_end() {
    let bytes = [9, 1, 0, 3, 0xff, 0xff, 0xff, 0xff, 0xff];
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    let storage = &decoded.storages()[0];
    assert!(storage.words().iter().all(|&word| word == 0));
    assert_eq!(storage.palette().values(), &[0]);
    assert_eq!(consumed, bytes.len());
}

#[test]
fn missing_palette_entries_read_as_zero() {
    let mut bytes = vec![9, 1, 0];
    bytes.extend(one_bit_storage(0, 2, &[5]));
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.storages()[0].palette().values(), &[5, 0]);
    assert_eq!(consumed, bytes.len());
}

#[test]
fn malformed_varints_read_as_zero_or_drop_high_bits() {
    // Five continuation bytes read as zero and leave the next byte unread.
    let unterminated = [9, 1, 0, 1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
    let (decoded, consumed) = SubChunk::decode_prefix(&unterminated, &IDS);
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(0));
    assert_eq!(consumed, 9);

    let overflowing = [9, 1, 0, 1, 0x84, 0x80, 0x80, 0x80, 0x10];
    assert_eq!(
        SubChunk::decode(&overflowing, &IDS).runtime_id(0, 0, 0, 0),
        Some(2)
    );

    let (decoded, consumed) = SubChunk::decode_prefix(&[9, 1, 0, 1, 0x80], &IDS);
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(0));
    assert_eq!(consumed, 5);
}

#[test]
fn palette_lengths_clamp_to_storage_capacity() {
    for (count, palette, expected, unread) in [
        (0, &[5_u32][..], &[5_u32][..], 0),
        (-1, &[5], &[5], 0),
        (3, &[5, 6, 7], &[5, 6], 1),
    ] {
        let mut bytes = vec![9, 1, 0];
        bytes.extend(one_bit_storage(0, count, palette));
        let (sub_chunk, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
        assert_eq!(sub_chunk.storages()[0].palette().values(), expected);
        assert_eq!(bytes.len() - consumed, unread);
    }
}

#[test]
fn palette_indices_past_the_palette_resolve_to_entry_zero() {
    let mut bytes = vec![9, 1, 0];
    bytes.extend(one_bit_storage(u32::MAX, 1, &[42]));
    let sub_chunk = SubChunk::decode(&bytes, &IDS);
    assert_eq!(sub_chunk.runtime_id(0, 0, 0, 0), Some(42));
    assert_eq!(sub_chunk.runtime_id(0, 15, 15, 15), Some(42));
}

#[test]
fn trailing_bytes_are_left_unread() {
    let mut bytes = uniform(9, Some(-4), 1);
    let expected = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let (decoded, consumed) = SubChunk::decode_prefix(&bytes, &IDS);
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(1));
    assert_eq!(consumed, expected);
    assert_eq!(SubChunk::decode(&bytes, &IDS), decoded);
}

#[test]
fn arbitrary_short_inputs_never_panic() {
    let mut state = 0x9e37_79b9_u32;
    for len in 0..512 {
        let mut input = vec![0; len];
        for byte in &mut input {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        for version in [8, 9] {
            if let Some(first) = input.first_mut() {
                *first = version;
            }
            let result = std::panic::catch_unwind(|| SubChunk::decode_prefix(&input, &IDS));
            let (_, consumed) = result.unwrap_or_else(|_| panic!("panicked at length {len}"));
            assert!(consumed <= len);
        }
    }
}
