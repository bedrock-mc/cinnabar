use std::{fs, path::Path, process::Command};

use asset_compiler::compile_entity_assets;
use assets::{EntityAssetKind, EntityDependencyKind};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../assets/vanilla-source.json");

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn synthetic_pack() -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(
        root,
        "entity/allay.entity.json",
        br#"{
          "format_version":"1.10.0",
          "minecraft:client_entity":{"description":{
            "identifier":"minecraft:allay",
            "textures":{"default":"textures/entity/allay/allay"},
            "geometry":{"default":"geometry.allay"},
            "animations":{"idle":"animation.allay.idle","general":"controller.animation.allay.general"},
            "animation_controllers":[{"general":"controller.animation.allay.general"}],
            "render_controllers":["controller.render.allay"]
          }}
        }"#,
    );
    write(
        root,
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay"},"bones":[]}]}"#,
    );
    write(
        root,
        "animations/allay.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.allay.idle":{"loop":true}}}"#,
    );
    write(
        root,
        "animation_controllers/allay.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.allay.general":{"states":{"default":{"animations":["idle"]}}}}}"#,
    );
    write(
        root,
        "render_controllers/allay.render_controllers.json",
        br#"{"format_version":"1.8.0","render_controllers":{"controller.render.allay":{"geometry":"Geometry.default","textures":["Texture.default"]}}}"#,
    );
    write(root, "textures/entity/allay/allay.png", b"not-decoded-yet");
    write(
        root,
        "textures/entity/allay/allay.texture_set.json",
        br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"allay"}}"#,
    );
    temporary
}

#[test]
fn compiler_enumerates_entity_authority_and_dependencies_deterministically() {
    let pack = synthetic_pack();
    let first = compile_entity_assets(pack.path(), MANIFEST).expect("compile entity catalog");
    let second = compile_entity_assets(pack.path(), MANIFEST).expect("compile twice");
    assert_eq!(first, second);
    assert_eq!(first.sources.len(), 7);

    let kinds = first
        .symbols
        .iter()
        .map(|symbol| symbol.kind)
        .collect::<Vec<_>>();
    for expected in [
        EntityAssetKind::Entity,
        EntityAssetKind::Geometry,
        EntityAssetKind::Animation,
        EntityAssetKind::AnimationController,
        EntityAssetKind::RenderController,
        EntityAssetKind::Texture,
    ] {
        assert!(kinds.contains(&expected), "missing {expected:?}");
    }
    let entity = first
        .symbols
        .iter()
        .find(|symbol| symbol.kind == EntityAssetKind::Entity)
        .unwrap();
    let dependencies = entity
        .dependencies
        .iter()
        .map(|dependency| (dependency.kind, dependency.identifier.as_ref()))
        .collect::<Vec<_>>();
    for expected in [
        (EntityDependencyKind::Geometry, "geometry.allay"),
        (EntityDependencyKind::Animation, "animation.allay.idle"),
        (
            EntityDependencyKind::AnimationController,
            "controller.animation.allay.general",
        ),
        (
            EntityDependencyKind::RenderController,
            "controller.render.allay",
        ),
        (EntityDependencyKind::Texture, "textures/entity/allay/allay"),
    ] {
        assert!(dependencies.contains(&expected), "missing {expected:?}");
    }
}

#[test]
fn compiler_rejects_duplicate_json_keys_and_unknown_family_root_fields() {
    let duplicate = synthetic_pack();
    write(
        duplicate.path(),
        "entity/allay.entity.json",
        br#"{"format_version":"1.10.0","format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:allay"}}}"#,
    );
    assert!(compile_entity_assets(duplicate.path(), MANIFEST).is_err());

    let unknown = synthetic_pack();
    write(
        unknown.path(),
        "animations/allay.animation.json",
        br#"{"format_version":"1.8.0","animations":{},"unexpected":true}"#,
    );
    assert!(compile_entity_assets(unknown.path(), MANIFEST).is_err());
}

#[test]
fn compiler_accepts_bedrock_line_and_block_comments_without_touching_strings() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "animation_controllers/allay.animation_controllers.json",
        br#"{
          // Bedrock Samples intentionally ships JSON-with-comments.
          "format_version":"1.10.0",
          "animation_controllers":{
            "controller.animation.allay.general":{
              /* URLs and comment markers inside strings remain bytes. */
              "states":{"default":{"transitions":[{"next":"query.mark_variant == 'http://a/*b*/'"}]}}
            }
          }
        }"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).expect("comments are supported");
    assert!(compiled.symbols.iter().any(|symbol| {
        symbol.kind == EntityAssetKind::AnimationController
            && symbol.identifier.as_ref() == "controller.animation.allay.general"
    }));
}

