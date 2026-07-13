use std::mem::size_of_val;

use assets::{
    BLOB_VERSION, BiomeRule, BlockFace, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAGS_MASK,
    MAX_ANIMATION_FRAMES, MAX_ANIMATIONS, MAX_MATERIALS, MAX_MODEL_QUADS, MAX_MODEL_TEMPLATES,
    Material, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode,
    RuntimeAssets, TINT_MAP_BYTES, TextureArray, TextureMip, TexturePage, TextureRef, TintSource,
    VisualKind, encode_blob,
};
use sha2::{Digest, Sha256};

#[test]
fn runtime_decodes_mcbeas04_tables() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode MCBEAS04");
    assert!(runtime.model_templates().is_empty());
    assert!(runtime.model_quads().is_empty());
    assert!(runtime.animations().is_empty());
    assert!(runtime.animation_frames().is_empty());
    assert_eq!(runtime.texture_pages().len(), 1);
}

const HEADER_BYTES: usize = 200;
const HASH_BYTES: usize = 32;
const VERSION_OFFSET: usize = 8;
const VISUAL_COUNT_OFFSET: usize = 20;
const HASH_COUNT_OFFSET: usize = 24;
const MATERIAL_COUNT_OFFSET: usize = 28;
const PAGE_COUNT_OFFSET: usize = 48;
const VISUALS_OFFSET_OFFSET: usize = 96;
const HASHES_OFFSET_OFFSET: usize = 104;
const MATERIALS_OFFSET_OFFSET: usize = 112;
const PAYLOAD_LENGTH_OFFSET: usize = 192;

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
                kind: VisualKind::Diagnostic,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
            BlockVisual {
                faces: [1, 1, 1, 1, 1, 1],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
        ]
        .into_boxed_slice(),
        // Hash 1 deliberately collides with sequential ID 1 but maps to visual 0.
        hashed: vec![(1, 0), (0xdbf4_4120, 1)].into_boxed_slice(),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            },
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags: MATERIAL_FLAG_FOLIAGE_TINT,
                animation: NO_ANIMATION,
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(texture_array(2))].into_boxed_slice(),
        biomes: CompiledBiomeAssets {
            tint_maps_rgb8: vec![0x44; TINT_MAP_BYTES].into_boxed_slice(),
            rules: vec![BiomeRule {
                id: 7,
                name: "minecraft:plains".into(),
                flags: 1,
                grass: TintSource::direct(0x11_2233),
                foliage: TintSource::direct(0x44_5566),
                dry_foliage: TintSource::direct(0x77_8899),
                water: TintSource::direct(0xaa_bbcc),
                temperature_bits: 0.8_f32.to_bits(),
                downfall_bits: 0.4_f32.to_bits(),
            }]
            .into_boxed_slice(),
        },
    }
}

fn valid_blob() -> Vec<u8> {
    encode_blob(&compiled_assets())
        .expect("encode synthetic runtime assets")
        .into_vec()
}

