use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assets::{
    AssetError, BlockFace, BlockFlags, CompiledAssets, DIAGNOSTIC_MATERIAL,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK,
    MATERIAL_FLAG_UV_MASK, MATERIAL_FLAGS_MASK, MAX_TEXTURE_LAYERS, Material, RegistryRecord,
    compile_pack, encode_blob,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use tempfile::TempDir;

const TILE_SIZE: u32 = 16;

#[test]
fn assetc_root_help_documents_all_compile_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .arg("--help")
        .output()
        .expect("run assetc help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    for required in ["compile", "--pack", "--registry", "--out"] {
        assert!(help.contains(required), "help omitted {required}:\n{help}");
    }
}

fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture");
}

fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    write_file(root.join("blocks.json"), blocks);
    write_file(root.join("textures/terrain_texture.json"), terrain);
    write_file(root.join("textures/flipbook_textures.json"), flipbooks);
}

fn png_bytes(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(pixels.len(), (width * height) as usize);
    let rgba = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect::<Vec<_>>();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .expect("encode synthetic PNG");
    png
}

fn tga_bytes(width: u16, height: u16, pixels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
    let mut tga = vec![0; 18];
    tga[2] = 2;
    tga[12..14].copy_from_slice(&width.to_le_bytes());
    tga[14..16].copy_from_slice(&height.to_le_bytes());
    tga[16] = 32;
    tga[17] = 0x28;
    for &[red, green, blue, alpha] in pixels {
        tga.extend_from_slice(&[blue, green, red, alpha]);
    }
    tga
}

fn solid(width: u32, height: u32, color: [u8; 4]) -> Vec<[u8; 4]> {
    vec![color; (width * height) as usize]
}

fn write_png(root: &Path, source_path: &str, width: u32, height: u32, pixels: &[[u8; 4]]) {
    write_file(
        root.join(format!("{source_path}.png")),
        png_bytes(width, height, pixels),
    );
}

fn write_tga(root: &Path, source_path: &str, width: u16, height: u16, pixels: &[[u8; 4]]) {
    write_file(
        root.join(format!("{source_path}.tga")),
        tga_bytes(width, height, pixels),
    );
}

fn record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    state: &str,
    flags: BlockFlags,
) -> RegistryRecord {
    RegistryRecord {
        sequential_id,
        network_hash,
        name: name.into(),
        canonical_state: state.into(),
        flags,
    }
}

fn material_for_face(compiled: &CompiledAssets, sequential_id: usize, face: BlockFace) -> Material {
    compiled.materials[compiled.visuals[sequential_id].faces[face as usize] as usize]
}

