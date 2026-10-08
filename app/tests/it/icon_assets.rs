use std::path::Path;

use bedrock_client::asset_startup::{
    DEFAULT_ASSET_PATH, icon_asset_path, icon_assets_rebuild_command, require_icon_assets,
};

#[test]
fn custom_world_icon_recovery_uses_the_exact_world_input() {
    let world = std::env::temp_dir().join(format!(
        "icon-recovery-{}-{}-selected world.mcbea",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let icon = icon_asset_path(&world);
    let failure = require_icon_assets(&world, include_str!("../../../assets/vanilla-source.json"))
        .unwrap_err();
    let message = failure.to_string();
    assert!(message.contains("ASSET_BLOB="));
    assert!(message.contains("selected world.mcbea"));
    assert!(message.contains("ICON_ASSET_BLOB="));
    assert!(message.contains("ICON_ASSET_REPORT="));
    assert!(
        !icon_assets_rebuild_command(&icon).contains(" ASSET_BLOB="),
        "public legacy path-only helper stays available"
    );
    assert_eq!(
        icon_assets_rebuild_command(&icon_asset_path(Path::new(DEFAULT_ASSET_PATH))),
        "make icon-assets"
    );
}