#[test]
fn compiler_preserves_same_named_symbols_from_distinct_pinned_sources() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "animation_controllers/allay.compat.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.allay.general":{"states":{"compat":{}}}}}"#,
    );
    let compiled =
        compile_entity_assets(pack.path(), MANIFEST).expect("compile duplicate symbol sources");
    let matches = compiled
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == EntityAssetKind::AnimationController
                && symbol.identifier.as_ref() == "controller.animation.allay.general"
        })
        .count();
    assert_eq!(matches, 2, "both pinned definitions remain attributable");
}

#[test]
fn compiler_treats_duplicates_inside_opaque_animation_payloads_as_runtime_semantics() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "animations/allay.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.allay.idle":{"bones":{"wing":{},"wing":{"rotation":[0,1,0]}}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST)
        .expect("the official pack contains duplicate opaque bone keys");
    assert!(compiled.symbols.iter().any(|symbol| {
        symbol.kind == EntityAssetKind::Animation
            && symbol.identifier.as_ref() == "animation.allay.idle"
    }));
}

#[test]
fn compiler_enumerates_legacy_geometry_root_identifiers() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay":{"bones":[]},"geometry.allay.compat":{"bones":[]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).expect("legacy geometry schema");
    for identifier in ["geometry.allay", "geometry.allay.compat"] {
        assert!(compiled.symbols.iter().any(|symbol| {
            symbol.kind == EntityAssetKind::Geometry && symbol.identifier.as_ref() == identifier
        }));
    }
}

#[test]
fn compiler_accepts_utf8_bom_in_pinned_json_sources() {
    let pack = synthetic_pack();
    let mut source = b"\xef\xbb\xbf".to_vec();
    source.extend_from_slice(
        br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"allay"}}"#,
    );
    write(
        pack.path(),
        "textures/entity/allay/allay.texture_set.json",
        &source,
    );
    compile_entity_assets(pack.path(), MANIFEST).expect("Bedrock texture sets may use a UTF-8 BOM");
}

#[test]
fn compiler_rejects_unbounded_entity_source_directory_depth() {
    let pack = synthetic_pack();
    let mut relative = String::from("textures/entity");
    for _ in 0..34 {
        relative.push_str("/nested");
    }
    write(pack.path(), &format!("{relative}/deep.png"), b"bounded");
    assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
}

#[test]
fn compiler_rejects_modified_manifest_and_unsupported_texture_payloads() {
    let pack = synthetic_pack();
    let modified = String::from_utf8(MANIFEST.to_vec())
        .unwrap()
        .replace("local-only", "committed");
    assert!(compile_entity_assets(pack.path(), modified.as_bytes()).is_err());

    write(
        pack.path(),
        "textures/entity/allay/readme.txt",
        b"unexpected",
    );
    assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
}

#[test]
fn assetc_entity_assets_writes_deterministic_carrier_and_report() {
    let pack = synthetic_pack();
    let outputs = tempfile::tempdir().unwrap();
    let manifest = outputs.path().join("vanilla-source.json");
    let blob = outputs.path().join("vanilla-v1.mcbeent");
    let report = outputs.path().join("entity-assets.json");
    fs::write(&manifest, MANIFEST).unwrap();

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_assetc"))
            .args(["entity-assets", "--pack"])
            .arg(pack.path())
            .arg("--source-manifest")
            .arg(&manifest)
            .arg("--out")
            .arg(&blob)
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let first_blob = fs::read(&blob).unwrap();
    let first_report = fs::read(&report).unwrap();
    let second = run();
    assert!(second.status.success());
    assert_eq!(fs::read(&blob).unwrap(), first_blob);
    assert_eq!(fs::read(&report).unwrap(), first_report);

    let decoded = assets::RuntimeEntityAssets::decode(&first_blob).unwrap();
    assert_eq!(decoded.sources().len(), 7);
    let report: serde_json::Value = serde_json::from_slice(&first_report).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["counts"]["sources"], 7);
    assert_eq!(report["counts"]["symbols"], decoded.symbols().len());
    assert_eq!(report["sources"].as_array().unwrap().len(), 7);
    assert_eq!(
        report["symbols"].as_array().unwrap().len(),
        decoded.symbols().len()
    );
    assert!(
        !String::from_utf8(first_report)
            .unwrap()
            .contains(&pack.path().display().to_string()),
        "deterministic report must not leak a machine-specific canonical path"
    );
}
