use super::*;

/// Writes a small original source document under a temporary pack root.
fn write(root: &Path, path: &str, value: Value) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, serde_json::to_vec(&value).unwrap()).unwrap();
}

/// Supplies original model/controller documents and tag-derived texture names without game art.
fn sample() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for family in ["entity", "animations", "animation_controllers"] {
        fs::create_dir_all(root.join(family)).unwrap();
    }
    write(
        root,
        MODEL,
        serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":GEOMETRY,"texture_width":8,"texture_height":8},
            "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]
        }]}),
    );
    write(
        root,
        CONTROLLERS,
        serde_json::json!({"format_version":"1.8.0","render_controllers":{
            (CONTROLLER):{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}
        }}),
    );
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let tag = facts.tag(assets::facts::HARNESS_TAG).unwrap();
    for record in &facts.payload().records {
        if !record.fact.tags.contains(tag) {
            continue;
        }
        let color = record
            .identifier
            .strip_prefix("minecraft:")
            .unwrap()
            .strip_suffix("_harness")
            .unwrap();
        let path = root.join(format!("textures/entity/harness/harness_{color}.png"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Carrier metadata compilation only inventories these bytes.
        fs::write(path, b"original fixture raster").unwrap();
    }
    dir
}

#[test]
fn omitted_harness_bindings_follow_registry_tags_and_present_resources() {
    let dir = sample();
    let compiled =
        compile_entity_assets_with_report(dir.path(), assets::VANILLA_SOURCE_MANIFEST.as_bytes())
            .unwrap();
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let tag = facts.tag(assets::facts::HARNESS_TAG).unwrap();
    let expected = facts
        .payload()
        .records
        .iter()
        .filter(|record| record.fact.tags.contains(tag))
        .count();
    assert_eq!(compiled.equipment_bindings.len(), expected);
    for binding in &compiled.equipment_bindings {
        assert!(
            facts
                .by_identifier(&binding.identifier)
                .unwrap()
                .tags
                .contains(tag)
        );
        assert_eq!(binding.category, assets::EquipmentCategory::Body);
        assert_eq!(binding.geometry.identifier.as_ref(), GEOMETRY);
        assert_eq!(
            binding.geometry.resolution,
            assets::EntityDependencyResolution::Catalog
        );
        assert_eq!(
            binding.texture.resolution,
            assets::EntityDependencyResolution::Catalog
        );
        assert_eq!(binding.render_controller.as_ref(), CONTROLLER);
    }
    fs::remove_dir_all(dir.path().join("textures/entity/harness")).unwrap();
    let compiled =
        compile_entity_assets_with_report(dir.path(), assets::VANILLA_SOURCE_MANIFEST.as_bytes())
            .unwrap();
    assert!(
        compiled.equipment_bindings.is_empty(),
        "absent artwork must not create an invented model"
    );
}

#[test]
fn authored_harness_condition_and_resource_removal_keep_their_meaning() {
    let dir = sample();
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let tag = facts.tag(assets::facts::HARNESS_TAG).unwrap();
    let item = &facts
        .payload()
        .records
        .iter()
        .find(|record| record.fact.tags.contains(tag))
        .unwrap()
        .identifier;
    write(
        dir.path(),
        "attachables/override.json",
        serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"fixture:disabled","item":{(item.as_ref()):"0"},
            "geometry":{"default":GEOMETRY},"textures":{"default":"textures/fixture"},
            "materials":{"default":"entity_alphatest"},"render_controllers":[CONTROLLER]
        }}}),
    );
    let compiled =
        compile_entity_assets_with_report(dir.path(), assets::VANILLA_SOURCE_MANIFEST.as_bytes())
            .unwrap();
    let bindings: Vec<_> = compiled
        .equipment_bindings
        .iter()
        .filter(|binding| binding.identifier == *item)
        .collect();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].attachable_identifier(), "fixture:disabled");
    fs::remove_file(dir.path().join(CONTROLLERS)).unwrap();
    let compiled =
        compile_entity_assets_with_report(dir.path(), assets::VANILLA_SOURCE_MANIFEST.as_bytes())
            .unwrap();
    assert_eq!(
        compiled.equipment_bindings.len(),
        1,
        "no controller must leave only the authored override"
    );
}
