#![allow(dead_code)]

#[path = "../src/args.rs"]
mod args;
#[path = "../src/asset_startup.rs"]
mod asset_startup;
#[path = "../src/metrics.rs"]
mod metrics;

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use ::assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NetworkIdMode,
    TextureArray, TextureMip, encode_blob,
};
use args::{ClientArgs, ParseOutcome};
use asset_startup::{
    AssetPathSource, COMPILE_COMMAND, DEFAULT_ASSET_PATH, FETCH_COMMAND, LoadedAssetKind,
    load_runtime_assets, select_asset_path,
};
use metrics::{DiagnosticQuadTracker, MetricsCollector};
use sha2::{Digest, Sha256};

fn temporary_directory(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-mcbe-assets-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn synthetic_blob() -> Box<[u8]> {
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .map(|size| {
            let bytes_per_layer = (size * size * 4) as usize;
            let mut rgba8 = vec![0_u8; bytes_per_layer * 2];
            rgba8[..bytes_per_layer].fill(0x11);
            rgba8[bytes_per_layer..].fill(0x77);
            TextureMip {
                size,
                rgba8: rgba8.into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    encode_blob(&CompiledAssets {
        visuals: vec![BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::CUBE_GEOMETRY,
        }]
        .into_boxed_slice(),
        hashed: vec![(0xdbf4_4120, 0)].into_boxed_slice(),
        materials: vec![
            Material { layer: 0, flags: 0 },
            Material { layer: 1, flags: 0 },
        ]
        .into_boxed_slice(),
        textures: TextureArray { layers: 2, mips },
        biomes: CompiledBiomeAssets::diagnostic(),
    })
    .unwrap()
}

#[test]
fn assets_flag_parses_and_cli_beats_environment_then_default() {
    let ParseOutcome::Run(args) =
        ClientArgs::parse_from(["bedrock-client", "--assets", "cli/vanilla.mcbea"]).unwrap()
    else {
        panic!("expected run arguments")
    };
    assert_eq!(args.assets, Some(PathBuf::from("cli/vanilla.mcbea")));

    let cli = select_asset_path(
        args.assets.as_deref(),
        Some(OsString::from("environment/vanilla.mcbea")),
    );
    assert_eq!(cli.path, PathBuf::from("cli/vanilla.mcbea"));
    assert_eq!(cli.source, AssetPathSource::CommandLine);

    let environment = select_asset_path(None, Some(OsString::from("environment/vanilla.mcbea")));
    assert_eq!(environment.path, PathBuf::from("environment/vanilla.mcbea"));
    assert_eq!(environment.source, AssetPathSource::Environment);

    let default = select_asset_path(None, Some(OsString::new()));
    assert_eq!(default.path, PathBuf::from(DEFAULT_ASSET_PATH));
    assert_eq!(default.source, AssetPathSource::Default);
}

#[test]
fn missing_blob_starts_with_diagnostic_assets_and_exact_local_commands() {
    let directory = temporary_directory("missing");
    let path = directory.join("missing.mcbea");
    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();

    assert_eq!(loaded.kind, LoadedAssetKind::Diagnostic);
    assert_eq!(Arc::strong_count(&loaded.runtime), 1);
    assert_eq!(loaded.metrics.texture_layers, 1);
    assert_eq!(loaded.metrics.material_count, 1);
    assert_eq!(loaded.metrics.texture_bytes_including_mips, 1_364);
    assert_eq!(loaded.metrics.blob_sha256, "diagnostic");
    let notice = loaded.notice.as_deref().unwrap();
    assert!(notice.contains(&path.display().to_string()));
    assert!(notice.contains(FETCH_COMMAND));
    assert!(notice.contains(COMPILE_COMMAND));
    assert!(
        loaded
            .runtime
            .resolve(NetworkIdMode::Sequential, 0)
            .is_known()
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_blob_failure_names_the_exact_selected_path() {
    let directory = temporary_directory("malformed");
    let path = directory.join("broken.mcbea");
    fs::write(&path, b"not a compiled asset blob").unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(message.contains(&path.display().to_string()), "{message}");
    assert!(message.contains("decode"), "{message}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn valid_blob_decodes_once_and_reports_identity_and_counts() {
    let directory = temporary_directory("valid");
    let path = directory.join("vanilla.mcbea");
    let bytes = synthetic_blob();
    fs::write(&path, &bytes).unwrap();
    let expected_hash = format!("{:x}", Sha256::digest(&bytes));

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();

    assert_eq!(loaded.kind, LoadedAssetKind::CompiledBlob);
    assert_eq!(Arc::strong_count(&loaded.runtime), 1);
    assert_eq!(loaded.metrics.source_tag, "v1.26.30.32-preview");
    assert_eq!(
        loaded.metrics.source_sha256,
        "12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c"
    );
    assert_eq!(loaded.metrics.blob_sha256, expected_hash);
    assert_eq!(loaded.metrics.texture_layers, 2);
    assert_eq!(loaded.metrics.texture_bytes_including_mips, 2_728);
    assert_eq!(loaded.metrics.material_count, 2);
    assert!(loaded.notice.is_none());
    assert!(
        loaded
            .runtime
            .resolve(NetworkIdMode::Sequential, 0)
            .is_known()
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn asset_metrics_flow_into_json_and_the_world_ready_marker() {
    let directory = temporary_directory("metrics");
    let path = directory.join("vanilla.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    let mut collector = MetricsCollector::with_asset_metrics(loaded.metrics);
    collector.record_asset_counters(7, 11);

    let report = collector.report();
    assert_eq!(report.assets.missing_mapping_count, 7);
    assert_eq!(report.assets.diagnostic_quad_count, 11);
    let marker = report.assets.world_ready_marker(19, 17);
    assert!(marker.starts_with("WORLD_READY "));
    let expected_blob_hash = format!("blob_sha256={}", report.assets.blob_sha256);
    assert!(marker.contains(&expected_blob_hash), "{marker}");
    for expected in [
        "source_tag=v1.26.30.32-preview",
        "source_sha256=12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c",
        "resident_sub_chunks=19",
        "visible_sub_chunks=17",
    ] {
        assert!(marker.contains(expected), "{marker}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostic_quad_tracker_keeps_the_current_resident_total() {
    let first = world::SubChunkKey::new(0, 1, 2, 3);
    let second = world::SubChunkKey::new(0, 4, 5, 6);
    let mut tracker = DiagnosticQuadTracker::default();

    tracker.upsert(first, 9);
    tracker.upsert(second, 4);
    assert_eq!(tracker.total(), 13);

    tracker.upsert(first, 2);
    assert_eq!(tracker.total(), 6);

    tracker.remove(second);
    assert_eq!(tracker.total(), 2);

    tracker.upsert(first, 0);
    assert_eq!(tracker.total(), 0);
    tracker.remove(first);
    assert_eq!(tracker.total(), 0);
}

#[test]
fn documented_commands_target_only_ignored_local_asset_paths() {
    assert_eq!(
        FETCH_COMMAND,
        "powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula"
    );
    assert_eq!(
        COMPILE_COMMAND,
        concat!(
            "cargo run -p assets --bin assetc -- compile ",
            "--pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack ",
            "--registry crates/assets/data/block-registry-v1001.bin ",
            "--out .local/assets/compiled/vanilla-v1001.mcbea"
        )
    );
    assert!(Path::new(DEFAULT_ASSET_PATH).starts_with(".local/assets"));
}
