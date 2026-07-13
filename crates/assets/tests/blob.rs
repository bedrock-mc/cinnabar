use std::fs;

use assets::{
    AssetError, BLOB_MAGIC, BLOB_VERSION, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, MATERIAL_FLAGS_MASK, MAX_MATERIALS, MAX_TEXTURE_LAYERS, MIP_COUNT,
    Material, NO_ANIMATION, NO_MODEL_TEMPLATE, TILE_SIZE, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_blob, write_blob_atomic,
};
use sha2::{Digest, Sha256};

#[test]
fn mcbeas04_exact_bytes() {
    assert_eq!(&BLOB_MAGIC, b"MCBEAS04");
    assert_eq!(BLOB_VERSION, 4);
    let texture = assets::TextureRef::new(1, 17).expect("bounded texture ref");
    assert_eq!(texture.raw(), 0x8000_0011);

    let mut fixture = valid_assets();
    fixture.visuals = vec![
        BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Diagnostic,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        },
        BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: 0,
            variant: 7,
        },
    ]
    .into_boxed_slice();
    fixture.hashed = vec![(1, 0), (2, 1)].into_boxed_slice();
    fixture.materials = vec![
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        },
        Material {
            texture: TextureRef::new(1, 0).unwrap(),
            flags: assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            animation: 0,
        },
    ]
    .into_boxed_slice();
    fixture.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    fixture.model_quads = vec![assets::ModelQuad {
        positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 1,
        flags: 1 | assets::MODEL_QUAD_FLAG_TWO_SIDED | (2 << 4),
    }]
    .into_boxed_slice();
    fixture.animations = vec![assets::Animation {
        frame_start: 0,
        frame_count: 2,
        ticks_per_frame: 3,
        atlas_index: 4,
        atlas_tile_variant: 5,
        replicate: 2,
        flags: assets::ANIMATION_FLAG_BLEND,
    }]
    .into_boxed_slice();
    fixture.animation_frames = vec![
        TextureRef::new(0, 0).unwrap(),
        TextureRef::new(1, 0).unwrap(),
    ]
    .into_boxed_slice();
    fixture.texture_pages = vec![
        TexturePage::new(texture_array(1)),
        TexturePage::new(texture_array(1)),
    ]
    .into_boxed_slice();

    let bytes = encode_blob(&fixture).expect("encode every MCBEAS04 table");
    assert_eq!(bytes.len(), 1_576_168);
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "4770d48d0925290720a8e53707f6a4cd2f6e8dd11ec89bbaa258feeae06944f5",
        "the complete every-table fixture is the byte-exact MCBEAS04 golden"
    );
    assert_eq!(read_u32(&bytes, 20), 2);
    assert_eq!(read_u32(&bytes, 28), 2);
    assert_eq!(read_u32(&bytes, 32), 1);
    assert_eq!(read_u32(&bytes, 36), 1);
    assert_eq!(read_u32(&bytes, 40), 1);
    assert_eq!(read_u32(&bytes, 44), 2);
    assert_eq!(read_u32(&bytes, 48), 2);
    let visuals = read_u64(&bytes, 96) as usize;
    let materials = read_u64(&bytes, 112) as usize;
    let templates = read_u64(&bytes, 120) as usize;
    let quads = read_u64(&bytes, 128) as usize;
    let animations = read_u64(&bytes, 136) as usize;
    let frames = read_u64(&bytes, 144) as usize;
    assert_eq!(
        &bytes[visuals + 40..visuals + 64],
        &[
            1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0
        ]
    );
    assert_eq!(read_u32(&bytes, materials + 12), 0x8000_0000);
    assert_eq!(read_u32(&bytes, templates + 4), 1);
    assert_eq!(read_u32(&bytes, quads + 40), 1);
    assert_eq!(read_u32(&bytes, animations + 4), 2);
    assert_eq!(read_u32(&bytes, animations + 12), 4);
    assert_eq!(read_u32(&bytes, animations + 16), 5);
    assert_eq!(read_u32(&bytes, animations + 20), 2);
    assert_eq!(
        read_u32(&bytes, animations + 24),
        assets::ANIMATION_FLAG_BLEND
    );
    assert_eq!(read_u32(&bytes, frames + 4), 0x8000_0000);
    let runtime = assets::RuntimeAssets::decode(&bytes).expect("decode exact fixture");
    assert_eq!(runtime.model_quads(), fixture.model_quads.as_ref());
    assert_eq!(runtime.animations(), fixture.animations.as_ref());
}

