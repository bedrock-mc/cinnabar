use std::{fs, path::Path};

use assets::{EntityAssetKind, RuntimeEntityAssets};
use pack_compiler::compile_entity_assets;
use serde_json::{Value, json};

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");

fn write(root: &Path, path: &str, value: Value) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

fn pack(ceiling: Value, definitions: &[(&str, Value, &str)]) -> tempfile::TempDir {
    let pack = tempfile::tempdir().unwrap();
    let root = pack.path();
    for family in ["animations", "animation_controllers", "textures/entity"] {
        fs::create_dir_all(root.join(family)).unwrap();
    }
    write(
        root,
        "manifest.json",
        json!({"header":{"min_engine_version":ceiling}}),
    );
    for (path, minimum, geometry) in definitions {
        let mut description = json!({
            "identifier":"minecraft:horse",
            "geometry":{"default":geometry},
            "materials":{"default":"entity_alphatest"},
            "render_controllers":["controller.render.fixture"]
        });
        if !minimum.is_null() {
            description["min_engine_version"] = minimum.clone();
        }
        write(
            root,
            path,
            json!({
                "format_version":"1.8.0",
                "minecraft:client_entity":{"description":description}
            }),
        );
        write(
            root,
            &format!("models/entity/{geometry}.json"),
            json!({
                "format_version":"1.12.0",
                "minecraft:geometry":[{
                    "description":{"identifier":geometry},
                    "bones":[{"name":"body"}]
                }]
            }),
        );
    }
    write(
        root,
        "render_controllers/fixture.json",
        json!({
            "format_version":"1.8.0",
            "render_controllers":{"controller.render.fixture":{
                "geometry":"Geometry.default",
                "materials":[{"*":"Material.default"}],
                "textures":[]
            }}
        }),
    );
    pack
}

fn selected_geometry(root: &Path) -> String {
    let compiled = compile_entity_assets(root, MANIFEST).unwrap();
    let assets = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let candidates = assets.symbol_candidates(EntityAssetKind::Entity, "minecraft:horse");
    assert_eq!(
        candidates.len(),
        1,
        "one compatible client definition owns the rig"
    );
    let symbol = assets
        .symbols()
        .iter()
        .position(|s| std::ptr::eq(s, &candidates[0]))
        .unwrap();
    let rig = assets
        .rig_bindings()
        .iter()
        .find(|rig| rig.entity_symbol as usize == symbol)
        .unwrap();
    let geometry = assets.rig_geometries()[rig.first_geometry as usize].geometry as usize;
    assets.geometries()[geometry].identifier.to_string()
}

#[test]
fn legacy_bed_geometry_is_retained_without_overriding_modern_actor_geometry() {
    let pack = pack(
        json!([1, 20, 0]),
        &[("entity/horse.json", Value::Null, "geometry.fixture")],
    );
    write(
        pack.path(),
        assets::LEGACY_ENTITY_GEOMETRY_PATH,
        json!({
            "format_version":"1.8.0",
            (assets::BED_GEOMETRY_IDENTIFIER):{
                "texturewidth":64,"textureheight":64,
                "bones":[{"name":"bed","cubes":[{
                    "origin":[0,0,0],"size":[16,32,6],"uv":[0,0]
                }]}]
            },
            "geometry.fixture":{
                "texturewidth":32,"textureheight":32,
                "bones":[{"name":"obsolete"}]
            }
        }),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let assets = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let bed = assets
        .geometries()
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == assets::BED_GEOMETRY_IDENTIFIER)
        .expect("legacy bed geometry must be available to its block renderer");
    assert_eq!(
        bed.bones[0].cubes[0].size.map(|value| value.get()),
        [16.0, 32.0, 6.0]
    );
    let actor = assets
        .geometries()
        .iter()
        .filter(|geometry| geometry.identifier.as_ref() == "geometry.fixture")
        .collect::<Vec<_>>();
    assert_eq!(
        actor.len(),
        1,
        "legacy actor copies must not override modern rigs"
    );
    assert_eq!(actor[0].bones[0].name.as_ref(), "body");
}

