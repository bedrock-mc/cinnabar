use std::{fs, path::Path, process::Command};

use asset_compiler::compile_entity_assets;
use assets::{EntityAssetKind, EntityDependencyKind, EntityDependencyResolution};
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
            "render_controllers":[
              "controller.render.allay",
              {"controller.render.allay.compat":"query.is_in_water"}
            ]
          }}
        }"#,
    );
    write(
        root,
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{
          "description":{"identifier":"geometry.allay","texture_width":32,"texture_height":64},
          "bones":[
            {"name":"root","pivot":[0,1,0],"rotation":[0,15,0],"mirror":true,"inflate":0.25,
             "cubes":[{"origin":[-2.5,5.01,-2.5],"size":[5,5,5],"rotation":[0,0,-2.5],"uv":[0,0]}]},
            {"name":"wing","parent":"root","pivot":[0.5,4,1],
             "cubes":[{"origin":[0.5,-1,1],"size":[0,5,8],"uv":[16,14],"mirror":false,"inflate":-0.2}]}
          ]
        }]}"#,
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
        br#"{"format_version":"1.8.0","render_controllers":{"controller.render.allay":{"geometry":"Geometry.default","textures":["Texture.default"]},"controller.render.allay.compat":{"geometry":"Geometry.default","textures":["Texture.default"]}}}"#,
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
    assert_eq!(first.geometries.len(), 1);
    let geometry = &first.geometries[0];
    assert_eq!(geometry.identifier.as_ref(), "geometry.allay");
    assert_eq!((geometry.texture_width, geometry.texture_height), (32, 64));
    assert_eq!(geometry.bones.len(), 2);
    assert_eq!(geometry.bones[1].parent.as_deref(), Some("root"));
    assert_eq!(geometry.bones[0].rotation[1].get(), 15.0);
    assert_eq!(geometry.bones[0].cubes[0].pivot[0].get(), 0.0);
    assert_eq!(geometry.bones[0].cubes[0].inflate.get(), 0.25);
    assert!(geometry.bones[0].cubes[0].mirror);
    assert_eq!(geometry.bones[1].cubes[0].inflate.get(), -0.2);
    assert!(!geometry.bones[1].cubes[0].mirror);

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
    assert!(
        !dependencies
            .iter()
            .any(|(_, target)| *target == "query.is_in_water")
    );
    assert!(
        entity
            .dependencies
            .iter()
            .all(|dependency| { dependency.resolution == EntityDependencyResolution::Catalog })
    );
    assert!(first.symbols.iter().any(|symbol| {
        symbol.kind == EntityAssetKind::Texture
            && symbol.identifier.as_ref() == "textures/entity/allay/allay"
    }));
}

#[test]
fn compiler_marks_out_of_scope_dependency_edges_explicitly_external() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "entity/allay.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:allay","textures":{"default":"textures/items/external"},"geometry":{"default":"geometry.allay"}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let entity = compiled
        .symbols
        .iter()
        .find(|symbol| symbol.kind == EntityAssetKind::Entity)
        .unwrap();
    let external = entity
        .dependencies
        .iter()
        .find(|dependency| dependency.identifier.as_ref() == "textures/items/external")
        .unwrap();
    assert_eq!(external.resolution, EntityDependencyResolution::External);
    let geometry = entity
        .dependencies
        .iter()
        .find(|dependency| dependency.identifier.as_ref() == "geometry.allay")
        .unwrap();
    assert_eq!(geometry.resolution, EntityDependencyResolution::Catalog);
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
fn compiler_preserves_same_named_geometry_payload_candidates_from_distinct_sources() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/z_allay.compat.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay","texture_width":16,"texture_height":16},"bones":[]}]}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let candidates = compiled
        .geometries
        .iter()
        .filter(|geometry| geometry.identifier.as_ref() == "geometry.allay")
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 2);
    assert!(candidates[0].source_index < candidates[1].source_index);
    assert_eq!(candidates[1].texture_width, 16);
}

#[test]
fn compiler_preserves_sparse_per_face_uvs_and_optional_uv_sizes() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay","texture_width":32,"texture_height":32},"bones":[{"name":"root","cubes":[{"origin":[0,-2.5,-3],"size":[0,5,16],"uv":{"east":{"uv":[0,0],"uv_size":[5,16]}}}]}]}]}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let uv = &compiled.geometries[0].bones[0].cubes[0].uv;
    let assets::EntityGeometryUv::Faces(faces) = uv else {
        panic!("expected per-face UV payload");
    };
    assert_eq!(faces.east.as_ref().unwrap().uv[0].get(), 0.0);
    assert_eq!(faces.east.as_ref().unwrap().uv_size.unwrap()[1].get(), 16.0);
    assert!(faces.west.is_none());
}