fn leaf_material_fixture() -> (TempDir, PathBuf, Vec<RegistryRecord>) {
    let directory = tempfile::tempdir().expect("create leaf fixture");
    let resource_pack = directory.path().join("resource_pack");
    write_pack(
        &resource_pack,
        r#"{
            "stone": {"textures": "shared"},
            "cherry_leaves": {"textures": "shared"},
            "azalea_leaves": {"textures": "azalea"},
            "azalea_leaves_flowered": {"textures": "flowered"}
        }"#,
        r#"{"texture_data": {
            "shared": {"textures": "textures/blocks/a_shared"},
            "azalea": {"textures": "textures/blocks/b_azalea"},
            "flowered": {"textures": "textures/blocks/c_flowered"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("textures/blocks/a_shared", [220, 80, 90, 255]),
        ("textures/blocks/b_azalea", [40, 180, 80, 255]),
        ("textures/blocks/c_flowered", [220, 120, 180, 255]),
    ] {
        write_png(
            &resource_pack,
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    let leaf = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = vec![
        record(
            0,
            100,
            "minecraft:stone",
            "{}",
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        ),
        record(1, 101, "minecraft:cherry_leaves", "{}", leaf),
        record(2, 102, "minecraft:azalea_leaves", "{}", leaf),
        record(3, 103, "minecraft:azalea_leaves_flowered", "{}", leaf),
    ];
    (directory, resource_pack, records)
}

fn biome_registry_bytes(id: u32, name: &str) -> Vec<u8> {
    let mut bytes = b"BIOREG01".to_vec();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(
        &u16::try_from(name.len())
            .expect("small fixture name")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

fn write_biome_fixture(resource_pack: &Path) {
    write_file(
        resource_pack.join("biomes/plains.client_biome.json"),
        r#"{
            "format_version":"1.21.0",
            "minecraft:client_biome":{
                "description":{"identifier":"minecraft:plains"},
                "components":{}
            }
        }"#,
    );
    let behavior_pack = resource_pack
        .parent()
        .expect("resource pack has fixture parent")
        .join("behavior_pack");
    write_file(
        behavior_pack.join("biomes/plains.biome.json"),
        r#"{
            "format_version":"1.21.0",
            "minecraft:biome":{
                "description":{"identifier":"minecraft:plains"},
                "components":{"minecraft:climate":{"temperature":0.8,"downfall":0.4}}
            }
        }"#,
    );
    for name in [
        "grass",
        "foliage",
        "birch",
        "evergreen",
        "swamp_grass",
        "swamp_foliage",
        "mangrove_swamp_foliage",
        "dry_foliage",
    ] {
        write_png(
            resource_pack,
            &format!("textures/colormap/{name}"),
            256,
            256,
            &solid(256, 256, [80, 160, 40, 255]),
        );
    }
}

fn registry_bytes(records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = b"BREG1002".to_vec();
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(record.flags.bits());
        bytes.extend_from_slice(
            &u16::try_from(record.name.len())
                .expect("small fixture name")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(
            &u32::try_from(record.canonical_state.len())
                .expect("small fixture state")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(record.name.as_bytes());
        bytes.extend_from_slice(record.canonical_state.as_bytes());
    }
    bytes
}

fn shuffled_records(records: &[RegistryRecord], mut state: u64) -> Vec<RegistryRecord> {
    let mut shuffled = records.to_vec();
    for upper in (1..shuffled.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let bound = u64::try_from(upper + 1).expect("fixture bound fits u64");
        let index = usize::try_from(state % bound).expect("shuffle index fits usize");
        shuffled.swap(upper, index);
    }
    shuffled
}

fn mip_layer(compiled: &CompiledAssets, mip_index: usize, layer: u32) -> &[u8] {
    let mip = &compiled.textures.mips[mip_index];
    let layer_bytes = usize::try_from(mip.size * mip.size * 4).expect("small mip");
    let start = usize::try_from(layer).expect("small layer") * layer_bytes;
    &mip.rgba8[start..start + layer_bytes]
}

fn alpha_survivors(rgba: &[u8]) -> usize {
    assert_eq!(rgba.len() % 4, 0);
    rgba.chunks_exact(4).filter(|pixel| pixel[3] >= 128).count()
}

fn scaled_survivors(raw_rgba: &[u8], scale: u32) -> usize {
    raw_rgba
        .chunks_exact(4)
        .filter(|pixel| {
            let alpha = ((u32::from(pixel[3]) * scale + 0x8000) >> 16).min(255) as u8;
            alpha >= 128
        })
        .count()
}

fn reference_nearest_scale(raw_rgba: &[u8], target: usize) -> u32 {
    const SCALE_MAX: u32 = 16 << 16;
    const SURVIVOR_NUMERATOR: u32 = (128 << 16) - 0x8000;
    let mut candidates = vec![0];
    for alpha in raw_rgba.chunks_exact(4).map(|pixel| pixel[3]) {
        if alpha == 0 {
            continue;
        }
        let alpha = u32::from(alpha);
        let threshold = SURVIVOR_NUMERATOR.div_ceil(alpha);
        if threshold <= SCALE_MAX {
            candidates.push(threshold.saturating_sub(1));
            candidates.push(threshold);
        }
    }
    assert!(candidates.len() <= raw_rgba.len() / 2 + 1);
    candidates.sort_unstable();
    candidates.dedup();
    candidates
        .into_iter()
        .min_by_key(|&scale| (scaled_survivors(raw_rgba, scale).abs_diff(target), scale))
        .expect("scale zero is always present")
}

fn reference_nearest_survivors(raw_rgba: &[u8], target: usize) -> usize {
    scaled_survivors(raw_rgba, reference_nearest_scale(raw_rgba, target))
}

fn cutout_pattern(colour: [u8; 3], threshold: u32) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let alpha = if ((x * 17 + y * 29 + x * y * 7) & 255) < threshold {
                255
            } else {
                0
            };
            pixels.push([colour[0], colour[1], colour[2], alpha]);
        }
    }
    pixels
}

fn aligned_half_pattern(colour: [u8; 3]) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for _y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            pixels.push([colour[0], colour[1], colour[2], u8::MAX * u8::from(x < 8)]);
        }
    }
    pixels
}

fn reference_raw_mips(base: &[[u8; 4]], colour: [u8; 3]) -> Vec<Vec<u8>> {
    let mut size = TILE_SIZE;
    let mut current = base.to_vec();
    let mut mips = vec![current.iter().flatten().copied().collect::<Vec<_>>()];
    while size > 1 {
        let target_size = size / 2;
        let mut target = Vec::with_capacity((target_size * target_size) as usize);
        for y in 0..target_size {
            for x in 0..target_size {
                let mut alpha_sum = 0_u32;
                for offset_y in 0..2 {
                    for offset_x in 0..2 {
                        let source = ((y * 2 + offset_y) * size + x * 2 + offset_x) as usize;
                        alpha_sum += u32::from(current[source][3]);
                    }
                }
                let rgb = if alpha_sum == 0 { [0; 3] } else { colour };
                target.push([rgb[0], rgb[1], rgb[2], ((alpha_sum + 2) / 4) as u8]);
            }
        }
        mips.push(target.iter().flatten().copied().collect());
        current = target;
        size = target_size;
    }
    mips
}

