use std::mem::size_of_val;

use assets::{
    BLOB_VERSION, BlockFace, BlockFlags, BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL,
    MAX_MATERIALS, MAX_TEXTURE_LAYERS, Material, NetworkIdMode, RuntimeAssets, TextureArray,
    TextureMip, encode_blob,
};
use sha2::{Digest, Sha256};

const HEADER_BYTES: usize = 88;
const HASH_BYTES: usize = 32;
const VERSION_OFFSET: usize = 8;
const VISUAL_COUNT_OFFSET: usize = 20;
const HASH_COUNT_OFFSET: usize = 24;
const MATERIAL_COUNT_OFFSET: usize = 28;
const LAYER_COUNT_OFFSET: usize = 32;
const VISUALS_OFFSET_OFFSET: usize = 40;
const HASHES_OFFSET_OFFSET: usize = 48;
const MATERIALS_OFFSET_OFFSET: usize = 56;
const TEXTURES_LENGTH_OFFSET: usize = 72;
const PAYLOAD_LENGTH_OFFSET: usize = 80;

fn texture_array(layers: u32) -> TextureArray {
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .enumerate()
        .map(|(level, size)| TextureMip {
            size,
            rgba8: vec![
                0x30 + u8::try_from(level).expect("small mip");
                size as usize * size as usize * 4 * layers as usize
            ]
            .into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    TextureArray { layers, mips }
}

fn compiled_assets() -> CompiledAssets {
    CompiledAssets {
        visuals: vec![
            BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
            },
            BlockVisual {
                faces: [1, 1, 1, 1, 1, 1],
                flags: BlockFlags::FULL_CUBE,
            },
        ]
        .into_boxed_slice(),
        // Hash 1 deliberately collides with sequential ID 1 but maps to visual 0.
        hashed: vec![(1, 0), (0xdbf4_4120, 1)].into_boxed_slice(),
        materials: vec![
            Material { layer: 0, flags: 0 },
            Material { layer: 1, flags: 7 },
        ]
        .into_boxed_slice(),
        textures: texture_array(2),
    }
}

fn valid_blob() -> Vec<u8> {
    encode_blob(&compiled_assets())
        .expect("encode synthetic runtime assets")
        .into_vec()
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("u64 bytes"))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn reseal(bytes: &mut [u8]) {
    let payload_length = read_u64(bytes, PAYLOAD_LENGTH_OFFSET) as usize;
    assert_eq!(bytes.len(), payload_length + HASH_BYTES);
    let digest = Sha256::digest(&bytes[..payload_length]);
    bytes[payload_length..].copy_from_slice(&digest);
}

fn assert_rejected(bytes: &[u8], case: &str) {
    assert!(RuntimeAssets::decode(bytes).is_err(), "accepted {case}");
}

#[test]
fn decode_rejects_bad_magic_version_hash_and_truncation() {
    let mut bad_magic = valid_blob();
    bad_magic[0] ^= 0xff;
    assert_rejected(&bad_magic, "bad magic");

    let mut bad_version = valid_blob();
    write_u32(&mut bad_version, VERSION_OFFSET, BLOB_VERSION + 1);
    reseal(&mut bad_version);
    assert_rejected(&bad_version, "unsupported version");

    let mut bad_hash = valid_blob();
    bad_hash[HEADER_BYTES] ^= 1;
    assert_rejected(&bad_hash, "bad trailing hash");

    let blob = valid_blob();
    let materials_offset = read_u64(&blob, MATERIALS_OFFSET_OFFSET) as usize;
    assert_rejected(&blob[..materials_offset + 1], "truncated material section");

    let mut overlapping_sections = valid_blob();
    write_u64(
        &mut overlapping_sections,
        HASHES_OFFSET_OFFSET,
        HEADER_BYTES as u64,
    );
    reseal(&mut overlapping_sections);
    assert_rejected(&overlapping_sections, "overlapping sections");
}

#[test]
fn decode_rejects_non_monotonic_or_out_of_range_references() {
    let mut non_monotonic = valid_blob();
    let hashes_offset = read_u64(&non_monotonic, HASHES_OFFSET_OFFSET) as usize;
    write_u32(&mut non_monotonic, hashes_offset, 9);
    write_u32(&mut non_monotonic, hashes_offset + 8, 8);
    reseal(&mut non_monotonic);
    assert_rejected(&non_monotonic, "non-monotonic hash keys");

    let mut bad_hash_visual = valid_blob();
    let hashes_offset = read_u64(&bad_hash_visual, HASHES_OFFSET_OFFSET) as usize;
    write_u32(&mut bad_hash_visual, hashes_offset + 4, 2);
    reseal(&mut bad_hash_visual);
    assert_rejected(&bad_hash_visual, "hash lookup visual out of range");

    let mut bad_visual_material = valid_blob();
    let visuals_offset = read_u64(&bad_visual_material, VISUALS_OFFSET_OFFSET) as usize;
    write_u32(&mut bad_visual_material, visuals_offset, 2);
    reseal(&mut bad_visual_material);
    assert_rejected(&bad_visual_material, "visual material out of range");

    let mut bad_material_layer = valid_blob();
    let materials_offset = read_u64(&bad_material_layer, MATERIALS_OFFSET_OFFSET) as usize;
    write_u32(&mut bad_material_layer, materials_offset + 8, 2);
    reseal(&mut bad_material_layer);
    assert_rejected(&bad_material_layer, "material layer out of range");
}

#[test]
fn decode_rejects_mip_length_mismatches_and_allocation_limits() {
    let mut wrong_texture_length = valid_blob();
    let texture_length = read_u64(&wrong_texture_length, TEXTURES_LENGTH_OFFSET);
    write_u64(
        &mut wrong_texture_length,
        TEXTURES_LENGTH_OFFSET,
        texture_length - 1,
    );
    reseal(&mut wrong_texture_length);
    assert_rejected(&wrong_texture_length, "mismatched mip byte length");

    for (offset, value, case) in [
        (VISUAL_COUNT_OFFSET, 65_537, "visual allocation limit"),
        (HASH_COUNT_OFFSET, 65_537, "hash allocation limit"),
        (
            MATERIAL_COUNT_OFFSET,
            u32::try_from(MAX_MATERIALS + 1).expect("material limit fits"),
            "material allocation limit",
        ),
        (
            LAYER_COUNT_OFFSET,
            u32::try_from(MAX_TEXTURE_LAYERS + 1).expect("layer limit fits"),
            "texture allocation limit",
        ),
    ] {
        let mut oversized = valid_blob();
        write_u32(&mut oversized, offset, value);
        reseal(&mut oversized);
        assert_rejected(&oversized, case);
    }
}

#[test]
fn explicit_network_id_mode_keeps_sequential_and_hash_lookups_isolated() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode valid blob");

    let sequential = runtime.resolve(NetworkIdMode::Sequential, 1);
    assert!(sequential.is_known());
    assert_eq!(sequential.flags(), BlockFlags::FULL_CUBE);
    assert_eq!(sequential.face(BlockFace::West).material_id(), 1);

    let hashed = runtime.resolve(NetworkIdMode::Hashed, 0xdbf4_4120);
    assert!(hashed.is_known());
    assert_eq!(hashed.face(BlockFace::South).material_id(), 1);

    let colliding_hash = runtime.resolve(NetworkIdMode::Hashed, 1);
    assert!(colliding_hash.is_known());
    assert_eq!(colliding_hash.face(BlockFace::West).material_id(), 0);
    assert_eq!(runtime.missing_count(), 0);
}

