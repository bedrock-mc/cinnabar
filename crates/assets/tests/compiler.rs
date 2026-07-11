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
            "blue": {"textures": "blue"},
            "alpha": {"textures": "alpha"}
        }"#,
        r#"{"texture_data": {
            "red": {"textures": "textures/blocks/a_red"},
            "blue": {"textures": "textures/blocks/b_blue"},
            "alpha": {"textures": "textures/blocks/c_alpha"}
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
    let mut alpha = solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 0]);
    alpha[0] = [255, 0, 0, 255];
    write_png(
        directory.path(),
        "textures/blocks/c_alpha",
        TILE_SIZE,
        TILE_SIZE,
        &alpha,
    );
    let records = [
        record(0, 200, "minecraft:red", "{}", BlockFlags::FULL_CUBE),
        record(1, 201, "minecraft:blue", "{}", BlockFlags::FULL_CUBE),
        record(2, 202, "minecraft:alpha", "{}", BlockFlags::FULL_CUBE),
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
    let alpha_layer = material_for_face(&compiled, 2, BlockFace::Up).layer;
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
    assert_eq!(
        mip_pixel(&compiled, 1, alpha_layer, 0, 0),
        [255, 0, 0, 64],
        "transparent blue must not bleed into the premultiplied-alpha result"
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
fn compiler_materializes_grass_alias_and_keeps_flipbooks_and_unlisted_blocks_diagnostic() {
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

    let compiled = compile_pack(directory.path(), &records).expect("compile legacy aliases");
    let grass = compiled.visuals[0];
    let sea_lantern = compiled.visuals[1];

    assert_ne!(grass.faces[BlockFace::Down as usize], DIAGNOSTIC_MATERIAL);
    assert_ne!(grass.faces[BlockFace::Up as usize], DIAGNOSTIC_MATERIAL);
    assert_ne!(grass.faces[BlockFace::West as usize], DIAGNOSTIC_MATERIAL);
    assert_ne!(
        grass.faces[BlockFace::Down as usize],
        grass.faces[BlockFace::Up as usize]
    );
    assert_eq!(
        grass.faces[BlockFace::West as usize],
        grass.faces[BlockFace::East as usize]
    );
    assert_eq!(
        grass.faces[BlockFace::West as usize],
        grass.faces[BlockFace::North as usize]
    );
    assert_eq!(
        grass.faces[BlockFace::West as usize],
        grass.faces[BlockFace::South as usize]
    );
    let bottom = material_for_face(&compiled, 0, BlockFace::Down);
    let side = material_for_face(&compiled, 0, BlockFace::West);
    assert_eq!(
        mip_pixel(&compiled, 0, bottom.layer, 0, 0),
        [80, 50, 20, 255]
    );
    assert_eq!(
        mip_pixel(&compiled, 0, side.layer, 0, 0),
        [100, 150, 60, 255]
    );
    assert_eq!(sea_lantern.faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
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