#[test]
fn compiler_marks_only_leaf_faces_as_alpha_cutout() {
    let (_directory, resource_pack, records) = leaf_material_fixture();
    let compiled = compile_pack(&resource_pack, &records).expect("compile leaf materials");

    assert_eq!(MATERIAL_FLAG_UV_MASK, 0x0f);
    assert_eq!(MATERIAL_FLAG_ALPHA_CUTOUT, 0x100);
    assert_eq!(MATERIAL_FLAGS_MASK, 0x77f);
    assert_eq!(std::mem::size_of::<Material>(), 8);
    let opaque_id = compiled.visuals[0].faces[BlockFace::Up as usize];
    let opaque = compiled.materials[opaque_id as usize];
    assert_eq!(opaque.flags & MATERIAL_FLAG_ALPHA_CUTOUT, 0);
    for leaf in 1..=3 {
        for face in BlockFace::ALL {
            let material_id = compiled.visuals[leaf].faces[face as usize];
            assert_ne!(material_id, DIAGNOSTIC_MATERIAL);
            let material = compiled.materials[material_id as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_UV_MASK, 0);
        }
    }
    let cherry_id = compiled.visuals[1].faces[BlockFace::Up as usize];
    let cherry = compiled.materials[cherry_id as usize];
    assert_eq!(
        opaque.layer, cherry.layer,
        "pixels must remain deduplicated"
    );
    assert_ne!(
        opaque_id, cherry_id,
        "opaque and cutout descriptors must differ"
    );
    assert!(
        compiled
            .materials
            .iter()
            .all(|material| material.flags & !MATERIAL_FLAGS_MASK == 0)
    );

    let baseline = encode_blob(&compiled).expect("encode cutout baseline");
    for iteration in 0..100_u64 {
        let shuffled = shuffled_records(&records, 0x9e37_79b9 ^ iteration);
        let actual = compile_pack(&resource_pack, &shuffled).expect("compile shuffled cutout pack");
        assert_eq!(encode_blob(&actual).expect("encode shuffle"), baseline);
    }
}

#[test]
fn compiler_assigns_generic_birch_evergreen_and_self_colored_leaf_flags() {
    let directory = tempfile::tempdir().expect("create leaf class fixture");
    write_pack(
        directory.path(),
        r#"{
            "oak_leaves":{"textures":"leaves"},
            "birch_leaves":{"textures":"leaves"},
            "spruce_leaves":{"textures":"leaves"},
            "cherry_leaves":{"textures":"leaves"}
        }"#,
        r#"{"texture_data":{"leaves":{"textures":"textures/blocks/leaves"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/leaves",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [80, 160, 40, 255]),
    );
    let leaf = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 100, "minecraft:oak_leaves", "{}", leaf),
        record(1, 101, "minecraft:birch_leaves", "{}", leaf),
        record(2, 102, "minecraft:spruce_leaves", "{}", leaf),
        record(3, 103, "minecraft:cherry_leaves", "{}", leaf),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile leaf classes");
    let flags = |record_id| material_for_face(&compiled, record_id, BlockFace::Up).flags;

    assert_eq!(
        flags(0),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT
    );
    assert_eq!(
        flags(1),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE
    );
    assert_eq!(
        flags(2),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE
    );
    assert_eq!(flags(3), MATERIAL_FLAG_ALPHA_CUTOUT);
    assert_eq!(flags(3) & MATERIAL_FLAG_TINT_MASK, 0);
    assert_eq!(flags(3) & MATERIAL_FLAG_FOLIAGE_CLASS_MASK, 0);
}

#[test]
fn assetc_summary_reports_deterministic_cutout_material_count() {
    let (directory, resource_pack, records) = leaf_material_fixture();
    let registry = directory.path().join("registry.bin");
    let biome_registry = directory.path().join("biome-registry.bin");
    let output_blob = directory.path().join("vanilla-v1001.mcbea");
    fs::write(&registry, registry_bytes(&records)).expect("write registry fixture");
    fs::write(&biome_registry, biome_registry_bytes(0, "minecraft:plains"))
        .expect("write biome registry fixture");
    write_biome_fixture(&resource_pack);
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["compile", "--pack"])
        .arg(&resource_pack)
        .arg("--registry")
        .arg(&registry)
        .arg("--biome-registry")
        .arg(&biome_registry)
        .arg("--out")
        .arg(&output_blob)
        .output()
        .expect("run assetc compile");
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 summary"),
        format!(
            "compiled 4 visuals, 5 materials (3 alpha cutout), 4 texture layers, and 1 biome rules to {}\n",
            output_blob.display()
        )
    );
}

