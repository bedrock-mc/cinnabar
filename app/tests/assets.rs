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
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};

use ::assets::{
    AtmosphereRole, AtmosphereTexture, BlockFlags, BlockVisual, CompiledAssets,
    CompiledAtmosphereAssets, CompiledBiomeAssets, Material, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    encode_atmosphere_blob, encode_blob,
};
use args::{ClientArgs, ParseOutcome};
use asset_startup::{
    ATMOSPHERE_COMPILE_COMMAND, ATMOSPHERE_FILENAME, AssetPathSource, COMPILE_COMMAND,
    DEFAULT_ASSET_PATH, FETCH_COMMAND, LoadedAssetKind, atmosphere_asset_path, load_runtime_assets,
    select_asset_path,
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
            kind: VisualKind::Cube,
            contributor_role: ::assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }]
        .into_boxed_slice(),
        light_properties: vec![::assets::LightProperties::new(0, 15).unwrap()].into_boxed_slice(),
        hashed: vec![(0xdbf4_4120, 0)].into_boxed_slice(),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            },
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 2, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    })
    .unwrap()
}

fn synthetic_atmosphere_blob(seed: u8) -> Box<[u8]> {
    let textures = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, source_path, width, height))| {
        let rgba8 = vec![seed.wrapping_add(index as u8); (width * height * 4) as usize];
        AtmosphereTexture {
            role,
            source_path: source_path.into(),
            source_bytes: 1,
            source_sha256: [index as u8 + 1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    encode_atmosphere_blob(&CompiledAtmosphereAssets {
        source_manifest_sha256: [0x77; 32],
        textures,
    })
    .unwrap()
}

fn write_sibling_atmosphere(world_asset_path: &Path, seed: u8) -> PathBuf {
    let path = atmosphere_asset_path(world_asset_path);
    fs::write(&path, synthetic_atmosphere_blob(seed)).unwrap();
    path
}

#[test]
fn workspace_consumers_accept_empty_new_tables() {
    let runtime = ::assets::RuntimeAssets::decode(&synthetic_blob()).expect("decode MCBEAS05");
    assert!(runtime.model_templates().is_empty());
    assert!(runtime.model_quads().is_empty());
    assert!(runtime.animations().is_empty());
    assert!(runtime.animation_frames().is_empty());
    assert_eq!(runtime.texture_pages().len(), 1);
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
    write_sibling_atmosphere(&path, 0x30);
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
    let (atmosphere, _) = loaded.atmosphere.into_parts();
    assert_eq!(Arc::strong_count(&atmosphere), 1);

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
    assert!(message.contains("rebuild"), "{message}");
    assert!(message.contains(COMPILE_COMMAND), "{message}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn valid_blob_decodes_once_and_reports_identity_and_counts() {
    let directory = temporary_directory("valid");
    let path = directory.join("vanilla.mcbea");
    let bytes = synthetic_blob();
    fs::write(&path, &bytes).unwrap();
    let atmosphere_bytes = synthetic_atmosphere_blob(0x40);
    let atmosphere_path = atmosphere_asset_path(&path);
    fs::write(&atmosphere_path, &atmosphere_bytes).unwrap();
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
    assert_eq!(loaded.atmosphere.selected_path(), atmosphere_path);
    assert!(
        loaded
            .runtime
            .resolve(NetworkIdMode::Sequential, 0)
            .is_known()
    );
    let (atmosphere, atmosphere_identity) = loaded.atmosphere.into_parts();
    assert_eq!(
        atmosphere_identity,
        <[u8; 32]>::from(Sha256::digest(&atmosphere_bytes))
    );
    assert_eq!(Arc::strong_count(&atmosphere), 1);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn asset_metrics_flow_into_json_and_the_world_ready_marker() {
    let directory = temporary_directory("metrics");
    let path = directory.join("vanilla.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    write_sibling_atmosphere(&path, 0x50);
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
            "--light-registry crates/assets/data/block-light-registry-v1001.bin ",
            "--biome-registry crates/assets/data/biome-registry-v1001.bin ",
            "--out .local/assets/compiled/vanilla-v1001.mcbea"
        )
    );
    assert!(Path::new(DEFAULT_ASSET_PATH).starts_with(".local/assets"));
    assert_eq!(ATMOSPHERE_FILENAME, "vanilla-v1.mcbeatm");
    assert_eq!(ATMOSPHERE_COMPILE_COMMAND, "make atmosphere-assets");
    assert_eq!(
        atmosphere_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbeatm")
    );
}

#[test]
fn required_atmosphere_carrier_missing_fails_closed_with_actionable_path() {
    let directory = temporary_directory("missing-atmosphere");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    let expected = directory.join(ATMOSPHERE_FILENAME);
    assert!(
        message.contains(&expected.display().to_string()),
        "{message}"
    );
    assert!(message.contains("required atmosphere"), "{message}");
    assert!(message.contains(ATMOSPHERE_COMPILE_COMMAND), "{message}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_required_atmosphere_carrier_fails_closed_with_rebuild_command() {
    let directory = temporary_directory("malformed-atmosphere");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    let atmosphere_path = atmosphere_asset_path(&path);
    fs::write(&atmosphere_path, b"not MCBEATM1").unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains(&atmosphere_path.display().to_string()),
        "{message}"
    );
    assert!(message.contains("decode"), "{message}");
    assert!(message.contains(ATMOSPHERE_COMPILE_COMMAND), "{message}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn startup_hands_the_single_decoded_atmosphere_identity_to_the_renderer() {
    let source = include_str!("../src/main.rs");
    assert!(source.contains("loaded_assets.atmosphere.into_parts()"));
    assert!(source.contains(".insert_resource(AtmosphereTextureAssets::new("));
    assert_eq!(
        source
            .matches("loaded_assets.atmosphere.into_parts()")
            .count(),
        1,
        "the required MCBEATM1 runtime must move into render exactly once"
    );
}

#[test]
fn make_client_rebuilds_only_a_missing_or_stale_asset_blob() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v1001.bin",
        concat!(
            "ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock crates/assets/Cargo.toml Makefile ",
            "$(wildcard crates/assets/src/*.rs) $(wildcard crates/assets/src/bin/*.rs)"
        ),
        concat!(
            "$(ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(BLOCK_REGISTRY) ",
            "$(LIGHT_REGISTRY) $(BIOME_REGISTRY)"
        ),
        "assets: $(ASSET_BLOB)",
        "client: $(ASSET_BLOB)",
        "--light-registry \"$(LIGHT_REGISTRY)\"",
    ] {
        assert!(
            makefile.contains(contract),
            "missing Makefile contract: {contract}"
        );
    }

    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .expect("Makefile has a .PHONY declaration");
    assert!(!phony.split_whitespace().any(|word| word == "$(ASSET_BLOB)"));
}

#[test]
fn make_assets_and_client_refresh_the_atmosphere_blob_and_report() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "VANILLA_SOURCE_MANIFEST ?= assets/vanilla-source.json",
        "ATMOSPHERE_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeatm",
        "ATMOSPHERE_REPORT ?= .local/assets/compiled/atmosphere-assets.json",
        concat!(
            "$(ATMOSPHERE_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) ",
            "$(VANILLA_SOURCE_MANIFEST)"
        ),
        "$(ATMOSPHERE_REPORT): $(ATMOSPHERE_BLOB)",
        concat!(
            "\t@if [ ! -f \"$@\" ] || [ \"$@\" -ot \"$<\" ]; then ",
            "$(ATMOSPHERE_COMPILE); fi"
        ),
        "\t$(ATMOSPHERE_COMPILE)",
        "atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "--source-manifest \"$(VANILLA_SOURCE_MANIFEST)\"",
        "--out \"$(ATMOSPHERE_BLOB)\" --report \"$(ATMOSPHERE_REPORT)\"",
    ] {
        assert!(
            makefile.contains(contract),
            "missing atmosphere Makefile contract: {contract}"
        );
    }

    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .expect("Makefile has a .PHONY declaration");
    assert!(
        phony
            .split_whitespace()
            .any(|word| word == "atmosphere-assets")
    );
    assert!(
        !phony
            .split_whitespace()
            .any(|word| word == "$(ATMOSPHERE_BLOB)" || word == "$(ATMOSPHERE_REPORT)")
    );
    assert!(!makefile.contains("$(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT):"));
    assert_eq!(
        makefile
            .lines()
            .filter(|line| line.starts_with('\t') && line.contains("$(ATMOSPHERE_COMPILE)"))
            .count(),
        2,
        "blob and missing-report recovery must use one shared producer command"
    );
}

