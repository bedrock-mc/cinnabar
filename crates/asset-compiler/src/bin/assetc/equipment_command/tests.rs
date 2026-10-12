//! Original fixture for the shared entity/equipment behavior binding.

use super::*;

/// Writes one original pack definition and creates its parent family.
fn fixture(root: &Path, name: &str, value: serde_json::Value) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn entity_only_behavior_seats_bind_the_same_equipment_entity_digest() {
    let root = tempfile::tempdir().unwrap();
    let resource = root.path().join("resource");
    let behavior = root.path().join("behavior");
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(resource.join(family)).unwrap();
    }
    fixture(
        &resource,
        "entity/mount.json",
        serde_json::json!({
            "format_version":"1.8.0", "minecraft:client_entity":{"description":{"identifier":"fixture:mount"}}
        }),
    );
    fixture(
        &behavior,
        "entities/mount.json",
        serde_json::json!({
            "format_version":"1.21.0", "minecraft:entity":{
                "description":{"identifier":"fixture:mount"},
                "components":{"minecraft:rideable":{"seats":{"position":[0,1,0]}}}
            }
        }),
    );
    assert!(!behavior.join("items").exists());
    let manifest = root.path().join("source.json");
    std::fs::write(
        &manifest,
        include_bytes!("../../../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let out = root.path().join("equipment.bin");
    let report = root.path().join("equipment.json");
    compile_equipment_assets_command(&resource, &manifest, &out, &report, Some(&behavior)).unwrap();
    let entity = compile_entity_assets_with_behavior_report(
        &resource,
        &behavior,
        &std::fs::read(&manifest).unwrap(),
    )
    .unwrap()
    .assets;
    let blob = encode_entity_blob(&entity).unwrap();
    let expected: [u8; 32] = Sha256::digest(&blob).into();
    let equipment = assets::RuntimeEquipmentCatalog::decode(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(equipment.entity_blob_sha256(), expected);
    assert!(equipment.item_use().is_empty());
    let without_behavior =
        compile_entity_assets_with_report(&resource, &std::fs::read(&manifest).unwrap())
            .unwrap()
            .assets;
    let without_behavior: [u8; 32] =
        Sha256::digest(encode_entity_blob(&without_behavior).unwrap()).into();
    assert_ne!(expected, without_behavior);
}