#[test]
fn cutout_mips_preserve_each_layer_coverage_without_cross_layer_bleed() {
    let directory = tempfile::tempdir().expect("create coverage fixture");
    write_pack(
        directory.path(),
        r#"{
            "cherry_leaves":{"textures":"red"},
            "azalea_leaves":{"textures":"blue"},
            "azalea_leaves_flowered":{"textures":"green"}
        }"#,
        r#"{"texture_data":{
            "red":{"textures":"textures/blocks/a_red"},
            "blue":{"textures":"textures/blocks/b_blue"},
            "green":{"textures":"textures/blocks/c_green"}
        }}"#,
        "[]",
    );
    let red = cutout_pattern([255, 0, 0], 78);
    let blue = cutout_pattern([0, 0, 255], 181);
    let green = aligned_half_pattern([0, 255, 0]);
    for (path, pixels) in [
        ("textures/blocks/a_red", &red),
        ("textures/blocks/b_blue", &blue),
        ("textures/blocks/c_green", &green),
    ] {
        write_png(directory.path(), path, TILE_SIZE, TILE_SIZE, pixels);
    }
    let flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 200, "minecraft:cherry_leaves", "{}", flags),
        record(1, 201, "minecraft:azalea_leaves", "{}", flags),
        record(2, 202, "minecraft:azalea_leaves_flowered", "{}", flags),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile coverage fixture");
    let red_layer = material_for_face(&compiled, 0, BlockFace::Up).layer;
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up).layer;
    assert_eq!(
        blue_layer,
        red_layer + 1,
        "red and blue must be adjacent layers"
    );

    let mut correction_exercised = false;
    for (record_id, base, colour, no_tie) in [
        (0, red.as_slice(), [255, 0, 0], false),
        (1, blue.as_slice(), [0, 0, 255], false),
        (2, green.as_slice(), [0, 255, 0], true),
    ] {
        let layer = material_for_face(&compiled, record_id, BlockFace::Up).layer;
        let raw_mips = reference_raw_mips(base, colour);
        let base_survivors = alpha_survivors(&raw_mips[0]);
        for (mip_index, mip) in compiled.textures.mips.iter().enumerate() {
            let actual = mip_layer(&compiled, mip_index, layer);
            let raw = &raw_mips[mip_index];
            let pixels = usize::try_from(mip.size * mip.size).expect("small mip");
            let target = (base_survivors * pixels + 128) / 256;
            let expected_scale = reference_nearest_scale(raw, target);
            assert_eq!(
                alpha_survivors(actual),
                reference_nearest_survivors(raw, target),
                "coverage mismatch for record {record_id} mip {mip_index}"
            );
            for (actual, raw) in actual.chunks_exact(4).zip(raw.chunks_exact(4)) {
                assert_eq!(&actual[..3], &raw[..3], "coverage scaling changed RGB");
                let expected_alpha = if mip_index == 0 {
                    raw[3]
                } else {
                    ((u32::from(raw[3]) * expected_scale + 0x8000) >> 16).min(255) as u8
                };
                assert_eq!(
                    actual[3], expected_alpha,
                    "coverage scaling chose the wrong tie-break scale"
                );
            }
            if record_id == 0 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[2] == 0));
            } else if record_id == 1 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[0] == 0));
            }
            if no_tie {
                assert_eq!(alpha_survivors(raw), target, "no-tie fixture missed target");
                assert_eq!(alpha_survivors(actual), target);
            } else if mip_index > 0 && alpha_survivors(raw) != target {
                correction_exercised = true;
            }
        }
    }
    assert!(
        correction_exercised,
        "patterns did not exercise coverage correction"
    );
}

#[test]
fn compiler_rejects_invalid_block_flag_semantics() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"format_version":[1,1,0]}"#,
        r#"{"texture_data":{}}"#,
        "[]",
    );

    for invalid in [
        BlockFlags::from_bits_retain(0x10),
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        let records = [record(0, 1, "minecraft:test", "{}", invalid)];
        assert!(matches!(
            compile_pack(directory.path(), &records),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }
}

fn mip_pixel(
    compiled: &CompiledAssets,
    mip_index: usize,
    layer: u32,
    x: usize,
    y: usize,
) -> [u8; 4] {
    let mip = &compiled.textures.mips[mip_index];
    let size = mip.size as usize;
    let layer_bytes = size * size * 4;
    let offset = layer as usize * layer_bytes + (y * size + x) * 4;
    mip.rgba8[offset..offset + 4]
        .try_into()
        .expect("RGBA pixel")
}

#[test]
fn compiler_deduplicates_pixels_without_conflating_uv_flags() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "same_a": {"textures": "red_a"},
            "same_b": {"textures": "red_b"},
            "pillar": {"textures": {"up": "red_a", "down": "red_a", "side": "red_a"}},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red_a": {"textures": "textures/blocks/a_red"},
            "red_b": {"textures": "textures/blocks/b_red_copy"},
            "blue": {"textures": "textures/blocks/c_blue"}
        }}"#,
        "[]",
    );
    let red = solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]);
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/b_red_copy",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/c_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 100, "minecraft:same_a", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 101, "minecraft:same_b", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            102,
            "minecraft:pillar",
            r#"{"pillar_axis":"x"}"#,
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(3, 103, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(compiled.textures.layers, 3, "diagnostic + red + blue");
    assert_eq!(
        compiled.materials.len(),
        4,
        "diagnostic + three descriptors"
    );
    assert_eq!(compiled.visuals[0].faces, compiled.visuals[1].faces);

    let red_plain = material_for_face(&compiled, 0, BlockFace::Up);
    let red_rotated = material_for_face(&compiled, 2, BlockFace::North);
    let blue = material_for_face(&compiled, 3, BlockFace::Up);
    assert_eq!(red_plain.layer, red_rotated.layer);
    assert_eq!(red_plain.flags, 0);
    assert_eq!(red_rotated.flags, MATERIAL_FLAG_ROTATE_UV);
    assert_ne!(red_plain, red_rotated);
    assert_ne!(red_plain.layer, blue.layer);
}

