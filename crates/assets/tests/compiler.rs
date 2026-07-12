use std::{
    collections::HashSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assets::{
    AssetError, BlockFace, BlockFlags, CollisionSeed, CompiledAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK,
    MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_OVERLAY_MASK,
    MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK,
    MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK, MAX_TEXTURE_LAYERS, MODEL_QUAD_FLAG_TWO_SIDED,
    MODEL_TEMPLATE_FLAG_KELP, Material, ModelFamily, ModelState, ModelStateField, NetworkIdMode,
    RegistryProvenance, RegistryRecord, RuntimeAssets, VisualKind, compile_pack, encode_blob,
    read_registry,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const TILE_SIZE: u32 = 16;

#[test]
fn flowerbed_generated_registry_has_exact_canonical_state_matrix() {
    let bytes = if let Ok(revision) = std::env::var("FLOWERBED_REGISTRY_GIT_REV") {
        let output = Command::new("git")
            .args([
                "show",
                &format!("{revision}:crates/assets/data/block-registry-v1001.bin"),
            ])
            .output()
            .expect("read requested registry revision");
        assert!(output.status.success(), "git show failed: {output:?}");
        output.stdout
    } else {
        include_bytes!("../data/block-registry-v1001.bin").to_vec()
    };
    let records = read_registry(&bytes).expect("decode committed generated registry");

    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 32, "{name} record count");

        let mut growths = [false; 8];
        let mut orientations = [false; 4];
        let mut selectors = HashSet::with_capacity(32);
        let mut canonical_states = HashSet::with_capacity(32);
        for record in selected {
            assert_eq!(record.model_family as u8, 31, "{name} raw family");
            assert_ne!(record.model_family, ModelFamily::Cross, "{name} is Cross");
            assert_ne!(
                record.model_family,
                ModelFamily::Unknown,
                "{name} is Unknown"
            );
            let growth = record
                .model_state
                .get(ModelStateField::Growth)
                .expect("flowerbed growth") as usize;
            let orientation = record
                .model_state
                .get(ModelStateField::Orientation)
                .expect("flowerbed orientation") as usize;
            assert!(
                growth < growths.len(),
                "{name} growth {growth} out of range"
            );
            assert!(
                orientation < orientations.len(),
                "{name} orientation {orientation} out of range"
            );
            growths[growth] = true;
            orientations[orientation] = true;
            assert!(
                selectors.insert((growth, orientation)),
                "{name} duplicate growth/orientation pair {growth}/{orientation}"
            );
            assert!(
                canonical_states.insert(record.canonical_state.as_ref()),
                "{name} duplicate canonical state"
            );
        }

        assert!(
            growths.into_iter().all(|present| present),
            "{name} growth coverage"
        );
        assert!(
            orientations.into_iter().all(|present| present),
            "{name} orientation coverage"
        );
        assert_eq!(selectors.len(), 32, "{name} selector uniqueness");
        assert_eq!(
            canonical_states.len(),
            32,
            "{name} canonical-state uniqueness"
        );
    }
}

#[test]
fn assetc_root_help_documents_all_compile_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .arg("--help")
        .output()
        .expect("run assetc help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    for required in [
        "compile",
        "animation-inventory",
        "--pack",
        "--registry",
        "--out",
    ] {
        assert!(help.contains(required), "help omitted {required}:\n{help}");
    }
}

#[test]
fn assetc_animation_inventory_records_the_full_deterministic_contract() {
    let directory = tempfile::tempdir().expect("create CLI inventory fixture");
    let pack = directory.path().join("resource pack");
    write_pack(
        &pack,
        "{}",
        r#"{"texture_data":{
            "still":{"textures":"textures/blocks/still"},
            "animated":{"textures":"textures/blocks/animated"}
        }}"#,
        r#"[{
            "flipbook_texture":"textures/blocks/animated",
            "atlas_tile":"animated",
            "ticks_per_frame":2,
            "blend_frames":true
        }]"#,
    );
    write_png(
        &pack,
        "textures/blocks/still",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [5, 10, 15, 255]),
    );
    let mut animation = solid(TILE_SIZE, TILE_SIZE, [20, 30, 40, 255]);
    animation.extend(solid(TILE_SIZE, TILE_SIZE, [50, 60, 70, 255]));
    write_png(
        &pack,
        "textures/blocks/animated",
        TILE_SIZE,
        TILE_SIZE * 2,
        &animation,
    );
    let manifest = directory.path().join("vanilla-source.json");
    let manifest_bytes = br#"{"schema":1,"commit":"synthetic"}