#[test]
fn mcbeas04_rejects_overlapping_pages() {
    let mut compiled = valid_assets();
    compiled.texture_pages = vec![
        assets::TexturePage::new(texture_array(1)),
        assets::TexturePage::new(texture_array(1)),
    ]
    .into_boxed_slice();
    let mut bytes = encode_blob(&compiled).expect("encode two pages").into_vec();
    let pages_offset = u64::from_le_bytes(bytes[152..160].try_into().unwrap()) as usize;
    let first_payload = u64::from_le_bytes(
        bytes[pages_offset + 16..pages_offset + 24]
            .try_into()
            .unwrap(),
    );
    bytes[pages_offset + 64 + 16..pages_offset + 64 + 24]
        .copy_from_slice(&first_payload.to_le_bytes());
    let payload_length = u64::from_le_bytes(bytes[192..200].try_into().unwrap()) as usize;
    let digest = Sha256::digest(&bytes[..payload_length]);
    bytes[payload_length..].copy_from_slice(&digest);
    assert!(assets::RuntimeAssets::decode(&bytes).is_err());
}

#[test]
fn mcbeas04_rejects_bad_texture_ref() {
    assert!(assets::TextureRef::from_raw(0x0010_0000).is_err());
    assert!(assets::TextureRef::new(2, 0).is_err());
    assert!(assets::TextureRef::new(0, 2_048).is_err());
}

#[test]
fn mcbeas04_rejects_noncanonical_new_tables_and_limits() {
    let quad = assets::ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material: 0,
        flags: 0,
    };
    let mut bad_template = valid_assets();
    bad_template.model_templates = vec![assets::ModelTemplate {
        quad_start: 1,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    bad_template.model_quads = vec![quad].into_boxed_slice();
    assert!(encode_blob(&bad_template).is_err());

    let mut kelp_template = valid_assets();
    kelp_template.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count: 6,
        flags: assets::MODEL_TEMPLATE_FLAG_KELP,
    }]
    .into_boxed_slice();
    let mut kelp_quads = vec![quad; 6];
    kelp_quads[4].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    kelp_quads[5].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    kelp_template.model_quads = kelp_quads.into_boxed_slice();
    assert!(encode_blob(&kelp_template).is_ok());
    kelp_template.model_templates[0].quad_count = 5;
    assert!(encode_blob(&kelp_template).is_err());
    kelp_template.model_templates[0].quad_count = 6;
    kelp_template.model_quads[0].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    assert!(encode_blob(&kelp_template).is_err());
    kelp_template.model_quads[0].flags = 0;
    kelp_template.model_quads[4].flags = 0;
    assert!(encode_blob(&kelp_template).is_err());
    kelp_template.model_quads[4].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    kelp_template.model_templates[0].flags = assets::MODEL_TEMPLATE_FLAG_KELP << 1;
    assert!(encode_blob(&kelp_template).is_err());

    let mut too_many_quads = valid_assets();
    too_many_quads.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count: 33,
        flags: 0,
    }]
    .into_boxed_slice();
    too_many_quads.model_quads = vec![quad; 33].into_boxed_slice();
    assert!(encode_blob(&too_many_quads).is_err());

    let mut bad_quad = valid_assets();
    bad_quad.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    bad_quad.model_quads = vec![assets::ModelQuad {
        flags: 0x80,
        ..quad
    }]
    .into_boxed_slice();
    assert!(encode_blob(&bad_quad).is_err());

    let mut bad_animation = valid_assets();
    bad_animation.animations = vec![assets::Animation {
        frame_start: 1,
        frame_count: 1,
        ticks_per_frame: 0,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 0,
        flags: 2,
    }]
    .into_boxed_slice();
    bad_animation.animation_frames = vec![TextureRef::DIAGNOSTIC].into_boxed_slice();
    assert!(encode_blob(&bad_animation).is_err());

    let mut third_page = valid_assets();
    third_page.texture_pages = vec![TexturePage::new(texture_array(1)); 3].into_boxed_slice();
    assert!(encode_blob(&third_page).is_err());
}