#[test]
fn compiler_builds_diagnostic_and_layer_isolated_linear_mips() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "red": {"textures": "red"},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red": {"textures": "textures/blocks/a_red"},
            "blue": {"textures": "textures/blocks/b_blue"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]),
    );
    write_png(
        directory.path(),
        "textures/blocks/b_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 200, "minecraft:red", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 201, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(compiled.materials[0], Material { layer: 0, flags: 0 });
    assert_eq!(mip_pixel(&compiled, 0, 0, 0, 0), [255, 0, 255, 255]);
    assert_eq!(mip_pixel(&compiled, 0, 0, 1, 0), [0, 0, 0, 255]);
    assert_eq!(compiled.textures.mips.len(), 5);
    assert_eq!(
        compiled
            .textures
            .mips
            .iter()
            .map(|mip| mip.size)
            .collect::<Vec<_>>(),
        [16, 8, 4, 2, 1]
    );

    let red_layer = material_for_face(&compiled, 0, BlockFace::Up).layer;
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up).layer;
    for (mip_index, mip) in compiled.textures.mips.iter().enumerate() {
        for y in 0..mip.size as usize {
            for x in 0..mip.size as usize {
                assert_eq!(
                    mip_pixel(&compiled, mip_index, red_layer, x, y),
                    [255, 0, 0, 255]
                );
                assert_eq!(
                    mip_pixel(&compiled, mip_index, blue_layer, x, y),
                    [0, 0, 255, 255]
                );
            }
        }
    }
}

#[test]
fn compiler_fails_closed_for_transparent_and_tinted_full_cubes() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "stone": {"textures": "stone"},
            "glass": {"textures": "glass"},
            "tinted_cube": {"textures": "tinted_cube"},
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }}
        }"#,
        r##"{"texture_data": {
            "stone": {"textures": "textures/blocks/stone"},
            "glass": {"textures": "textures/blocks/glass"},
            "tinted_cube": {"textures": {
                "path": "textures/blocks/tinted_cube", "overlay_color": "#79c05a"
            }},
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"}
        }}"##,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let mut glass = solid(TILE_SIZE, TILE_SIZE, [210, 230, 255, 0]);
    glass[0] = [210, 230, 255, 255];
    write_png(
        directory.path(),
        "textures/blocks/glass",
        TILE_SIZE,
        TILE_SIZE,
        &glass,
    );
    for path in [
        "textures/blocks/tinted_cube",
        "textures/blocks/grass_bottom",
        "textures/blocks/grass_side",
        "textures/blocks/grass_top",
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [80, 160, 60, 255]),
        );
    }
    let records = [
        record(0, 300, "minecraft:stone", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 301, "minecraft:glass", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            302,
            "minecraft:tinted_cube",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            3,
            303,
            "minecraft:grass_block",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    for deferred in 1..=2 {
        assert_eq!(
            compiled.visuals[deferred].faces, [DIAGNOSTIC_MATERIAL; 6],
            "deferred transparent/tinted record {deferred} must fail closed"
        );
    }
    let grass = compiled.visuals[3];
    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 3, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 3, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_eq!(
            material_for_face(&compiled, 3, face).flags,
            MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
        );
    }
}

#[test]
fn compiler_output_is_identical_across_shuffled_sources_and_records() {
    fn fixture(blocks: &str, terrain: &str) -> TempDir {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(directory.path(), blocks, terrain, "[]");
        write_png(
            directory.path(),
            "textures/blocks/a",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 255]),
        );
        write_png(
            directory.path(),
            "textures/blocks/z",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [220, 210, 200, 255]),
        );
        directory
    }

    let first = fixture(
        r#"{"alpha":{"textures":"alpha"},"zeta":{"textures":"zeta"}}"#,
        r#"{"texture_data":{"alpha":{"textures":"textures/blocks/a"},"zeta":{"textures":"textures/blocks/z"}}}"#,
    );
    let second = fixture(
        r#"{"zeta":{"textures":"zeta"},"alpha":{"textures":"alpha"}}"#,
        r#"{"texture_data":{"zeta":{"textures":"textures/blocks/z"},"alpha":{"textures":"textures/blocks/a"}}}"#,
    );
    let alpha = record(
        0,
        0xffff_fff0,
        "minecraft:alpha",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );
    let zeta = record(
        1,
        0x8000_0001,
        "minecraft:zeta",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );

    let first = compile_pack(first.path(), &[alpha.clone(), zeta.clone()]).expect("first compile");
    let second = compile_pack(second.path(), &[zeta, alpha]).expect("second compile");

    assert_eq!(first, second);
    assert_eq!(
        encode_blob(&first).expect("encode first"),
        encode_blob(&second).expect("encode second")
    );
}