"#;
    write_file(&manifest, manifest_bytes);
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");

    for out in [&first, &second] {
        let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
            .args([
                "animation-inventory",
                "--pack",
                pack.to_str().expect("UTF-8 pack path"),
                "--source-manifest",
                manifest.to_str().expect("UTF-8 manifest path"),
                "--max-layers-per-page",
                "3",
                "--max-pages",
                "2",
                "--out",
                out.to_str().expect("UTF-8 output path"),
            ])
            .output()
            .expect("run animation inventory CLI");
        assert!(
            output.status.success(),
            "assetc failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first_bytes = fs::read(&first).expect("read first report");
    let second_bytes = fs::read(&second).expect("read second report");
    assert_eq!(
        first_bytes, second_bytes,
        "report bytes must be deterministic"
    );
    assert_eq!(first_bytes.last(), Some(&b'\n'));
    let report: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("parse inventory report");
    let manifest_sha = format!("{:x}", Sha256::digest(manifest_bytes));
    let canonical_pack = fs::canonicalize(&pack).expect("canonical pack path");

    assert_eq!(report["schema"], 1);
    assert_eq!(report["source_manifest_sha256"], manifest_sha);
    assert_eq!(
        report["canonical_pack_path"],
        canonical_pack.to_string_lossy().as_ref()
    );
    assert_eq!(report["limits"]["max_layers_per_page"], 3);
    assert_eq!(report["limits"]["max_pages"], 2);
    assert_eq!(report["inventory"]["static_sources"], 1);
    assert_eq!(report["inventory"]["reachable_animations"], 1);
    assert_eq!(report["inventory"]["physical_animation_frames"], 2);
    assert_eq!(report["inventory"]["deduplicated_layers"], 4);
    assert_eq!(
        report["inventory"]["page_layers"],
        serde_json::json!([3, 1])
    );
}

#[test]
fn compile_pack_installs_flipbook_pages_frames_and_material_animation() {
    let directory = tempfile::tempdir().expect("create runtime animation fixture");
    write_pack(
        directory.path(),
        r#"{"animated_block":{"textures":"animated"}}"#,
        r#"{"texture_data":{"animated":{"textures":"textures/blocks/animated"}}}"#,
        r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated","ticks_per_frame":3,"blend_frames":true}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [40, 50, 60, 255]));
    write_png(
        directory.path(),
        "textures/blocks/animated",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            1,
            "minecraft:animated_block",
            "minecraft:animated_block[]",
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        )],
    )
    .expect("compile flipbook into MCBEAS04 tables");

    assert_eq!(compiled.animations.len(), 1);
    assert_eq!(compiled.animations[0].frame_count, 2);
    assert_eq!(compiled.animations[0].ticks_per_frame, 3);
    assert_eq!(compiled.animation_frames.len(), 2);
    assert_ne!(compiled.animation_frames[0], compiled.animation_frames[1]);
    assert_eq!(compiled.materials.len(), 2);
    assert_eq!(compiled.materials[1].animation, 0);
    assert_eq!(compiled.materials[1].texture, compiled.animation_frames[0]);
    assert_eq!(compiled.texture_pages.len(), 1);
    let runtime = assets::RuntimeAssets::decode(&encode_blob(&compiled).unwrap())
        .expect("decode installed animation tables");
    assert_eq!(runtime.animations(), compiled.animations.as_ref());
    assert_eq!(
        runtime.animation_frames(),
        compiled.animation_frames.as_ref()
    );
}