#[test]
fn horse_uses_the_highest_compatible_definition_instead_of_the_first_filename() {
    let pack = pack(
        json!([1, 20, 0]),
        &[
            ("entity/a_old.json", Value::Null, "geometry.old"),
            ("entity/b_future.json", json!("99.0.0"), "geometry.future"),
            (
                "entity/c_previous.json",
                json!("1.2.6"),
                "geometry.previous",
            ),
            (
                "entity/z_current.json",
                json!("1.17.10"),
                "geometry.current",
            ),
        ],
    );
    assert_eq!(selected_geometry(pack.path()), "geometry.current");
}

#[test]
fn the_pack_engine_ceiling_can_select_an_older_horse_definition() {
    let pack = pack(
        json!([1, 2, 6]),
        &[
            ("entity/a_old.json", Value::Null, "geometry.old"),
            (
                "entity/b_previous.json",
                json!([1, 2, 6]),
                "geometry.previous",
            ),
            (
                "entity/c_current.json",
                json!("1.17.10"),
                "geometry.current",
            ),
        ],
    );
    assert_eq!(selected_geometry(pack.path()), "geometry.previous");
}

#[test]
fn an_unversioned_definition_remains_the_fallback_below_every_versioned_one() {
    let pack = pack(
        json!([1, 0, 0]),
        &[
            ("entity/a_future.json", json!("1.17.10"), "geometry.future"),
            ("entity/z_default.json", Value::Null, "geometry.default"),
        ],
    );
    assert_eq!(selected_geometry(pack.path()), "geometry.default");
}

#[test]
fn shortened_minima_remain_versioned_and_use_zero_for_missing_components() {
    for (minimum, ceiling) in [
        (json!("1"), json!([1, 0, 0])),
        (json!("1.17"), json!([1, 17, 0])),
    ] {
        let pack = pack(
            ceiling,
            &[
                ("entity/a_default.json", Value::Null, "geometry.default"),
                ("entity/z_versioned.json", minimum, "geometry.versioned"),
            ],
        );
        assert_eq!(selected_geometry(pack.path()), "geometry.versioned");
    }
}

#[test]
fn prerelease_minima_compare_numeric_identifiers_against_the_pack_ceiling() {
    let pack = pack(
        json!("1.17.10-beta.3+pack.2"),
        &[
            ("entity/a_default.json", Value::Null, "geometry.default"),
            (
                "entity/b_newer.json",
                json!("1.17.10-beta.10"),
                "geometry.newer",
            ),
            (
                "entity/c_release.json",
                json!("1.17.10"),
                "geometry.release",
            ),
            (
                "entity/z_current.json",
                json!("1.17.10-beta.2+asset.1"),
                "geometry.current",
            ),
        ],
    );
    assert_eq!(selected_geometry(pack.path()), "geometry.current");
}

#[test]
fn malformed_minima_use_the_unversioned_fallback() {
    for minimum in [json!("1.017.10"), json!("1.65536.0"), json!([1, 65536, 0])] {
        let pack = pack(
            json!([1, 65535, 0]),
            &[
                ("entity/a_malformed.json", minimum, "geometry.malformed"),
                (
                    "entity/z_current.json",
                    json!([1, 2, 0]),
                    "geometry.current",
                ),
            ],
        );
        assert_eq!(selected_geometry(pack.path()), "geometry.current");
    }
}