#[test]
fn compiler_selects_huge_mushroom_face_variants_and_keeps_other_arrays_at_zero() {
    let directory = tempfile::tempdir().expect("create fixture");
    let families = [
        ("brown_mushroom_block", "mushroom_brown"),
        ("red_mushroom_block", "mushroom_red"),
        ("mushroom_stem", "mushroom_stem"),
    ];
    let faces = [
        (BlockFace::West, "west", "west"),
        (BlockFace::East, "east", "east"),
        (BlockFace::Down, "down", "bottom"),
        (BlockFace::Up, "up", "top"),
        (BlockFace::North, "north", "north"),
        (BlockFace::South, "south", "south"),
    ];
    let colour = |family: usize, face: usize, bits: u8| {
        let discriminator = 1 + family as u8 * 36 + face as u8 * 2 + u8::from(bits == 15);
        [discriminator, 255 - discriminator, bits, 255]
    };
    let is_static_stem_face = |family: usize, face: BlockFace| {
        family == 2
            && matches!(
                face,
                BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South
            )
    };

    let mut block_entries = serde_json::Map::new();
    let mut terrain_entries = serde_json::Map::new();
    for (family_index, (block_name, texture_prefix)) in families.iter().enumerate() {
        let mut face_entries = serde_json::Map::new();
        for (face_index, (face, block_face, texture_suffix)) in faces.iter().enumerate() {
            let key = format!("{texture_prefix}_{texture_suffix}");
            face_entries.insert((*block_face).into(), serde_json::Value::String(key.clone()));
            let selected_bits: &[u8] = if is_static_stem_face(family_index, *face) {
                terrain_entries.insert(
                    key,
                    serde_json::json!({
                        "textures": format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_static"
                        )
                    }),
                );
                &[0]
            } else {
                let variants = (0..16)
                    .map(|bits| {
                        serde_json::Value::String(format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_{bits}"
                        ))
                    })
                    .collect::<Vec<_>>();
                terrain_entries.insert(key, serde_json::json!({ "textures": variants }));
                &[0, 15]
            };

            for &bits in selected_bits {
                let source = if is_static_stem_face(family_index, *face) {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_static")
                } else {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_{bits}")
                };
                write_png(
                    directory.path(),
                    &source,
                    TILE_SIZE,
                    TILE_SIZE,
                    &solid(TILE_SIZE, TILE_SIZE, colour(family_index, face_index, bits)),
                );
            }
        }
        block_entries.insert(
            (*block_name).into(),
            serde_json::json!({ "textures": face_entries }),
        );
    }

    let unrelated_variants = (0..16)
        .map(|bits| serde_json::Value::String(format!("textures/blocks/unrelated_{bits}")))
        .collect::<Vec<_>>();
    block_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": "unrelated" }),
    );
    terrain_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": unrelated_variants }),
    );
    write_png(
        directory.path(),
        "textures/blocks/unrelated_0",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [240, 120, 60, 255]),
    );

    write_pack(
        directory.path(),
        &serde_json::Value::Object(block_entries).to_string(),
        &serde_json::json!({ "texture_data": terrain_entries }).to_string(),
        "[]",
    );

    let mut records = Vec::new();
    for (family_index, (block_name, _)) in families.iter().enumerate() {
        for (state_index, bits) in [0_u8, 15].into_iter().enumerate() {
            let sequential_id = (family_index * 2 + state_index) as u32;
            records.push(record(
                sequential_id,
                0x8000_1000 + sequential_id,
                &format!("minecraft:{block_name}"),
                &format!(r#"{{"huge_mushroom_bits":{bits}}}"#),
                BlockFlags::CUBE_GEOMETRY,
            ));
        }
    }
    let fallback_states = [
        "{}",
        "null",
        "not JSON",
        r#"{"huge_mushroom_bits":-1}"#,
        r#"{"huge_mushroom_bits":16}"#,
        r#"{"huge_mushroom_bits":"15"}"#,
    ];
    for state in fallback_states {
        let sequential_id = records.len() as u32;
        records.push(record(
            sequential_id,
            0x8000_1000 + sequential_id,
            "minecraft:brown_mushroom_block",
            state,
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    let invalid_stem_id = records.len() as u32;
    records.push(record(
        invalid_stem_id,
        0x8000_1000 + invalid_stem_id,
        "minecraft:mushroom_stem",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    ));
    let unrelated_id = records.len() as u32;
    records.push(record(
        unrelated_id,
        0x8000_1000 + unrelated_id,
        "minecraft:unrelated",
        r#"{"huge_mushroom_bits":15}"#,
        BlockFlags::CUBE_GEOMETRY,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile mushroom variants");
    for (family_index, _) in families.iter().enumerate() {
        for (state_index, bits) in [0_u8, 15].into_iter().enumerate() {
            let sequential_id = family_index * 2 + state_index;
            for (face_index, (face, _, _)) in faces.iter().enumerate() {
                let material = material_for_face(&compiled, sequential_id, *face);
                let expected_bits = if is_static_stem_face(family_index, *face) {
                    0
                } else {
                    bits
                };
                assert_eq!(
                    mip_pixel(&compiled, 0, material.layer, 0, 0),
                    colour(family_index, face_index, expected_bits),
                    "wrong {bits} texture for {} {face:?}",
                    families[family_index].0
                );
            }
        }
    }
    for sequential_id in 6..6 + fallback_states.len() {
        assert_eq!(
            compiled.visuals[sequential_id].faces, [0; 6],
            "invalid or absent mushroom selector must fail closed"
        );
    }
    assert_eq!(
        compiled.visuals[invalid_stem_id as usize].faces, [0; 6],
        "invalid selector must also fail closed for static mushroom face paths"
    );
    let unrelated = material_for_face(&compiled, unrelated_id as usize, BlockFace::Up);
    assert_eq!(
        mip_pixel(&compiled, 0, unrelated.layer, 0, 0),
        [240, 120, 60, 255],
        "unrelated terrain arrays must retain variant-zero selection"
    );

    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed records");
    assert_eq!(
        encode_blob(&compiled).expect("encode mushroom variants"),
        encode_blob(&reversed).expect("encode reversed mushroom variants")
    );
}

#[test]
fn compiler_fails_closed_for_noncanonical_mushroom_variant_counts() {
    let directory = tempfile::tempdir().expect("create fixture");
    let variants = (0..15)
        .map(|bits| format!("textures/blocks/mushroom_brown_top_{bits}"))
        .collect::<Vec<_>>();
    write_pack(
        directory.path(),
        r#"{"brown_mushroom_block":{"textures":"mushroom_brown_top"}}"#,
        &serde_json::json!({
            "texture_data": {
                "mushroom_brown_top": { "textures": variants }
            }
        })
        .to_string(),
        "[]",
    );
    let records = [record(
        0,
        0x8000_2000,
        "minecraft:brown_mushroom_block",
        r#"{"huge_mushroom_bits":14}"#,
        BlockFlags::CUBE_GEOMETRY,
    )];

    let compiled = compile_pack(directory.path(), &records)
        .expect("a malformed mushroom variant table must fail closed without loading a texture");

    assert_eq!(compiled.visuals[0].faces, [0; 6]);
    assert_eq!(compiled.materials.len(), 1);
    assert_eq!(compiled.textures.layers, 1);
}

#[test]
fn compiler_only_loads_full_cubes_and_builds_equivalent_lookup_tables() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "flower": {"textures": "missing_flower"},
            "stone": {"textures": "stone"}
        }"#,
        r#"{"texture_data": {
            "missing_flower": {"textures": "textures/blocks/not_present"},
            "stone": {"textures": "textures/blocks/stone"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let records = [
        record(
            5,
            0xf000_0005,
            "minecraft:stone",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x8000_0002,
            "minecraft:flower",
            "{}",
            BlockFlags::empty(),
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile full cubes only");

    assert_eq!(compiled.visuals.len(), 6);
    assert_eq!(compiled.visuals[2].faces, [0; 6]);
    assert!(
        compiled.visuals[5]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    assert_eq!(&*compiled.hashed, &[(0x8000_0002, 2), (0xf000_0005, 5)]);
    for &(hash, visual_index) in compiled.hashed.iter() {
        let record = records
            .iter()
            .find(|record| record.network_hash == hash)
            .expect("hash record");
        assert_eq!(visual_index, record.sequential_id);
        assert_eq!(
            compiled.visuals[visual_index as usize],
            compiled.visuals[record.sequential_id as usize]
        );
    }
}

#[test]
fn compiler_compiles_grass_faces_but_keeps_flipbooks_and_unlisted_blocks_diagnostic() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }},
            "seaLantern": {"textures": "sea_lantern"}
        }"#,
        r#"{"texture_data": {
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"},
            "sea_lantern": {"textures": "textures/blocks/sea_lantern"}
        }}"#,
        r#"[{
            "flipbook_texture": "textures/blocks/sea_lantern",
            "atlas_tile": "sea_lantern",
            "ticks_per_frame": 5
        }]"#,
    );
    for (path, colour) in [
        ("textures/blocks/grass_bottom", [80, 50, 20, 255]),
        ("textures/blocks/grass_top", [70, 190, 50, 255]),
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    write_tga(
        directory.path(),
        "textures/blocks/grass_side",
        TILE_SIZE as u16,
        TILE_SIZE as u16,
        &solid(TILE_SIZE, TILE_SIZE, [100, 150, 60, 255]),
    );
    let records = [
        record(
            0,
            0xde31_28b4,
            "minecraft:grass_block",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            1,
            0x1111_1111,
            "minecraft:sea_lantern",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x2222_2222,
            "minecraft:invisible_bedrock",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile deferred visuals");
    let grass = compiled.visuals[0];
    let sea_lantern = compiled.visuals[1];

    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 0, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::North).flags,
        MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
    );
    assert_eq!(sea_lantern.faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.materials.len(), 4);
    assert_eq!(compiled.textures.layers, 4);
}