#[test]
fn compile_pack_uses_compact_exact_flipbook_selector_and_preserves_metadata() {
    let directory = tempfile::tempdir().expect("create selector fixture");
    write_pack(
        directory.path(),
        r#"{"chosen_block":{"textures":"chosen"}}"#,
        r#"{"texture_data":{
            "unselected":{"textures":"textures/blocks/unselected"},
            "chosen":{"textures":"textures/blocks/chosen"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/unselected","atlas_tile":"unselected"},
            {"flipbook_texture":"textures/blocks/chosen","atlas_tile":"chosen","atlas_index":4,"atlas_tile_variant":5,"replicate":2},
            {"flipbook_texture":"textures/blocks/chosen","atlas_tile":"chosen","ticks_per_frame":7,"atlas_index":0,"atlas_tile_variant":0,"replicate":3}
        ]"#,
    );
    let mut chosen = solid(TILE_SIZE, TILE_SIZE, [1, 2, 3, 255]);
    chosen.extend(solid(TILE_SIZE, TILE_SIZE, [4, 5, 6, 255]));
    write_png(
        directory.path(),
        "textures/blocks/chosen",
        TILE_SIZE,
        TILE_SIZE * 2,
        &chosen,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            2,
            "minecraft:chosen_block",
            "minecraft:chosen_block[]",
            BlockFlags::CUBE_GEOMETRY,
        )],
    )
    .expect("compile selected flipbooks without loading unselected strip");

    assert_eq!(compiled.animations.len(), 2);
    assert_eq!(
        compiled.materials[1].animation, 1,
        "default selector is compact index one"
    );
    assert_eq!(compiled.animations[0].atlas_index, 4);
    assert_eq!(compiled.animations[0].atlas_tile_variant, 5);
    assert_eq!(compiled.animations[0].replicate, 2);
    assert_eq!(compiled.animations[1].atlas_index, 0);
    assert_eq!(compiled.animations[1].atlas_tile_variant, 0);
    assert_eq!(compiled.animations[1].replicate, 3);
    assert_eq!(compiled.animations[1].ticks_per_frame, 7);
}

#[test]
fn compile_pack_uses_first_strip_frame_for_non_flipbook_path_alias() {
    let directory = tempfile::tempdir().expect("create animated path alias fixture");
    write_pack(
        directory.path(),
        r#"{"flattened_prismarine":{"textures":"flattened_prismarine"}}"#,
        r#"{"texture_data":{
            "prismarine":{"textures":"textures/blocks/prismarine_rough"},
            "flattened_prismarine":{"textures":"textures/blocks/prismarine_rough"}
        }}"#,
        r#"[{"flipbook_texture":"textures/blocks/prismarine_rough","atlas_tile":"prismarine","ticks_per_frame":2}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [11, 22, 33, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [44, 55, 66, 255]));
    write_png(
        directory.path(),
        "textures/blocks/prismarine_rough",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            3,
            "minecraft:flattened_prismarine",
            "minecraft:flattened_prismarine[]",
            BlockFlags::CUBE_GEOMETRY,
        )],
    )
    .expect("compile alias without decoding the strip as a static 16x64 image");

    assert_eq!(compiled.animations.len(), 1, "strip remains compiled once");
    assert_eq!(compiled.materials.len(), 2);
    assert_eq!(compiled.materials[1].animation, assets::NO_ANIMATION);
    assert_eq!(compiled.materials[1].texture, compiled.animation_frames[0]);
    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material == 1)
    );
}

#[test]
fn compile_pack_keeps_static_and_animated_keys_distinct_on_one_strip_path() {
    let directory = tempfile::tempdir().expect("create shared strip-key fixture");
    write_pack(
        directory.path(),
        r#"{
            "static_block":{"textures":"a_static"},
            "animated_block":{"textures":"z_anim"}
        }"#,
        r#"{"texture_data":{
            "a_static":{"textures":"textures/blocks/shared_strip"},
            "z_anim":{"textures":"textures/blocks/shared_strip"}
        }}"#,
        r#"[{"flipbook_texture":"textures/blocks/shared_strip","atlas_tile":"z_anim"}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [1, 10, 100, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [2, 20, 200, 255]));
    write_png(
        directory.path(),
        "textures/blocks/shared_strip",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[
            record(
                0,
                4,
                "minecraft:static_block",
                "{}",
                BlockFlags::CUBE_GEOMETRY,
            ),
            record(
                1,
                5,
                "minecraft:animated_block",
                "{}",
                BlockFlags::CUBE_GEOMETRY,
            ),
        ],
    )
    .expect("compile distinct atlas-key semantics on one source strip");

    let static_material = compiled.visuals[0].faces[0] as usize;
    let animated_material = compiled.visuals[1].faces[0] as usize;
    assert_ne!(static_material, animated_material);
    assert_eq!(
        compiled.materials[static_material].animation,
        assets::NO_ANIMATION
    );
    assert_eq!(compiled.materials[animated_material].animation, 0);
    assert_eq!(
        compiled.materials[static_material].texture, compiled.materials[animated_material].texture,
        "both begin on the same deduplicated physical frame"
    );
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
    let model_family = if flags.contains(BlockFlags::AIR) {
        ModelFamily::Air
    } else if flags.contains(BlockFlags::LEAF_MODEL) {
        ModelFamily::Leaves
    } else if flags.contains(BlockFlags::CUBE_GEOMETRY) {
        ModelFamily::Cube
    } else {
        ModelFamily::Unknown
    };
    RegistryRecord {
        sequential_id,
        network_hash,
        name: name.into(),
        canonical_state: state.into(),
        flags,
        model_family,
        contributor_role: if flags.contains(BlockFlags::AIR) {
            ContributorRole::Air
        } else {
            ContributorRole::Primary
        },
        model_state: ModelState::default(),
        face_coverage: if flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
            0x3f
        } else {
            0
        },
        collision_seed: CollisionSeed::default(),
        provenance: RegistryProvenance::DRAGONFLY,
    }
}

