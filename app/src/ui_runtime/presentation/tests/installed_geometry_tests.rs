//! Installed carrier integration uses the app startup paths.
use super::*;
use assets::{RuntimeAssets, RuntimeEntityAssets};

#[test]
fn installed_carriers_admit_geometry_and_keep_diagnostic_world_fallback() {
    use crate::asset_startup::equipment_carrier::equipment_asset_path;
    use crate::asset_startup::{DEFAULT_ASSET_PATH, ENTITY_ASSETS_FILENAME, icon_asset_path};
    use assets::RuntimeIconCatalog;
    let world_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(DEFAULT_ASSET_PATH);
    let (Ok(world), Ok(entities), Ok(icons)) = (
        std::fs::read(&world_path),
        std::fs::read(world_path.with_file_name(ENTITY_ASSETS_FILENAME)),
        std::fs::read(icon_asset_path(&world_path)),
    ) else {
        eprintln!("local GUI carrier regression skipped: runtime carriers absent");
        eprintln!(
            "skipping installed_carriers_admit_geometry_and_keep_diagnostic_world_fallback: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let world = RuntimeAssets::decode(&world).unwrap();
    let entities = RuntimeEntityAssets::decode(&entities).unwrap();
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let equipment = std::fs::read(equipment_asset_path(&world_path))
        .ok()
        .map(|bytes| Arc::new(assets::RuntimeEquipmentCatalog::decode(&bytes).unwrap()));
    client_ui::test_support::assert_installed_geometry(&world, &entities, icons, equipment);
}

#[test]
fn installed_shield_bound_root_stays_at_each_hand_not_above_head() {
    use crate::asset_startup::{
        DEFAULT_ASSET_PATH, ENTITY_ASSETS_FILENAME, equipment_carrier::equipment_asset_path,
    };
    use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog};
    let world_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(DEFAULT_ASSET_PATH);
    let (Ok(entities), Ok(equipment)) = (
        std::fs::read(world_path.with_file_name(ENTITY_ASSETS_FILENAME)),
        std::fs::read(equipment_asset_path(&world_path)),
    ) else {
        eprintln!("local shield grip regression skipped: runtime carriers absent");
        eprintln!(
            "skipping installed_shield_bound_root_stays_at_each_hand_not_above_head: fixture unavailable; requires installed entity and equipment carriers (make assets)"
        );
        return;
    };
    let entities = RuntimeEntityAssets::decode(&entities).unwrap();
    let equipment = RuntimeEquipmentCatalog::decode(&equipment).unwrap();
    client_ui::test_support::assert_installed_shield(&entities, &equipment);
}