#[test]
fn legacy_entity_schemas_do_not_use_the_minimum_engine_field() {
    let pack = pack(
        json!([1, 20, 0]),
        &[
            ("entity/a_legacy.json", json!("1.17.10"), "geometry.legacy"),
            ("entity/z_current.json", json!("1.2.6"), "geometry.current"),
        ],
    );
    let path = pack.path().join("entity/a_legacy.json");
    let mut legacy: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    legacy["format_version"] = json!("1.7.0");
    fs::write(path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(selected_geometry(pack.path()), "geometry.current");
}

#[test]
fn future_only_definitions_do_not_produce_a_rig() {
    let pack = pack(
        json!([1, 0, 0]),
        &[("entity/future.json", json!("1.17.10"), "geometry.future")],
    );
    let assets =
        RuntimeEntityAssets::from_compiled(compile_entity_assets(pack.path(), MANIFEST).unwrap())
            .unwrap();
    assert!(
        assets
            .symbol_candidates(EntityAssetKind::Entity, "minecraft:horse")
            .is_empty()
    );
    assert!(assets.rig_bindings().is_empty());
}

#[test]
fn equally_versioned_definitions_require_merging_even_when_build_suffixes_differ() {
    let pack = pack(
        json!([1, 20, 0]),
        &[
            ("entity/a.json", json!("1.17.10+asset.1"), "geometry.first"),
            ("entity/b.json", json!("1.17.10+asset.2"), "geometry.second"),
        ],
    );
    let error = compile_entity_assets(pack.path(), MANIFEST)
        .unwrap_err()
        .to_string();
    assert!(error.contains("equally versioned client definitions require pack-order merging"));
}

#[test]
fn a_present_manifest_requires_a_valid_engine_ceiling() {
    for ceiling in [
        Value::Null,
        json!("bad"),
        json!("1.020.0"),
        json!([1, 65536, 0]),
    ] {
        let pack = pack(
            ceiling,
            &[("entity/default.json", Value::Null, "geometry.default")],
        );
        let error = compile_entity_assets(pack.path(), MANIFEST)
            .unwrap_err()
            .to_string();
        assert!(error.contains("manifest.json has missing or invalid header.min_engine_version"));
    }
}

#[test]
fn manifestless_base_assets_use_the_pinned_compiler_target() {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let pack = pack(
        Value::Null,
        &[
            ("entity/a_default.json", Value::Null, "geometry.default"),
            (
                "entity/z_target.json",
                target["game_version"].clone(),
                "geometry.target",
            ),
        ],
    );
    fs::remove_file(pack.path().join("manifest.json")).unwrap();
    assert_eq!(selected_geometry(pack.path()), "geometry.target");
}

#[test]
fn legacy_cape_is_available_to_renderer_and_reference_sidecar() {
    let pack = pack(
        json!([1, 20, 0]),
        &[("entity/horse.json", Value::Null, "geometry.fixture")],
    );
    let cape = assets::CAPE_GEOMETRY_IDENTIFIER;
    write(
        pack.path(),
        assets::LEGACY_ENTITY_GEOMETRY_PATH,
        json!({
            "format_version":"1.8.0",
            (cape):{"texturewidth":64,"textureheight":32,"bones":[{"name":"cape","cubes":[{
                "origin":[-5,0,0],"size":[10,16,1],"uv":[0,0]
            }]}]},
            "geometry.fixture":{"bones":[{"name":"obsolete"}]}
        }),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let runtime = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let geometry = runtime
        .geometries()
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == cape)
        .expect("cape model must be available to the cape renderer");
    assert_eq!(
        geometry.bones[0].cubes[0].size.map(|value| value.get()),
        [10.0, 16.0, 1.0]
    );
    let refs = pack_compiler::compile_vanilla_entity_refs(pack.path()).unwrap();
    let index = refs.geometry_index[cape] as usize;
    let source: Value = serde_json::from_str(&refs.geometry_files[index].text).unwrap();
    assert!(source.get(cape).is_some());
    assert!(
        source.get("geometry.fixture").is_none(),
        "legacy actor copies stay excluded"
    );
}

#[test]
fn obsolete_only_legacy_catalog_is_ignored() {
    let pack = pack(
        json!([1, 20, 0]),
        &[("entity/horse.json", Value::Null, "geometry.fixture")],
    );
    write(
        pack.path(),
        assets::LEGACY_ENTITY_GEOMETRY_PATH,
        json!({
            "format_version":"1.8.0",
            "geometry.fixture":{"bones":[{"name":"obsolete"}]}
        }),
    );
    assert_eq!(selected_geometry(pack.path()), "geometry.fixture");
    let refs = pack_compiler::compile_vanilla_entity_refs(pack.path()).unwrap();
    assert!(
        refs.geometry_files
            .iter()
            .all(|file| file.path != assets::LEGACY_ENTITY_GEOMETRY_PATH)
    );
}
