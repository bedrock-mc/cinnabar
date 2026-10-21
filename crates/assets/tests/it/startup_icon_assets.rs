use std::path::Path;

use assets::carriers::startup::{
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
    let failure = require_icon_assets(&world, assets::VANILLA_SOURCE_MANIFEST).unwrap_err();
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

#[test]
fn source_inheritance_requires_raw_bindings_and_reports_a_rebuild_for_old_carriers() {
    let directory = tempfile::tempdir().unwrap();
    let world = directory.path().join("selected.mcbea");
    let path = icon_asset_path(&world);
    let manifest =
        assets::canonical_source_manifest_sha256(assets::VANILLA_SOURCE_MANIFEST.as_bytes());
    let sprite = assets::IconSprite {
        width: 1,
        height: 1,
        rgba8: std::sync::Arc::from([1, 2, 3, 255]),
    };
    std::fs::write(
        &path,
        assets::encode_icon_catalog(manifest, std::slice::from_ref(&sprite), &[]).unwrap(),
    )
    .unwrap();
    let failure = require_icon_assets(&world, assets::VANILLA_SOURCE_MANIFEST)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("raw source bindings"));
    assert!(failure.contains("make assets"));
    assert!(failure.contains(path.to_str().unwrap()));
    std::fs::write(
        &path,
        assets::encode_icon_catalog_with_sources(manifest, &[sprite], &[], &[], &[], &[]).unwrap(),
    )
    .unwrap();
    require_icon_assets(&world, assets::VANILLA_SOURCE_MANIFEST).unwrap();
}
