use std::{fs, path::PathBuf};

use serde::Deserialize;
use world::{DecodeError, MAX_STORAGE_COUNT, SubChunk};

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
    if version == 9 {
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
        let sub_chunk = SubChunk::decode(&bytes)
            .unwrap_or_else(|error| panic!("{} failed to decode: {error}", fixture.file));

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
        let decoded = SubChunk::decode(&bytes).expect("decode compatibility sub-chunk");
        assert_eq!(decoded.version(), version);
        assert_eq!(decoded.y_index(), None);
        assert_eq!(decoded.storages().len(), 1);
        assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(99));
        assert_eq!(decoded.runtime_id(0, 15, 15, 15), Some(99));
    }
}

#[test]
fn zero_storage_version_nine_sub_chunk_is_all_air_without_allocation() {
    let decoded = SubChunk::decode(&[9, 0, (-4_i8) as u8]).expect("decode all-air sub-chunk");
    assert_eq!(decoded.y_index(), Some(-4));
    assert!(decoded.has_no_storages());
    assert!(decoded.storages().is_empty());
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), None);
}

#[test]
fn preserves_high_bit_block_network_hashes() {
    for network_value in [0xdead_beef_u32, u32::MAX] {
        let decoded = SubChunk::decode(&uniform(9, Some(-4), network_value))
            .expect("decode high-bit network value");
        assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(network_value));
        assert_eq!(decoded.storages()[0].palette().values(), &[network_value]);
    }
}

#[test]
fn runtime_lookup_checks_layer_and_coordinate_bounds() {
    let decoded = SubChunk::decode(&uniform(9, Some(-4), 7)).expect("decode uniform");
    assert_eq!(decoded.runtime_id(1, 0, 0, 0), None);
    assert_eq!(decoded.runtime_id(0, 16, 0, 0), None);
    assert_eq!(decoded.runtime_id(0, 0, 16, 0), None);
    assert_eq!(decoded.runtime_id(0, 0, 0, 16), None);
}

#[test]
fn rejects_unknown_or_truncated_sub_chunk_headers() {
    assert_eq!(
        SubChunk::decode(&[]),
        Err(DecodeError::UnexpectedEof {
            context: "sub-chunk version",
            needed: 1,
            remaining: 0,
        })
    );
    assert_eq!(
        SubChunk::decode(&[7]),
        Err(DecodeError::UnsupportedVersion(7))
    );
    assert!(matches!(
        SubChunk::decode(&[8]),
        Err(DecodeError::UnexpectedEof {
            context: "storage count",
            ..
        })
    ));
    assert!(matches!(
        SubChunk::decode(&[9, 1]),
        Err(DecodeError::UnexpectedEof {
            context: "sub-chunk Y index",
            ..
        })
    ));
}

#[test]
fn rejects_storage_count_over_the_client_bound() {
    let bytes = [9, (MAX_STORAGE_COUNT + 1) as u8, 0];
    assert_eq!(
        SubChunk::decode(&bytes),
        Err(DecodeError::TooManyStorages {
            count: MAX_STORAGE_COUNT + 1,
            max: MAX_STORAGE_COUNT,
        })
    );
}

#[test]
fn rejects_disk_palettes_and_unsupported_bit_widths() {
    assert_eq!(
        SubChunk::decode(&[9, 1, 0, 0]),
        Err(DecodeError::DiskPaletteInNetworkData { header: 0 })
    );
    assert_eq!(
        SubChunk::decode(&[9, 1, 0, (7 << 1) | 1]),
        Err(DecodeError::UnsupportedBitsPerIndex(7))
    );
}

#[test]
fn rejects_truncated_words_and_palette_varints() {
    assert!(matches!(
        SubChunk::decode(&[9, 1, 0, 3]),
        Err(DecodeError::UnexpectedEof {
            context: "packed index words",
            ..
        })
    ));

    let mut bytes = vec![9, 1, 0];
    bytes.extend(one_bit_storage(0, 2, &[0]));
    assert!(matches!(
        SubChunk::decode(&bytes),
        Err(DecodeError::UnexpectedEof {
            context: "palette entry",
            ..
        })
    ));

    let unterminated = [9, 1, 0, 1, 0x80, 0x80, 0x80, 0x80, 0x80];
    assert!(matches!(
        SubChunk::decode(&unterminated),
        Err(DecodeError::VarIntTooLong {
            context: "palette entry"
        })
    ));
}

#[test]
fn rejects_varints_that_overflow_u32() {
    let bytes = [
        9, 1, 0, 1, // v9, one storage, Y=0, uniform network palette.
        0x80, 0x80, 0x80, 0x80, 0x10,
    ];
    assert_eq!(
        SubChunk::decode(&bytes),
        Err(DecodeError::VarIntOverflow {
            context: "palette entry",
        })
    );
}

#[test]
fn rejects_invalid_palette_lengths_before_allocating() {
    for (count, expected) in [
        (0, DecodeError::InvalidPaletteLength { count: 0, max: 2 }),
        (-1, DecodeError::InvalidPaletteLength { count: -1, max: 2 }),
        (3, DecodeError::InvalidPaletteLength { count: 3, max: 2 }),
    ] {
        let mut bytes = vec![9, 1, 0];
        bytes.extend(one_bit_storage(0, count, &[]));
        assert_eq!(SubChunk::decode(&bytes), Err(expected));
    }
}

#[test]
fn rejects_palette_indices_that_do_not_resolve() {
    let mut bytes = vec![9, 1, 0];
    bytes.extend(one_bit_storage(u32::MAX, 1, &[42]));
    assert_eq!(
        SubChunk::decode(&bytes),
        Err(DecodeError::PaletteIndexOutOfBounds {
            block_index: 0,
            palette_index: 1,
            palette_len: 1,
        })
    );
}

#[test]
fn exact_decoder_rejects_trailing_bytes() {
    let mut bytes = uniform(9, Some(-4), 1);
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    assert_eq!(
        SubChunk::decode(&bytes),
        Err(DecodeError::TrailingBytes { remaining: 2 })
    );
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
        let result = std::panic::catch_unwind(|| SubChunk::decode(&input));
        assert!(result.is_ok(), "decoder panicked for input length {len}");
    }
}
