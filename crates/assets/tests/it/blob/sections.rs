//! Blob binary-section layout and model-occluder checks split from the carrier tests.
use super::*;

fn full_face_model_assets(quad_count: u32) -> CompiledAssets {
    let mut compiled = valid_assets();
    compiled.visuals[0].flags = BlockFlags::OCCLUDES_FULL_FACE;
    compiled.visuals[0].kind = VisualKind::Model;
    compiled.visuals[0].support = VisualSupport::Exact;
    compiled.visuals[0].model_template = 0;
    compiled.model_templates = vec![assets::ModelTemplate {
        quad_start: 0,
        quad_count,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.model_quads = vec![
        assets::ModelQuad {
            positions: [[0; 3]; 4],
            uvs: [[0; 2]; 4],
            material: 0,
            flags: 0,
        };
        quad_count as usize
    ]
    .into_boxed_slice();
    compiled
}

#[test]
fn world_carrier_rejects_the_previous_page_encoding_at_the_header() {
    let previous_version = BLOB_VERSION - 1;
    let previous_magic = format!("MCBEAS{previous_version:02}");
    for (magic, expected_detail) in [
        (previous_magic.as_bytes(), "invalid world asset magic"),
        (BLOB_MAGIC.as_slice(), "unsupported world asset header"),
    ] {
        let mut bytes = encode_blob(&valid_assets()).unwrap().into_vec();
        // An older carrier must fail before reference validation, even with current magic.
        bytes[..8].copy_from_slice(magic);
        bytes[8..12].copy_from_slice(&previous_version.to_le_bytes());
        let payload_length = bytes.len() - 32;
        let digest = Sha256::digest(&bytes[..payload_length]);
        bytes[payload_length..].copy_from_slice(&digest);
        assert!(matches!(assets::RuntimeAssets::decode(&bytes),
            Err(AssetError::InvalidCompiledAssets { detail }) if detail.as_ref() == expected_detail));
    }
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
    assert_eq!(&bytes[64..96], &FIXTURE_PROVENANCE.source_manifest_sha256);
    assert_eq!(&bytes[96..128], &FIXTURE_PROVENANCE.block_registry_sha256);
    assert_eq!(&bytes[128..160], &FIXTURE_PROVENANCE.light_registry_sha256);
    assert_eq!(&bytes[160..192], &FIXTURE_PROVENANCE.biome_registry_sha256);

    let visuals_offset = read_u64(&bytes, 192) as usize;
    let hashes_offset = read_u64(&bytes, 200) as usize;
    let materials_offset = read_u64(&bytes, 208) as usize;
    let pages_offset = read_u64(&bytes, 248) as usize;
    let textures_offset = read_u64(&bytes, 256) as usize;
    let tint_maps_offset = read_u64(&bytes, 264) as usize;
    let biome_rules_offset = read_u64(&bytes, 272) as usize;
    let biome_names_offset = read_u64(&bytes, 280) as usize;
    let payload_length = read_u64(&bytes, 288) as usize;
    assert_eq!(visuals_offset, HEADER_BYTES);
    assert_eq!(hashes_offset, visuals_offset + assets::blob::VISUAL_BYTES);
    assert_eq!(materials_offset, hashes_offset + 8);
    assert_eq!(pages_offset, materials_offset + assets::MATERIAL_BYTES);
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
        BlockFlags::from_bits_retain(!BlockFlags::all().bits()),
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
            ..assets::Material::unvaried()
        },
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: assets::stem::MATERIAL_FLAG | assets::MATERIAL_FLAG_GRASS_TINT,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
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
        ..assets::Material::unvaried()
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
fn cauldron_water_material_roundtrips_only_with_untinted_blending() {
    let mut compiled = valid_assets();
    let mut materials = compiled.materials.into_vec();
    let mut water = materials[0];
    water.flags = assets::MATERIAL_FLAG_CAULDRON_WATER | assets::MATERIAL_FLAG_ALPHA_BLEND;
    materials.push(water);
    compiled.materials = materials.into_boxed_slice();
    let encoded = encode_blob(&compiled).unwrap();
    let runtime = assets::RuntimeAssets::decode(&encoded).unwrap();
    assert_eq!(runtime.material(1).flags, water.flags);
    for flags in [
        assets::MATERIAL_FLAG_CAULDRON_WATER,
        water.flags | assets::MATERIAL_FLAG_WATER_TINT,
        water.flags | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ] {
        compiled.materials[1].flags = flags;
        assert!(matches!(
            encode_blob(&compiled),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }
}

#[test]
fn blob_accepts_model_full_face_occluder_without_cube_geometry() {
    let compiled = full_face_model_assets(1);
    assert!(encode_blob(&compiled).is_ok());
}

#[test]
fn blob_rejects_full_face_occlusion_on_nondrawable_non_cube_visuals() {
    for (kind, role, template) in [
        (
            VisualKind::Diagnostic,
            assets::ContributorRole::Primary,
            NO_MODEL_TEMPLATE,
        ),
        (VisualKind::Cross, assets::ContributorRole::Primary, 0),
        (
            VisualKind::Liquid,
            assets::ContributorRole::LiquidAdditional,
            NO_MODEL_TEMPLATE,
        ),
        (
            VisualKind::Invisible,
            assets::ContributorRole::Primary,
            NO_MODEL_TEMPLATE,
        ),
    ] {
        let mut compiled = full_face_model_assets(1);
        compiled.visuals[0].kind = kind;
        compiled.visuals[0].contributor_role = role;
        compiled.visuals[0].model_template = template;
        assert!(
            encode_blob(&compiled).is_err(),
            "accepted standalone occlusion on {kind:?}"
        );
    }

    assert!(
        encode_blob(&full_face_model_assets(0)).is_err(),
        "accepted standalone occlusion on a zero-quad Model"
    );
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
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
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

#[test]
fn world_destroy_facts_roundtrip_and_reject_forged_source() {
    use assets::compiled::block_destroy::{BlockDestroyFacts, builtin_destroy_facts};
    let mut fixture = valid_assets();
    for facts in [
        BlockDestroyFacts::Unresolved,
        builtin_destroy_facts("minecraft:stone"),
        builtin_destroy_facts("minecraft:bedrock"),
    ] {
        fixture.visuals[0].destroy = facts;
        let bytes = encode_blob(&fixture).unwrap();
        let decoded = assets::RuntimeAssets::decode(&bytes).unwrap();
        assert_eq!(
            decoded
                .resolve(assets::NetworkIdMode::Sequential, 0)
                .destroy(),
            facts
        );
        let mut forged = bytes.into_vec();
        forged[assets::blob::DESTROY_SHA_OFFSET] ^= 1;
        let end = forged.len() - 32;
        let digest = Sha256::digest(&forged[..end]);
        forged[end..].copy_from_slice(&digest);
        assert!(
            matches!(assets::RuntimeAssets::decode(&forged), Err(AssetError::InvalidCompiledAssets { detail }) if detail.contains("destroy source identity"))
        );
    }
}