fn model_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    state: &str,
    model_family: ModelFamily,
) -> RegistryRecord {
    let mut record = record(
        sequential_id,
        network_hash,
        name,
        state,
        BlockFlags::empty(),
    );
    record.model_family = model_family;
    record
}

#[test]
fn compiler_compiles_exact_terrestrial_cross_alias_tint_and_crop_variants() {
    let directory = tempfile::tempdir().expect("create terrestrial cross fixture");
    write_pack(
        directory.path(),
        r#"{
            "short_grass":{"textures":"short_grass"},
            "fern":{"textures":"fern"},
            "yellow_flower":{"textures":"yellow_flower"},
            "sapling":{"textures":"sapling"},
            "wheat":{"textures":"wheat"},
            "carrots":{"textures":"carrots"},
            "melon_stem":{"textures":"melon_stem"}
        }"#,
        r#"{"texture_data":{
            "short_grass":{"textures":"textures/blocks/short_grass"},
            "fern":{"textures":"textures/blocks/fern"},
            "yellow_flower":{"textures":"textures/blocks/dandelion"},
            "sapling":{"textures":["textures/blocks/oak","textures/blocks/spruce"]},
            "wheat":{"textures":[
                "textures/blocks/wheat0","textures/blocks/wheat1",
                "textures/blocks/wheat2","textures/blocks/wheat3",
                "textures/blocks/wheat4","textures/blocks/wheat5",
                "textures/blocks/wheat6","textures/blocks/wheat7"
            ]},
            "carrots":{"textures":[
                "textures/blocks/carrots0","textures/blocks/carrots1",
                "textures/blocks/carrots2","textures/blocks/carrots3"
            ]},
            "melon_stem":{"textures":[
                "textures/blocks/melon_disconnected","textures/blocks/melon_connected"
            ]}
        }}"#,
        "[]",
    );
    for (index, path) in [
        "short_grass",
        "fern",
        "dandelion",
        "oak",
        "spruce",
        "wheat0",
        "wheat1",
        "wheat2",
        "wheat3",
        "wheat4",
        "wheat5",
        "wheat6",
        "wheat7",
        "carrots0",
        "carrots1",
        "carrots2",
        "carrots3",
        "melon_disconnected",
        "melon_connected",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 17, 31, 255]),
        );
    }
    let records = [
        model_record(0, 100, "minecraft:short_grass", "{}", ModelFamily::Cross),
        model_record(1, 101, "minecraft:fern", "{}", ModelFamily::Cross),
        model_record(2, 102, "minecraft:dandelion", "{}", ModelFamily::Cross),
        model_record(
            3,
            103,
            "minecraft:oak_sapling",
            r#"{"age_bit":{"type":"byte","value":1}}"#,
            ModelFamily::Cross,
        ),
        model_record(
            4,
            104,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":0}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            5,
            105,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            6,
            106,
            "minecraft:carrots",
            r#"{"growth":{"type":"int","value":5}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            7,
            107,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":0},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            8,
            108,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":2},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile crossed plants");
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Cross && visual.model_template != assets::NO_MODEL_TEMPLATE
    }));
    assert_eq!(
        compiled
            .visuals
            .iter()
            .map(|visual| visual.variant)
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 7, 2, 0, 1]
    );
    for (index, visual) in compiled.visuals.iter().enumerate() {
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(
            template.quad_count, 2,
            "visual {index} did not use one crossed pair"
        );
        assert!(template.quad_count <= 32);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert!(
            quads
                .iter()
                .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
        );
        assert!(quads.iter().all(|quad| {
            compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
        }));
    }
    for index in [0_usize, 1] {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            MATERIAL_FLAG_GRASS_TINT,
            "grass and fern must use the biome grass tint class"
        );
    }
    for index in 2..compiled.visuals.len() {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            0
        );
    }
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[5].model_template
    );
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[6].model_template
    );
}

