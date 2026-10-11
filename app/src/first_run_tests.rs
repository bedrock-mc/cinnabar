use std::path::Path;

/// Startup fails closed without these, so setup must refuse to finish without them too.
#[test]
fn every_carrier_startup_requires_is_required_by_the_carrier_table() {
    use crate::asset_startup::{
        atmosphere_asset_path, entity_asset_path, hud_asset_path, icon_asset_path, lang_asset_path,
    };
    let world = Path::new("compiled").join(assets::carriers::WORLD.output);
    let startup = [
        atmosphere_asset_path(&world),
        entity_asset_path(&world),
        hud_asset_path(&world),
        icon_asset_path(&world),
        lang_asset_path(&world),
        client_ui::ui_runtime::json_ui_assets::ui_asset_path(&world),
    ];
    for path in startup {
        let name = path.file_name().unwrap().to_str().unwrap();
        let carrier = assets::carriers::CARRIERS
            .iter()
            .find(|carrier| carrier.output == name)
            .unwrap_or_else(|| panic!("{name} is missing from the carrier table"));
        assert!(carrier.required, "{name} must be a required carrier");
    }
}