#[test]
fn missing_values_and_materials_use_one_bounded_diagnostic_counter() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode valid blob");
    let runtime_size = size_of_val(&runtime);

    for value in 0..10_000 {
        let missing = runtime.resolve(NetworkIdMode::Sequential, value + 100);
        assert!(!missing.is_known());
        assert_eq!(
            missing.face(BlockFace::Up).material_id(),
            DIAGNOSTIC_MATERIAL
        );
    }

    assert_eq!(runtime.missing_count(), 10_000);
    assert_eq!(size_of_val(&runtime), runtime_size);
    assert_eq!(runtime.material(u32::MAX), Material { layer: 0, flags: 0 });
    assert_eq!(runtime.missing_count(), 10_001);
    assert_eq!(runtime.material(1), Material { layer: 1, flags: 7 });
    assert_eq!(runtime.missing_count(), 10_001);
}

#[test]
fn decoded_texture_mips_are_exposed_without_lookup_mutation() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode valid blob");
    let textures = runtime.texture_array();

    assert_eq!(textures.layers, 2);
    assert_eq!(textures.mips.len(), 5);
    assert_eq!(textures.mips[0].rgba8.len(), 16 * 16 * 4 * 2);
    assert_eq!(textures.mips[4].rgba8.len(), 4 * 2);
    assert_eq!(runtime.missing_count(), 0);
}

#[test]
fn programmatic_diagnostic_runtime_is_minimal_and_self_contained() {
    let runtime = RuntimeAssets::diagnostic();

    let diagnostic = runtime.resolve(NetworkIdMode::Sequential, 0);
    assert!(diagnostic.is_known());
    assert_eq!(
        diagnostic.face(BlockFace::Up).material_id(),
        DIAGNOSTIC_MATERIAL
    );
    assert_eq!(runtime.material(0), Material { layer: 0, flags: 0 });
    assert_eq!(runtime.texture_array().layers, 1);
    assert_eq!(runtime.texture_array().mips.len(), 5);
    assert_eq!(runtime.texture_array().mips[0].rgba8.len(), 16 * 16 * 4);

    assert!(
        !runtime.resolve(NetworkIdMode::Hashed, 0).is_known(),
        "diagnostic fallback must not blur sequential and hashed namespaces"
    );
}