#[test]
fn compiler_maps_recognized_flipbooks_to_diagnostic_without_loading_the_strip() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"water":{"textures":"water"}}"#,
        r#"{"texture_data":{"water":{"textures":"textures/blocks/water"}}}"#,
        r#"[{"flipbook_texture":"textures/blocks/water","atlas_tile":"water"}]"#,
    );
    let records = [record(
        0,
        700,
        "minecraft:water",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    )];

    let compiled = compile_pack(directory.path(), &records).expect("compile flipbook reference");

    assert_eq!(compiled.visuals[0].faces, [0; 6]);
    assert_eq!(compiled.materials.len(), 1);
    assert_eq!(compiled.textures.layers, 1);
}

fn one_texture_fixture() -> (TempDir, RegistryRecord, PathBuf) {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken"}}}"#,
        "[]",
    );
    let expected = directory.path().join("textures/blocks/broken.png");
    (
        directory,
        record(0, 900, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY),
        expected,
    )
}

fn assert_source_context(error: &AssetError, expected_path: &Path) {
    let rendered = error.to_string();
    assert!(
        rendered.contains("broken_key"),
        "missing source key: {rendered}"
    );
    assert!(
        rendered.contains(expected_path.to_string_lossy().as_ref()),
        "missing source path: {rendered}"
    );
}