const HEADER_BYTES: usize = 200;

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
            kind: VisualKind::Diagnostic,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }]
        .into_boxed_slice(),
        hashed: vec![(0x8000_0000, 0)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(texture_array(1))].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
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
    assert_eq!(read_u32(&bytes, 32), 0, "template count");
    assert_eq!(read_u32(&bytes, 36), 0, "quad count");
    assert_eq!(read_u32(&bytes, 40), 0, "animation count");
    assert_eq!(read_u32(&bytes, 44), 0, "frame count");
    assert_eq!(read_u32(&bytes, 48), 1, "page count");
    assert_eq!(read_u32(&bytes, 52), 8, "tint-map count");
    assert_eq!(read_u32(&bytes, 56), 256, "tint-map size");
    assert_eq!(read_u32(&bytes, 60), 0, "biome-rule count");
    assert_eq!(&bytes[64..96], &[0; 32]);

    let visuals_offset = read_u64(&bytes, 96) as usize;
    let hashes_offset = read_u64(&bytes, 104) as usize;
    let materials_offset = read_u64(&bytes, 112) as usize;
    let pages_offset = read_u64(&bytes, 152) as usize;
    let textures_offset = read_u64(&bytes, 160) as usize;
    let tint_maps_offset = read_u64(&bytes, 168) as usize;
    let biome_rules_offset = read_u64(&bytes, 176) as usize;
    let biome_names_offset = read_u64(&bytes, 184) as usize;
    let payload_length = read_u64(&bytes, 192) as usize;
    assert_eq!(visuals_offset, HEADER_BYTES);
    assert_eq!(hashes_offset, visuals_offset + 40);
    assert_eq!(materials_offset, hashes_offset + 8);
    assert_eq!(pages_offset, materials_offset + 12);
    assert_eq!(textures_offset, pages_offset + 64);
    assert_eq!(tint_maps_offset, textures_offset + 1_364);
    assert_eq!(biome_rules_offset, tint_maps_offset + 8 * 256 * 256 * 3);
    assert_eq!(biome_names_offset, biome_rules_offset);
    assert_eq!(payload_length, biome_names_offset);
    assert_eq!(bytes.len(), payload_length + 32);

    let expected_hash = Sha256::digest(&bytes[..payload_length]);
    assert_eq!(&bytes[payload_length..], expected_hash.as_slice());
}

#[test]
fn blob_rejects_material_layer_visual_and_mip_invariants() {
    let mut bad_material_layer = valid_assets();
    bad_material_layer.materials[0].texture = TextureRef::new(0, 1).unwrap();
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
    bad_mip_length.texture_pages[0].texture.mips[1].rgba8 = vec![0; 7].into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_mip_length),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut bad_mip_count = valid_assets();
    bad_mip_count.texture_pages[0].texture.mips = Vec::new().into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_mip_count),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    for invalid in [
        BlockFlags::from_bits_retain(0x10),
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::AIR | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::LEAF_MODEL | BlockFlags::OCCLUDES_FULL_FACE,
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
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        },
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: MATERIAL_FLAGS_MASK | 0x800,
            animation: NO_ANIMATION,
        },
    ]
    .into_boxed_slice();
    assert!(matches!(
        encode_blob(&bad_material_flags),
        Err(AssetError::InvalidCompiledAssets { .. })
    ));

    let mut blend = valid_assets();
    let mut materials = blend.materials.into_vec();
    materials.push(Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: assets::MATERIAL_FLAG_ALPHA_BLEND,
        animation: NO_ANIMATION,
    });
    blend.materials = materials.into_boxed_slice();
    assert!(
        encode_blob(&blend).is_ok(),
        "blend is a supported render class"
    );
    blend.materials[1].flags |= assets::MATERIAL_FLAG_ALPHA_CUTOUT;
    assert!(
        encode_blob(&blend).is_err(),
        "blend and cutout are mutually exclusive"
    );
}

#[test]
fn blob_accepts_model_full_face_occluder_without_cube_geometry() {
    let mut compiled = valid_assets();
    compiled.visuals[0].flags = BlockFlags::OCCLUDES_FULL_FACE;
    compiled.visuals[0].kind = VisualKind::Model;
    compiled.visuals[0].model_template = 0;
    compiled.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.model_quads = vec![assets::ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material: 0,
        flags: 0,
    }]
    .into_boxed_slice();

    assert!(encode_blob(&compiled).is_ok());
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
    materials.materials = vec![
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION
        };
        MAX_MATERIALS + 1
    ]
    .into_boxed_slice();
    assert!(matches!(
        encode_blob(&materials),
        Err(AssetError::TooManyMaterials {
            count,
            max: MAX_MATERIALS
        }) if count == MAX_MATERIALS + 1
    ));

    let mut layers = valid_assets();
    layers.texture_pages[0].texture = TextureArray {
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
