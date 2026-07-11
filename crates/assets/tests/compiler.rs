use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assets::{
    AssetError, BlockFace, BlockFlags, CompiledAssets, DIAGNOSTIC_MATERIAL,
    MATERIAL_FLAG_ROTATE_UV, MAX_TEXTURE_LAYERS, Material, RegistryRecord, compile_pack,
    encode_blob,
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
        record(0, 100, "minecraft:same_a", "{}", BlockFlags::FULL_CUBE),
        record(1, 101, "minecraft:same_b", "{}", BlockFlags::FULL_CUBE),
        record(
            2,
            102,
            "minecraft:pillar",
            r#"{"pillar_axis":"x"}"#,
            BlockFlags::FULL_CUBE,
        ),
        record(3, 103, "minecraft:blue", "{}", BlockFlags::FULL_CUBE),
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
        record(0, 200, "minecraft:red", "{}", BlockFlags::FULL_CUBE),
        record(1, 201, "minecraft:blue", "{}", BlockFlags::FULL_CUBE),
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
        record(0, 300, "minecraft:stone", "{}", BlockFlags::FULL_CUBE),
        record(1, 301, "minecraft:glass", "{}", BlockFlags::FULL_CUBE),
        record(2, 302, "minecraft:tinted_cube", "{}", BlockFlags::FULL_CUBE),
        record(3, 303, "minecraft:grass_block", "{}", BlockFlags::FULL_CUBE),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    for deferred in 1..=3 {
        assert_eq!(
            compiled.visuals[deferred].faces, [DIAGNOSTIC_MATERIAL; 6],
            "deferred transparent/tinted record {deferred} must fail closed"
        );
    }
    assert_eq!(compiled.materials.len(), 2, "diagnostic + opaque stone");
    assert_eq!(
        compiled.textures.layers, 2,
        "deferred sources stay out of the blob"
    );
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
        BlockFlags::FULL_CUBE,
    );
    let zeta = record(
        1,
        0x8000_0001,
        "minecraft:zeta",
        "{}",
        BlockFlags::FULL_CUBE,
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
                BlockFlags::FULL_CUBE,
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
            BlockFlags::FULL_CUBE,
        ));
    }
    let invalid_stem_id = records.len() as u32;
    records.push(record(
        invalid_stem_id,
        0x8000_1000 + invalid_stem_id,
        "minecraft:mushroom_stem",
        "{}",
        BlockFlags::FULL_CUBE,
    ));
    let unrelated_id = records.len() as u32;
    records.push(record(
        unrelated_id,
        0x8000_1000 + unrelated_id,
        "minecraft:unrelated",
        r#"{"huge_mushroom_bits":15}"#,
        BlockFlags::FULL_CUBE,
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
        BlockFlags::FULL_CUBE,
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
            BlockFlags::FULL_CUBE,
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
fn compiler_keeps_tinted_grass_flipbooks_and_unlisted_blocks_diagnostic() {
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
            BlockFlags::FULL_CUBE,
        ),
        record(
            1,
            0x1111_1111,
            "minecraft:sea_lantern",
            "null",
            BlockFlags::FULL_CUBE,
        ),
        record(
            2,
            0x2222_2222,
            "minecraft:invisible_bedrock",
            "null",
            BlockFlags::FULL_CUBE,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile deferred visuals");
    let grass = compiled.visuals[0];
    let sea_lantern = compiled.visuals[1];

    assert_eq!(grass.faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(sea_lantern.faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.materials.len(), 1);
    assert_eq!(compiled.textures.layers, 1);
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
        BlockFlags::FULL_CUBE,
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
        record(0, 900, "minecraft:broken", "{}", BlockFlags::FULL_CUBE),
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
        let record = record(0, 901, "minecraft:broken", "{}", BlockFlags::FULL_CUBE);
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
    let record = record(0, 902, "minecraft:broken", "{}", BlockFlags::FULL_CUBE);

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
            BlockFlags::FULL_CUBE,
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