#[test]
fn make_atmosphere_target_serializes_one_producer_for_missing_and_stale_pairs() {
    let make_available = match Command::new("make").arg("--version").output() {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            eprintln!(
                "skipping executable Makefile test: `make --version` failed with {}",
                output.status
            );
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping executable Makefile test: `make` is unavailable");
            false
        }
        Err(error) => panic!("failed to probe make: {error}"),
    };
    if !make_available {
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = temporary_directory("make-atmosphere-behavior");
    let world = temporary.join("world.mcbea");
    let block = temporary.join("block.bin");
    let light = temporary.join("light.bin");
    let biome = temporary.join("biome.bin");
    let manifest = temporary.join("vanilla-source.json");
    let atmosphere = temporary.join("atmosphere.mcbeatm");
    let report = temporary.join("atmosphere.json");
    let invocations = temporary.join("invocations.log");
    for prerequisite in [&block, &light, &biome] {
        fs::write(prerequisite, b"registry").unwrap();
    }
    fs::write(&world, b"world").unwrap();
    fs::copy(root.join("assets/vanilla-source.json"), &manifest).unwrap();
    let now = SystemTime::now();
    for prerequisite in [&block, &light, &biome, &manifest] {
        fs::File::options()
            .write(true)
            .open(prerequisite)
            .unwrap()
            .set_modified(now - Duration::from_secs(120))
            .unwrap();
    }
    fs::File::options()
        .write(true)
        .open(&world)
        .unwrap()
        .set_modified(now - Duration::from_secs(60))
        .unwrap();

    let producer = format!(
        "echo invocation >> \"{}\" && echo blob > \"{}\" && echo report > \"{}\"",
        make_path(&invocations),
        make_path(&atmosphere),
        make_path(&report)
    );
    let assignments = [
        "ASSET_COMPILER_INPUTS=".to_owned(),
        format!("ASSET_BLOB={}", make_path(&world)),
        format!("BLOCK_REGISTRY={}", make_path(&block)),
        format!("LIGHT_REGISTRY={}", make_path(&light)),
        format!("BIOME_REGISTRY={}", make_path(&biome)),
        format!("VANILLA_SOURCE_MANIFEST={}", make_path(&manifest)),
        format!("ATMOSPHERE_BLOB={}", make_path(&atmosphere)),
        format!("ATMOSPHERE_REPORT={}", make_path(&report)),
        format!("ATMOSPHERE_COMPILE={producer}"),
    ];

    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 1);
    assert!(atmosphere.is_file() && report.is_file());

    fs::remove_file(&report).unwrap();
    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 2);
    assert!(atmosphere.is_file() && report.is_file());

    fs::File::options()
        .write(true)
        .open(&manifest)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(60))
        .unwrap();
    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 3);
    assert!(atmosphere.is_file() && report.is_file());

    fs::remove_dir_all(temporary).unwrap();
}

fn run_make_atmosphere(root: &Path, assignments: &[String]) {
    let output = Command::new("make")
        .current_dir(root)
        .args(["-f", "Makefile", "-j4", "atmosphere-assets"])
        .args(assignments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "make atmosphere-assets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn make_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