#[test]
fn compiler_reports_missing_malformed_and_wrong_size_png_sources() {
    let (missing_root, missing_record, missing_path) = one_texture_fixture();
    let error = compile_pack(missing_root.path(), &[missing_record]).expect_err("missing PNG");
    assert!(matches!(error, AssetError::TextureIo { .. }));
    assert_source_context(&error, &missing_path);

    let (malformed_root, malformed_record, malformed_path) = one_texture_fixture();
    write_file(&malformed_path, b"not a png");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed PNG");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = one_texture_fixture();
    write_file(&sized_path, png_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong PNG size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_reports_malformed_and_wrong_size_tga_with_source_context() {
    let fixture = || {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(
            directory.path(),
            r#"{"broken":{"textures":"broken_key"}}"#,
            r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.tga"}}}"#,
            "[]",
        );
        let path = directory.path().join("textures/blocks/broken.tga");
        let record = record(0, 901, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);
        (directory, record, path)
    };

    let (malformed_root, malformed_record, malformed_path) = fixture();
    write_file(&malformed_path, b"not a tga");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed TGA");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = fixture();
    write_file(&sized_path, tga_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong TGA size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_rejects_unsupported_static_texture_formats_with_source_context() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.jpg"}}}"#,
        "[]",
    );
    let path = directory.path().join("textures/blocks/broken.jpg");
    write_file(&path, b"not a supported static texture");
    let record = record(0, 902, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);

    let error = compile_pack(directory.path(), &[record]).expect_err("unsupported texture format");
    let rendered = error.to_string();
    assert!(rendered.contains("unsupported"), "{rendered}");
    assert!(rendered.contains("broken_key"), "{rendered}");
    assert!(
        rendered.contains(path.to_string_lossy().as_ref()),
        "{rendered}"
    );
}

#[test]
fn compiler_rejects_more_than_the_bounded_layer_count_with_source_context() {
    let directory = tempfile::tempdir().expect("create fixture");
    let mut blocks = String::from("{");
    let mut terrain = String::from(r#"{"texture_data":{"#);
    let mut records = Vec::with_capacity(MAX_TEXTURE_LAYERS);
    for index in 0..MAX_TEXTURE_LAYERS {
        if index != 0 {
            blocks.push(',');
            terrain.push(',');
        }
        write!(blocks, r#""block_{index}":{{"textures":"key_{index}"}}"#)
            .expect("append block JSON");
        write!(
            terrain,
            r#""key_{index}":{{"textures":"textures/generated/layer_{index}"}}"#
        )
        .expect("append terrain JSON");
        let color = [index as u8, (index >> 8) as u8, (index >> 16) as u8, 255];
        write_png(
            directory.path(),
            &format!("textures/generated/layer_{index}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, color),
        );
        records.push(record(
            index as u32,
            0x8000_0000 + index as u32,
            &format!("minecraft:block_{index}"),
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    blocks.push('}');
    terrain.push_str("}}");
    write_pack(directory.path(), &blocks, &terrain, "[]");

    let error = compile_pack(directory.path(), &records).expect_err("layer bound");

    match error {
        AssetError::TooManyTextureLayers {
            count,
            max,
            key: Some(key),
            path: Some(path),
        } => {
            assert_eq!(count, MAX_TEXTURE_LAYERS + 1);
            assert_eq!(max, MAX_TEXTURE_LAYERS);
            assert!(key.starts_with("key_"));
            assert!(path.to_string_lossy().ends_with(".png"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