fn full_face_model_blob() -> Vec<u8> {
    let mut compiled = compiled_assets();
    compiled.visuals[1].flags = BlockFlags::OCCLUDES_FULL_FACE;
    compiled.visuals[1].kind = VisualKind::Model;
    compiled.visuals[1].model_template = 0;
    compiled.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.model_quads = vec![ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    encode_blob(&compiled)
        .expect("encode model full-face occluder")
        .into_vec()
}

fn rich_blob() -> Vec<u8> {
    let mut compiled = compiled_assets();
    compiled.visuals[1].flags = BlockFlags::empty();
    compiled.visuals[1].kind = VisualKind::Liquid;
    compiled.visuals[1].model_template = NO_MODEL_TEMPLATE;
    compiled.visuals[1].animation = 0;
    compiled.visuals[1].contributor_role = assets::ContributorRole::LiquidAdditional;
    compiled.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.model_quads = vec![ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material: 1,
        flags: 1,
    }]
    .into_boxed_slice();
    compiled.animations = vec![assets::Animation {
        frame_start: 0,
        frame_count: 1,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.animation_frames = vec![TextureRef::new(0, 1).unwrap()].into_boxed_slice();
    encode_blob(&compiled).unwrap().into_vec()
}

#[test]
fn runtime_decodes_checked_contributor_role_with_new_tables() {
    let runtime = RuntimeAssets::decode(&rich_blob()).expect("decode rich MCBEAS04 fixture");
    let block = runtime.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(
        block.contributor_role(),
        assets::ContributorRole::LiquidAdditional
    );
    assert_eq!(block.kind(), VisualKind::Liquid);
    assert_eq!(block.model_template(), None);
    assert_eq!(block.animation(), Some(0));
    assert_eq!(block.variant(), 0);
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
fn decode_rejects_resealed_old_schema_magic_and_version() {
    let mut old_schema = valid_blob();
    old_schema[..8].copy_from_slice(b"MCBEAS01");
    write_u32(&mut old_schema, VERSION_OFFSET, 1);
    reseal(&mut old_schema);
    assert_rejected(&old_schema, "resealed MCBEAS01 version 1 blob");
}

#[test]
fn decode_rejects_invalid_visual_flag_semantics() {
    for raw in [0x10, 0x03, 0x05, 0x08, 0x0c, 0x0e] {
        let mut blob = valid_blob();
        let visuals_offset = read_u64(&blob, VISUALS_OFFSET_OFFSET) as usize;
        blob[visuals_offset + 24] = raw;
        reseal(&mut blob);
        assert_rejected(&blob, &format!("invalid visual flags {raw:#x}"));
    }
}

#[test]
fn runtime_preserves_model_full_face_occluder_without_cube_geometry() {
    let runtime =
        RuntimeAssets::decode(&full_face_model_blob()).expect("decode model full-face occluder");
    let block = runtime.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(block.kind(), VisualKind::Model);
    assert_eq!(block.flags(), BlockFlags::OCCLUDES_FULL_FACE);
    assert!(!block.flags().contains(BlockFlags::CUBE_GEOMETRY));
}

#[test]
fn runtime_rejects_full_face_occlusion_on_non_model_visuals() {
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
        let mut blob = full_face_model_blob();
        let visuals = read_u64(&blob, VISUALS_OFFSET_OFFSET) as usize;
        blob[visuals + 40 + 25] = kind as u8;
        blob[visuals + 40 + 26] = role as u8;
        write_u32(&mut blob, visuals + 40 + 28, template);
        reseal(&mut blob);
        assert_rejected(&blob, &format!("standalone occlusion on {kind:?}"));
    }

    let mut compiled = compiled_assets();
    compiled.visuals[1].flags = BlockFlags::empty();
    compiled.visuals[1].kind = VisualKind::Model;
    compiled.visuals[1].model_template = 0;
    compiled.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags: 0,
    }]
    .into_boxed_slice();
    compiled.model_quads = Box::new([]);
    let mut zero_quad = encode_blob(&compiled)
        .expect("encode non-occluding zero-quad model")
        .into_vec();
    let visuals = read_u64(&zero_quad, VISUALS_OFFSET_OFFSET) as usize;
    zero_quad[visuals + 40 + 24] = BlockFlags::OCCLUDES_FULL_FACE.bits();
    reseal(&mut zero_quad);
    assert_rejected(&zero_quad, "standalone occlusion on zero-quad Model");
}

#[test]
fn decode_rejects_material_flags_outside_supported_mask() {
    let mut blob = valid_blob();
    let materials_offset = read_u64(&blob, MATERIALS_OFFSET_OFFSET) as usize;
    write_u32(
        &mut blob,
        materials_offset + 16,
        MATERIAL_FLAGS_MASK | 0x800,
    );
    reseal(&mut blob);
    assert_rejected(&blob, "material flags outside supported mask");
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
    write_u32(&mut bad_material_layer, materials_offset + 12, 2);
    reseal(&mut bad_material_layer);
    assert_rejected(&bad_material_layer, "material layer out of range");
}