#[test]
fn compiler_uses_bedrock_default_cube_uv() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay"},"bones":[{"name":"root","cubes":[{"origin":[-3,2,-3],"size":[6,8,6]}]}]}]}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let assets::EntityGeometryUv::Box(uv) = &compiled.geometries[0].bones[0].cubes[0].uv else {
        panic!("expected default box UV");
    };
    assert_eq!([uv[0].get(), uv[1].get()], [0.0, 0.0]);
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
        br#"{"format_version":"1.8.0","geometry.allay":{"texturewidth":32,"textureheight":32,"bones":[]},"geometry.allay.compat":{"texturewidth":64,"textureheight":32,"bones":[]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).expect("legacy geometry schema");
    for identifier in ["geometry.allay", "geometry.allay.compat"] {
        assert!(compiled.symbols.iter().any(|symbol| {
            symbol.kind == EntityAssetKind::Geometry && symbol.identifier.as_ref() == identifier
        }));
    }
    assert_eq!(compiled.geometries.len(), 2);
    assert_eq!(compiled.geometries[0].texture_width, 32);
    assert_eq!(compiled.geometries[1].texture_width, 64);
}

#[test]
fn compiler_uses_bedrock_legacy_default_texture_dimensions() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay":{"bones":[]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.geometries[0].texture_width, 64);
    assert_eq!(compiled.geometries[0].texture_height, 64);
}

#[test]
fn compiler_uses_bedrock_modern_default_texture_dimensions() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay"},"bones":[]}]}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.geometries[0].texture_width, 64);
    assert_eq!(compiled.geometries[0].texture_height, 64);
}

#[test]
fn compiler_preserves_legacy_geometry_inheritance_and_inherited_bone_parents() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.base":{"texturewidth":64,"textureheight":64,"bones":[{"name":"head"}]},"geometry.allay:geometry.base":{"texturewidth":64,"textureheight":64,"bones":[{"name":"nose","parent":"head"}]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let derived = compiled
        .geometries
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == "geometry.allay")
        .unwrap();
    let inheritance = derived.inherits.as_ref().unwrap();
    assert_eq!(inheritance.identifier.as_ref(), "geometry.base");
    assert_eq!(inheritance.resolution, EntityDependencyResolution::Catalog);
    assert!(compiled.symbols.iter().any(|symbol| {
        symbol.kind == EntityAssetKind::Geometry && symbol.identifier.as_ref() == "geometry.allay"
    }));
    assert!(
        !compiled
            .symbols
            .iter()
            .any(|symbol| { symbol.identifier.as_ref() == "geometry.allay:geometry.base" })
    );
}

#[test]
fn compiler_marks_builtin_legacy_geometry_inheritance_external() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay:geometry.humanoid":{"bones":[]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.geometries[0].inherits.as_ref().unwrap().resolution,
        EntityDependencyResolution::External
    );
}

#[test]
fn compiler_accepts_bedrock_case_insensitive_bone_parent_references() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay":{"bones":[{"name":"Head"},{"name":"mouth","parent":"head"}]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.geometries[0].bones[1].parent.as_deref(),
        Some("head")
    );
}

#[test]
fn compiler_normalizes_official_self_parent_bones_to_roots() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay":{"bones":[{"name":"body","parent":"body"}]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(compiled.geometries[0].bones[0].parent.is_none());
}

#[test]
fn compiler_preserves_duplicate_official_bone_generation_candidates() {
    let pack = synthetic_pack();
    write(
        pack.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.8.0","geometry.allay":{"bones":[{"name":"hat","pivot":[0,1,0]},{"name":"hat","pivot":[0,2,0]}]}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.geometries[0].bones.len(), 2);
    assert_eq!(compiled.geometries[0].bones[0].pivot[1].get(), 1.0);
    assert_eq!(compiled.geometries[0].bones[1].pivot[1].get(), 2.0);
}

#[test]
fn compiler_rejects_unknown_geometry_fields_invalid_hierarchy_and_unbounded_numbers() {
    let unknown = synthetic_pack();
    write(
        unknown.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay","texture_width":32,"texture_height":32},"bones":[{"name":"root","unexpected":true}]}]}"#,
    );
    assert!(compile_entity_assets(unknown.path(), MANIFEST).is_err());

    let cycle = synthetic_pack();
    write(
        cycle.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay","texture_width":32,"texture_height":32},"bones":[{"name":"a","parent":"b"},{"name":"b","parent":"a"}]}]}"#,
    );
    assert!(compile_entity_assets(cycle.path(), MANIFEST).is_err());

    let unbounded = synthetic_pack();
    write(
        unbounded.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay","texture_width":32,"texture_height":32},"bones":[{"name":"root","pivot":[1e100,0,0]}]}]}"#,
    );
    assert!(compile_entity_assets(unbounded.path(), MANIFEST).is_err());

    let negative_size = synthetic_pack();
    write(
        negative_size.path(),
        "models/entity/allay.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.allay"},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,-1,1]}]}]}]}"#,
    );
    assert!(compile_entity_assets(negative_size.path(), MANIFEST).is_err());
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
    assert_eq!(report["schema"], 2);
    assert_eq!(report["counts"]["sources"], 7);
    assert_eq!(report["counts"]["symbols"], decoded.symbols().len());
    assert_eq!(report["counts"]["geometries"], 1);
    assert_eq!(report["counts"]["bones"], 2);
    assert_eq!(report["counts"]["cubes"], 2);
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