#[test]
fn compiler_compiles_exact_animated_seagrass_pairs_without_biome_tint() {
    let directory = tempfile::tempdir().expect("create seagrass fixture");
    write_pack(
        directory.path(),
        r#"{
            "seagrass":{"textures":{
                "up":"seagrass_short",
                "down":"seagrass_tall_bot_a",
                "south":"seagrass_tall_bot_b",
                "east":"seagrass_tall_top_a",
                "west":"seagrass_tall_top_b"
            }}
        }"#,
        r#"{"texture_data":{
            "seagrass_short":{"textures":"textures/blocks/seagrass"},
            "seagrass_tall_bot_a":{"textures":"textures/blocks/seagrass_bottom_a"},
            "seagrass_tall_bot_b":{"textures":"textures/blocks/seagrass_bottom_b"},
            "seagrass_tall_top_a":{"textures":"textures/blocks/seagrass_top_a"},
            "seagrass_tall_top_b":{"textures":"textures/blocks/seagrass_top_b"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/seagrass","atlas_tile":"seagrass_short","ticks_per_frame":4},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_a","atlas_tile":"seagrass_tall_bot_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_b","atlas_tile":"seagrass_tall_bot_b","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_a","atlas_tile":"seagrass_tall_top_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_b","atlas_tile":"seagrass_tall_top_b","ticks_per_frame":3}
        ]"#,
    );
    for (index, path) in [
        "seagrass",
        "seagrass_bottom_a",
        "seagrass_bottom_b",
        "seagrass_top_a",
        "seagrass_top_b",
    ]
    .into_iter()
    .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 40, 80, 255]);
        strip.extend(solid(TILE_SIZE, TILE_SIZE, [index as u8 + 11, 50, 90, 255]));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }
    let records = [
        model_record(
            0,
            200,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"default"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            1,
            201,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_bot"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            2,
            202,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_top"}}"#,
            ModelFamily::Aquatic,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile animated seagrass");
    assert_eq!(compiled.visuals.len(), 3);
    let expected_ticks = [[4, 4], [3, 3], [3, 3]];
    for (index, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(visual.kind, VisualKind::Cross);
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, 2);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for (quad, ticks) in quads.iter().zip(expected_ticks[index]) {
            assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
            let material = compiled.materials[quad.material as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            assert_eq!(
                compiled.animations[material.animation as usize].ticks_per_frame,
                ticks
            );
        }
        assert!(visual.flags.is_empty());
    }
    let materials_for = |index: usize| {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
            .iter()
            .map(|quad| quad.material)
            .collect::<Vec<_>>()
    };
    let short = materials_for(0);
    assert_eq!(short[0], short[1]);
    assert_ne!(materials_for(1)[0], materials_for(1)[1]);
    assert_ne!(materials_for(2)[0], materials_for(2)[1]);
}

#[test]
fn compiler_compiles_all_kelp_ages_as_six_animated_body_and_head_faces() {
    let directory = tempfile::tempdir().expect("create kelp fixture");
    write_pack(
        directory.path(),
        r#"{"kelp":{"textures":{
            "down":"kelp_d","east":"kelp_top","north":"kelp_a",
            "south":"kelp_b","up":"kelp_c","west":"kelp_top_bulb"
        }}}"#,
        r#"{"texture_data":{
            "kelp_a":{"textures":"textures/blocks/kelp_a"},
            "kelp_b":{"textures":"textures/blocks/kelp_b"},
            "kelp_c":{"textures":"textures/blocks/kelp_c"},
            "kelp_d":{"textures":"textures/blocks/kelp_d"},
            "kelp_top":{"textures":"textures/blocks/kelp_top"},
            "kelp_top_bulb":{"textures":"textures/blocks/kelp_top_bulb"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/kelp_a","atlas_tile":"kelp_a","ticks_per_frame":4,"frames":[0,1,2,3,4,5]},
            {"flipbook_texture":"textures/blocks/kelp_b","atlas_tile":"kelp_b","ticks_per_frame":4,"frames":[1,2,3,4,5,0]},
            {"flipbook_texture":"textures/blocks/kelp_c","atlas_tile":"kelp_c","ticks_per_frame":4,"frames":[2,3,4,5,0,1]},
            {"flipbook_texture":"textures/blocks/kelp_d","atlas_tile":"kelp_d","ticks_per_frame":4,"frames":[3,4,5,0,1,2]},
            {"flipbook_texture":"textures/blocks/kelp_top","atlas_tile":"kelp_top","ticks_per_frame":4,"frames":[4,5,0,1,2,3]},
            {"flipbook_texture":"textures/blocks/kelp_top_bulb","atlas_tile":"kelp_top_bulb","ticks_per_frame":4,"frames":[5,0,1,2,3,4]}
        ]"#,
    );
    for (texture_index, path) in [
        "kelp_a",
        "kelp_b",
        "kelp_c",
        "kelp_d",
        "kelp_top",
        "kelp_top_bulb",
    ]
    .into_iter()
    .enumerate()
    {
        let strip = (0..6)
            .flat_map(|frame| {
                solid(
                    TILE_SIZE,
                    TILE_SIZE,
                    [texture_index as u8 + 1, frame as u8 + 20, 90, 255],
                )
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 6,
            &strip,
        );
    }
    let records = (0..26)
        .map(|age| {
            model_record(
                age,
                300 + age,
                "minecraft:kelp",
                &format!(r#"{{"kelp_age":{{"type":"int","value":{age}}}}}"#),
                ModelFamily::Aquatic,
            )
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile all kelp ages");
    assert_eq!(compiled.visuals.len(), 26);
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Model
            && visual.model_template == compiled.visuals[0].model_template
            && visual.flags.is_empty()
    }));
    let template = compiled.model_templates[compiled.visuals[0].model_template as usize];
    assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_KELP);
    assert_eq!(template.quad_count, 6);
    let quads = &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
    assert_eq!(
        quads.iter().map(|quad| quad.material).collect::<Vec<_>>(),
        [
            compiled.visuals[0].faces[BlockFace::North as usize],
            compiled.visuals[0].faces[BlockFace::South as usize],
            compiled.visuals[0].faces[BlockFace::Up as usize],
            compiled.visuals[0].faces[BlockFace::Down as usize],
            compiled.visuals[0].faces[BlockFace::East as usize],
            compiled.visuals[0].faces[BlockFace::West as usize],
        ]
    );
    assert_eq!(
        quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
        [
            0,
            0,
            0,
            0,
            MODEL_QUAD_FLAG_TWO_SIDED,
            MODEL_QUAD_FLAG_TWO_SIDED
        ]
    );
    assert_eq!(
        quads[2].positions,
        [
            quads[0].positions[1],
            quads[0].positions[0],
            quads[0].positions[3],
            quads[0].positions[2]
        ]
    );
    assert_eq!(
        quads[3].positions,
        [
            quads[1].positions[1],
            quads[1].positions[0],
            quads[1].positions[3],
            quads[1].positions[2]
        ]
    );
    let normal = |quad: &assets::ModelQuad| {
        let a = [
            i64::from(quad.positions[1][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[1][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[1][2]) - i64::from(quad.positions[0][2]),
        ];
        let b = [
            i64::from(quad.positions[2][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[2][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[2][2]) - i64::from(quad.positions[0][2]),
        ];
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    for (forward, reverse) in [(0, 2), (1, 3)] {
        let forward = normal(&quads[forward]);
        let reverse = normal(&quads[reverse]);
        assert!(
            forward
                .into_iter()
                .zip(reverse)
                .map(|(left, right)| left * right)
                .sum::<i64>()
                < 0,
            "kelp body windings must face opposite directions"
        );
    }
    let animations = quads
        .iter()
        .map(|quad| compiled.materials[quad.material as usize])
        .map(|material| {
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            let animation = compiled.animations[material.animation as usize];
            assert_eq!(animation.ticks_per_frame, 4);
            compiled.animation_frames[animation.frame_start as usize
                ..(animation.frame_start + animation.frame_count) as usize]
                .to_vec()
        })
        .collect::<Vec<_>>();
    assert_eq!(animations.len(), 6);
    for left in 0..animations.len() {
        for right in left + 1..animations.len() {
            assert_ne!(animations[left], animations[right]);
        }
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
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(record.flags.bits());
        bytes.push(record.model_family as u8);
        bytes.push(record.contributor_role as u8);
        bytes.push(record.model_state.mask());
        bytes.push(record.face_coverage);
        bytes.push(record.collision_seed.confidence as u8);
        bytes.push(record.provenance.bits());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).expect("small collision seed"));
        bytes.extend_from_slice(&record.collision_seed.shape_id.to_le_bytes());
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
        for field in [
            assets::ModelStateField::Orientation,
            assets::ModelStateField::Half,
            assets::ModelStateField::Open,
            assets::ModelStateField::Hinge,
            assets::ModelStateField::Connections,
            assets::ModelStateField::Growth,
            assets::ModelStateField::LiquidDepth,
            assets::ModelStateField::Flags,
        ] {
            bytes.extend_from_slice(&record.model_state.get(field).unwrap_or(0).to_le_bytes());
        }
        for collision_box in &record.collision_seed.boxes {
            for coordinate in [
                collision_box.min_x,
                collision_box.min_y,
                collision_box.min_z,
                collision_box.max_x,
                collision_box.max_y,
                collision_box.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
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
    let mip = &compiled.texture_pages[0].texture.mips[mip_index];
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
    assert_eq!(MATERIAL_FLAGS_MASK, 0x7ff);
    assert_eq!(std::mem::size_of::<Material>(), 12);
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
        opaque.texture.layer(),
        cherry.texture.layer(),
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
    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
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
        let layer = material_for_face(&compiled, record_id, BlockFace::Up)
            .texture
            .layer();
        let raw_mips = reference_raw_mips(base, colour);
        let base_survivors = alpha_survivors(&raw_mips[0]);
        for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
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
    let mip = &compiled.texture_pages[0].texture.mips[mip_index];
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

    assert_eq!(
        compiled.texture_pages[0].texture.layers, 3,
        "diagnostic + red + blue"
    );
    assert_eq!(
        compiled.materials.len(),
        4,
        "diagnostic + three descriptors"
    );
    assert_eq!(compiled.visuals[0].faces, compiled.visuals[1].faces);

    let red_plain = material_for_face(&compiled, 0, BlockFace::Up);
    let red_rotated = material_for_face(&compiled, 2, BlockFace::North);
    let blue = material_for_face(&compiled, 3, BlockFace::Up);
    assert_eq!(red_plain.texture.layer(), red_rotated.texture.layer());
    assert_eq!(red_plain.flags, 0);
    assert_eq!(red_rotated.flags, MATERIAL_FLAG_ROTATE_UV);
    assert_ne!(red_plain, red_rotated);
    assert_ne!(red_plain.texture.layer(), blue.texture.layer());
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

    assert_eq!(
        compiled.materials[0],
        Material {
            texture: assets::TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: assets::NO_ANIMATION
        }
    );
    assert_eq!(mip_pixel(&compiled, 0, 0, 0, 0), [255, 0, 255, 255]);
    assert_eq!(mip_pixel(&compiled, 0, 0, 1, 0), [0, 0, 0, 255]);
    assert_eq!(compiled.texture_pages[0].texture.mips.len(), 5);
    assert_eq!(
        compiled.texture_pages[0]
            .texture
            .mips
            .iter()
            .map(|mip| mip.size)
            .collect::<Vec<_>>(),
        [16, 8, 4, 2, 1]
    );

    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
    for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
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
fn compiler_supports_vanilla_glass_and_fails_closed_for_arbitrary_tinted_full_cubes() {
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
    assert_eq!(compiled.visuals[1].kind, VisualKind::Cube);
    assert!(
        compiled.visuals[1]
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    for face in BlockFace::ALL {
        assert_eq!(
            material_for_face(&compiled, 1, face).flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    let artifact = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let artifact_glass = artifact.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(artifact_glass.kind(), VisualKind::Cube);
    assert!(artifact_glass.flags().contains(BlockFlags::CUBE_GEOMETRY));
    for face in BlockFace::ALL {
        assert_eq!(
            artifact
                .material(artifact_glass.face(face).material_id())
                .flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    assert_eq!(
        compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6],
        "arbitrary tinted full cube must remain fail closed"
    );
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
                    mip_pixel(&compiled, 0, material.texture.layer(), 0, 0),
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
        mip_pixel(&compiled, 0, unrelated.texture.layer(), 0, 0),
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
    assert_eq!(compiled.texture_pages[0].texture.layers, 1);
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
fn compiler_compiles_grass_and_flipbook_faces_but_keeps_unlisted_blocks_diagnostic() {
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
    let mut sea_lantern_strip = solid(TILE_SIZE, TILE_SIZE, [30, 180, 200, 255]);
    sea_lantern_strip.extend(solid(TILE_SIZE, TILE_SIZE, [50, 210, 230, 255]));
    write_png(
        directory.path(),
        "textures/blocks/sea_lantern",
        TILE_SIZE,
        TILE_SIZE * 2,
        &sea_lantern_strip,
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
    assert!(
        sea_lantern
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(
        compiled.materials[sea_lantern.faces[0] as usize].animation,
        0
    );
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.materials.len(), 5);
    assert_eq!(compiled.texture_pages[0].texture.layers, 6);
}

#[test]
fn compiler_installs_recognized_flipbooks_after_loading_the_strip() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "water":{"textures":{"up":"water_still","down":"water_still","side":"water_flow"}},
            "lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}},
            "broken_water":{"textures":{"west":"water_flow","east":"water_flow","up":"water_still","north":"water_flow","south":"water_flow"}}
        }"#,
        r#"{"texture_data":{
            "water_still":{"textures":"textures/blocks/water_still"},
            "water_flow":{"textures":"textures/blocks/water_flow"},
            "lava_still":{"textures":"textures/blocks/lava_still"},
            "lava_flow":{"textures":"textures/blocks/lava_flow"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/water_still","atlas_tile":"water_still"},
            {"flipbook_texture":"textures/blocks/water_flow","atlas_tile":"water_flow"},
            {"flipbook_texture":"textures/blocks/lava_still","atlas_tile":"lava_still"},
            {"flipbook_texture":"textures/blocks/lava_flow","atlas_tile":"lava_flow"}
        ]"#,
    );
    let mut water_record = model_record(
        0,
        700,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":3}}"#,
        ModelFamily::Liquid,
    );
    water_record.contributor_role = ContributorRole::LiquidAdditional;
    assert_eq!(
        water_record.model_state.get(ModelStateField::LiquidDepth),
        None
    );
    let mut lava_record = model_record(
        1,
        701,
        "minecraft:lava",
        r#"{"liquid_depth":{"type":"int","value":15}}"#,
        ModelFamily::Liquid,
    );
    lava_record.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_depth = model_record(2, 702, "minecraft:water", "{}", ModelFamily::Liquid);
    missing_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut out_of_range_depth = model_record(
        3,
        703,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":16}}"#,
        ModelFamily::Liquid,
    );
    out_of_range_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_face = model_record(
        4,
        704,
        "minecraft:broken_water",
        r#"{"liquid_depth":{"type":"int","value":2}}"#,
        ModelFamily::Liquid,
    );
    missing_face.contributor_role = ContributorRole::LiquidAdditional;
    let records = [
        water_record,
        lava_record,
        missing_depth,
        out_of_range_depth,
        missing_face,
    ];
    for (index, path) in ["water_still", "water_flow", "lava_still", "lava_flow"]
        .into_iter()
        .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [20 + index as u8, 80, 200, 180]);
        strip.extend(solid(
            TILE_SIZE,
            TILE_SIZE,
            [30 + index as u8, 100, 220, 180],
        ));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile flipbook reference");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Liquid);
    assert_eq!(
        compiled.visuals[0].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert!(
        !compiled.visuals[0]
            .flags
            .contains(BlockFlags::CUBE_GEOMETRY)
    );
    assert_eq!(compiled.visuals[0].variant, 3);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[1].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.visuals[1].variant, 0);
    for visual in &compiled.visuals[2..] {
        assert_eq!(visual.kind, VisualKind::Diagnostic);
        assert_eq!(visual.faces, [DIAGNOSTIC_MATERIAL; 6]);
        assert_eq!(visual.variant, 0);
    }
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::Down as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::West as usize],
        compiled.visuals[0].faces[BlockFace::East as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::North as usize],
        compiled.visuals[0].faces[BlockFace::South as usize]
    );
    assert_ne!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::North as usize]
    );
    assert_eq!(
        compiled.visuals[1].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert_eq!(compiled.materials.len(), 7);
    let water_material =
        compiled.materials[compiled.visuals[0].faces[BlockFace::Up as usize] as usize];
    let water_flow =
        compiled.materials[compiled.visuals[0].faces[BlockFace::North as usize] as usize];
    assert_eq!(
        water_material.flags,
        MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
    );
    assert_eq!(water_material.animation, 0);
    assert_eq!(water_flow.animation, 1);
    assert_eq!(compiled.animations.len(), 4);
    assert_eq!(compiled.animation_frames.len(), 8);
    assert_eq!(compiled.texture_pages[0].texture.layers, 9);
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
fn compiler_rolls_reachable_layers_into_the_bounded_second_page() {
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

    let compiled = compile_pack(directory.path(), &records).expect("two-page asset set");
    assert_eq!(compiled.texture_pages.len(), 2);
    assert_eq!(
        compiled.texture_pages[0].texture.layers,
        MAX_TEXTURE_LAYERS as u32
    );
    assert_eq!(compiled.texture_pages[1].texture.layers, 1);
}