#[test]
fn decode_rejects_mip_length_mismatches_and_allocation_limits() {
    let mut wrong_texture_length = valid_blob();
    let pages_offset = read_u64(&wrong_texture_length, 152) as usize;
    let texture_length = read_u64(&wrong_texture_length, pages_offset + 24);
    write_u64(
        &mut wrong_texture_length,
        pages_offset + 24,
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
        (PAGE_COUNT_OFFSET, 3, "texture page allocation limit"),
    ] {
        let mut oversized = valid_blob();
        write_u32(&mut oversized, offset, value);
        reseal(&mut oversized);
        assert_rejected(&oversized, case);
    }
}

#[test]
fn decode_rejects_page_payload_hash_reserved_bits_and_noncanonical_ranges() {
    let mut bad_page_hash = valid_blob();
    let pages_offset = read_u64(&bad_page_hash, 152) as usize;
    bad_page_hash[pages_offset + 32] ^= 1;
    reseal(&mut bad_page_hash);
    assert_rejected(&bad_page_hash, "page payload hash mismatch");

    let mut bad_page_reserved = valid_blob();
    let pages_offset = read_u64(&bad_page_reserved, 152) as usize;
    write_u32(&mut bad_page_reserved, pages_offset + 12, 1);
    reseal(&mut bad_page_reserved);
    assert_rejected(&bad_page_reserved, "page descriptor reserved bits");

    let mut bad_page_offset = valid_blob();
    let pages_offset = read_u64(&bad_page_offset, 152) as usize;
    let texture_offset = read_u64(&bad_page_offset, pages_offset + 16);
    write_u64(&mut bad_page_offset, pages_offset + 16, texture_offset + 1);
    reseal(&mut bad_page_offset);
    assert_rejected(&bad_page_offset, "page payload gap");

    let mut bad_texture_reserved = valid_blob();
    let materials_offset = read_u64(&bad_texture_reserved, MATERIALS_OFFSET_OFFSET) as usize;
    write_u32(
        &mut bad_texture_reserved,
        materials_offset + 12,
        0x0010_0000,
    );
    reseal(&mut bad_texture_reserved);
    assert_rejected(&bad_texture_reserved, "texture reference reserved bits");
}

#[test]
fn decode_rejects_malformed_model_animation_and_visual_sections() {
    let mutate = |offset: usize, value: u32, case: &str| {
        let mut bytes = rich_blob();
        write_u32(&mut bytes, offset, value);
        reseal(&mut bytes);
        assert_rejected(&bytes, case);
    };
    let mutate_byte = |offset: usize, value: u8, case: &str| {
        let mut bytes = rich_blob();
        bytes[offset] = value;
        reseal(&mut bytes);
        assert_rejected(&bytes, case);
    };
    let blob = rich_blob();
    let visuals = read_u64(&blob, VISUALS_OFFSET_OFFSET) as usize;
    let templates = read_u64(&blob, 120) as usize;
    let quads = read_u64(&blob, 128) as usize;
    let animations = read_u64(&blob, 136) as usize;
    let frames = read_u64(&blob, 144) as usize;

    mutate_byte(visuals + 40 + 25, 99, "unknown visual kind");
    mutate_byte(visuals + 40 + 26, 99, "unknown contributor role");
    mutate_byte(visuals + 40 + 27, 1, "visual reserved byte");
    mutate_byte(visuals + 40 + 26, 0, "liquid with primary contributor role");
    mutate_byte(
        visuals + 40 + 25,
        VisualKind::Invisible as u8,
        "invisible liquid contributor",
    );
    mutate_byte(
        visuals + 40 + 24,
        BlockFlags::CUBE_GEOMETRY.bits(),
        "liquid with cube flags",
    );
    mutate(visuals + 40 + 28, 1, "visual template ID");
    mutate(visuals + 40 + 32, 1, "visual animation ID");
    mutate(templates, 1, "noncanonical template start");
    mutate(
        templates + 8,
        assets::MODEL_TEMPLATE_FLAG_KELP << 1,
        "unknown template flags",
    );
    mutate(quads + 40, 2, "quad material ID");
    mutate(quads + 44, 0x80, "quad unknown flags");
    mutate(animations, 1, "noncanonical animation start");
    mutate(animations + 8, 0, "zero animation ticks");
    mutate(animations + 20, 0, "zero animation replication");
    mutate(animations + 24, 2, "unknown animation flags");
    mutate(frames, 0x0010_0000, "animation frame reserved bits");
}

