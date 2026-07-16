use super::support::*;

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
    .expect("compile flipbook into MCBEAS05 tables");

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
