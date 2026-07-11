use std::fs;

use assets::{
    AssetError, BLOB_MAGIC, BLOB_VERSION, BlockFlags, BlockVisual, CompiledAssets,
    MATERIAL_FLAGS_MASK, MAX_MATERIALS, MAX_TEXTURE_LAYERS, MIP_COUNT, Material, TILE_SIZE,
    TextureArray, TextureMip, encode_blob, write_blob_atomic,
};
use sha2::{Digest, Sha256};

const HEADER_BYTES: usize = 88;

fn texture_array(layers: u32) -> TextureArray {
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .map(|size| TextureMip {
            size,
            rgba8: vec![0x55; size as usize * size as usize * 4 * layers as usize]
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    TextureArray { layers, mips }
}

fn valid_assets() -> CompiledAssets {
    CompiledAssets {
        visuals: vec![BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        }]
        .into_boxed_slice(),
        hashed: vec![(0x8000_0000, 0)].into_boxed_slice(),
        materials: vec![Material { layer: 0, flags: 0 }].into_boxed_slice(),
        textures: texture_array(1),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("u32 bytes"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("u64 bytes"))
}

#[test]
fn blob_has_checked_little_endian_sections_and_trailing_sha256() {
    let bytes = encode_blob(&valid_assets()).expect("encode valid assets");

    assert_eq!(&bytes[..8], &BLOB_MAGIC);
    assert_eq!(read_u32(&bytes, 8), BLOB_VERSION);
    assert_eq!(read_u32(&bytes, 12), TILE_SIZE);
    assert_eq!(read_u32(&bytes, 16), MIP_COUNT);
    assert_eq!(read_u32(&bytes, 20), 1, "visual count");
    assert_eq!(read_u32(&bytes, 24), 1, "hash count");
    assert_eq!(read_u32(&bytes, 28), 1, "material count");
    assert_eq!(read_u32(&bytes, 32), 1, "layer count");
    assert_eq!(read_u32(&bytes, 36), 0, "reserved header word");

    let visuals_offset = read_u64(&bytes, 40) as usize;
    let hashes_offset = read_u64(&bytes, 48) as usize;
    let materials_offset = read_u64(&bytes, 56) as usize;
    let textures_offset = read_u64(&bytes, 64) as usize;
    let textures_length = read_u64(&bytes, 72) as usize;
    let payload_length = read_u64(&bytes, 80) as usize;
    assert_eq!(visuals_offset, HEADER_BYTES);
    assert_eq!(hashes_offset, visuals_offset + 28);
    assert_eq!(materials_offset, hashes_offset + 8);
    assert_eq!(textures_offset, materials_offset + 8);
    assert_eq!(textures_length, 1_364);
    assert_eq!(payload_length, textures_offset + textures_length);
    assert_eq!(bytes.len(), payload_length + 32);

    let expected_hash = Sha256::digest(&bytes[..payload_length]);
    assert_eq!(&bytes[payload_length..], expected_hash.as_slice());
}

#[test]
fn blob_rejects_material_layer_visual_and_mip_invariants() {
    let mut bad_material_layer = valid_assets();
    bad_material_layer.materials[0].layer = 1;
    assert!(matches!(
        encode_blob(&bad_material_layer),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut bad_visual_material = valid_assets();
    bad_visual_material.visuals[0].faces[0] = 1;
    assert!(matches!(
        encode_blob(&bad_visual_material),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut bad_hash_visual = valid_assets();
    bad_hash_visual.hashed[0].1 = 1;
    assert!(matches!(
        encode_blob(&bad_hash_visual),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut bad_mip_length = valid_assets();
    bad_mip_length.textures.mips[1].rgba8 = vec![0; 7].into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_mip_length),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut bad_mip_count = valid_assets();
    bad_mip_count.textures.mips = Vec::new().into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_mip_count),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    for invalid in [
        BlockFlags::from_bits_retain(0x10),
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        let mut bad_flags = valid_assets();
        bad_flags.visuals[0].flags = invalid;
        assert!(matches!(
            encode_blob(&bad_flags),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }

    let mut bad_material_flags = valid_assets();
    bad_material_flags.materials = vec![
        Material { layer: 0, flags: 0 },
        Material {
            layer: 0,
            flags: MATERIAL_FLAGS_MASK | 0x80,
        },
    ]
    .into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_material_flags),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));
}

#[test]
fn blob_rejects_non_monotonic_hashes_and_allocation_counts() {
    let mut hashes = valid_assets();
    hashes.hashed = vec![(9, 0), (8, 0)].into_boxed_slice();
    assert!(matches!(
        encode_blob(&hashes),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut materials = valid_assets();
    materials.materials =
        vec![Material { layer: 0, flags: 0 }; MAX_MATERIALS + 1].into_boxed_slice();
    assert!(matches!(
        encode_blob(&materials),
        Err(AssetError::TooManyMaterials {
            count,
            max: MAX_MATERIALS
        }) if count == MAX_MATERIALS + 1
    ));

    let mut layers = valid_assets();
    layers.textures = TextureArray {
        layers: (MAX_TEXTURE_LAYERS + 1) as u32,
        mips: Vec::new().into_boxed_slice(),
    };
    assert!(matches!(
        encode_blob(&layers),
        Err(AssetError::TooManyTextureLayers {
            count,
            max: MAX_TEXTURE_LAYERS,
            ..
        }) if count == MAX_TEXTURE_LAYERS + 1
    ));
}

#[test]
fn blob_output_is_written_by_same_directory_atomic_rename() {
    let directory = tempfile::tempdir().expect("create output fixture");
    let output = directory.path().join("compiled/vanilla-v1001.mcbea");
    let bytes = encode_blob(&valid_assets()).expect("encode assets");

    write_blob_atomic(&output, &bytes).expect("write atomically");

    assert_eq!(fs::read(&output).expect("read output"), &*bytes);
    let siblings = fs::read_dir(output.parent().expect("output parent"))
        .expect("read output directory")
        .map(|entry| entry.expect("directory entry").path())
        .collect::<Vec<_>>();
    assert_eq!(siblings, [output]);
}