#[test]
fn decode_accepts_known_kelp_template_flag() {
    let mut compiled = compiled_assets();
    compiled.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 6,
        flags: assets::MODEL_TEMPLATE_FLAG_KELP,
    }]
    .into_boxed_slice();
    let mut quads = vec![
        ModelQuad {
            positions: [[0; 3]; 4],
            uvs: [[0; 2]; 4],
            material: 0,
            flags: 0,
        };
        6
    ];
    quads[4].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    quads[5].flags = assets::MODEL_QUAD_FLAG_TWO_SIDED;
    compiled.model_quads = quads.into_boxed_slice();
    let blob = encode_blob(&compiled).expect("encode known kelp template");
    let runtime = RuntimeAssets::decode(&blob).expect("decode known kelp template flag");
    assert_eq!(
        runtime.model_templates()[0].flags,
        assets::MODEL_TEMPLATE_FLAG_KELP
    );
}

#[test]
fn decode_rejects_kelp_flag_on_noncanonical_template_shape() {
    let mut blob = rich_blob();
    let templates = read_u64(&blob, 120) as usize;
    write_u32(&mut blob, templates + 8, assets::MODEL_TEMPLATE_FLAG_KELP);
    reseal(&mut blob);
    assert_rejected(&blob, "one-quad kelp template");
}

#[test]
fn decode_rejects_new_section_count_limits_before_allocation() {
    for (offset, value, case) in [
        (32, MAX_MODEL_TEMPLATES as u32 + 1, "template count limit"),
        (36, MAX_MODEL_QUADS as u32 + 1, "quad count limit"),
        (40, MAX_ANIMATIONS as u32 + 1, "animation count limit"),
        (
            44,
            MAX_ANIMATION_FRAMES as u32 + 1,
            "animation frame count limit",
        ),
    ] {
        let mut bytes = rich_blob();
        write_u32(&mut bytes, offset, value);
        reseal(&mut bytes);
        assert_rejected(&bytes, case);
    }
}

#[test]
fn explicit_network_id_mode_keeps_sequential_and_hash_lookups_isolated() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode valid blob");

    let sequential = runtime.resolve(NetworkIdMode::Sequential, 1);
    assert!(sequential.is_known());
    assert_eq!(
        sequential.flags(),
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    );
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
    assert_eq!(
        runtime.material(u32::MAX),
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION
        }
    );
    assert_eq!(runtime.missing_count(), 10_001);
    assert_eq!(
        runtime.material(1),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            flags: MATERIAL_FLAG_FOLIAGE_TINT,
            animation: NO_ANIMATION
        }
    );
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
    assert_eq!(runtime.biome_assets(), &compiled_assets().biomes);
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
    assert_eq!(
        runtime.material(0),
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION
        }
    );
    assert_eq!(runtime.texture_array().layers, 1);
    assert_eq!(runtime.texture_array().mips.len(), 5);
    assert_eq!(runtime.texture_array().mips[0].rgba8.len(), 16 * 16 * 4);

    assert!(
        !runtime.resolve(NetworkIdMode::Hashed, 0).is_known(),
        "diagnostic fallback must not blur sequential and hashed namespaces"
    );
}
