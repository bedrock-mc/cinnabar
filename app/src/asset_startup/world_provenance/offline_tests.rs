//! Optional replay of the owner's installed world carriers, without touching their bytes.

use std::path::Path;

use assets::{BlockFace, NetworkIdMode, RuntimeAssets, VisualKind, VisualSupport};

use super::{active_content_registry_protocol, pinned_block_registry_bytes, verify_world_carrier};
use crate::asset_startup::{
    AssetStartupError, DEFAULT_ASSET_PATH, load_runtime_assets, select_asset_path,
};

#[test]
fn installed_lifeboat_carriers_reject_stale_diagnostic_geometry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let active_path = root.join(DEFAULT_ASSET_PATH);
    // This legacy filename and runtime ID identify the owner's captured failure fixture.
    let stale_path = active_path.with_file_name("vanilla-v2168.mcbea");
    if !active_path.is_file() || !stale_path.is_file() {
        eprintln!("skipping Lifeboat carrier replay: installed active/legacy carriers absent");
        return;
    }
    let observed_id = 15844;
    let records = assets::read_registry_for_protocol(
        pinned_block_registry_bytes(),
        active_content_registry_protocol(),
    )
    .unwrap();
    let record = records
        .iter()
        .find(|record| record.sequential_id == observed_id)
        .unwrap();
    assert_eq!(record.name.as_ref(), "minecraft:mushroom_stem");

    let active = decode_installed(&active_path);
    verify_world_carrier(&active_path, &active).expect("active installed carrier matches checkout");
    let block = active.resolve(NetworkIdMode::Sequential, observed_id);
    assert_eq!(block.kind(), VisualKind::Cube);
    assert_eq!(block.support(), VisualSupport::Exact);
    assert_ne!(
        block.face(BlockFace::Up).material_id(),
        assets::DIAGNOSTIC_MATERIAL
    );

    let error = load_runtime_assets(select_asset_path(Some(&stale_path), None))
        .expect_err("startup must reject the owner's stale world carrier");
    match &error {
        AssetStartupError::Decode { source, .. } => assert!(matches!(
            source.as_ref(),
            assets::AssetError::InvalidCompiledAssets { detail } if detail.contains("magic")
        )),
        AssetStartupError::WorldAssetsProvenance { .. } => {}
        _ => panic!("unexpected startup rejection: {error}"),
    }
    assert!(
        error
            .to_string()
            .contains(&stale_path.display().to_string())
    );
    eprintln!(
        "LIFEBOAT_CARRIER_REPLAY id={observed_id} state={} active={:?}/{:?} active_air={:?} rejected={error}",
        record.canonical_state,
        block.kind(),
        block.support(),
        active.air_network_id(NetworkIdMode::Sequential),
    );
}

/// Reads a local carrier through the worktree path without rewriting installed assets.
fn decode_installed(path: &Path) -> RuntimeAssets {
    RuntimeAssets::decode(&std::fs::read(path).unwrap()).unwrap()
}
