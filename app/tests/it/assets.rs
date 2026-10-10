#![allow(dead_code)]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use ::assets::{
    AtmosphereRole, AtmosphereTexture, BlobProvenance, BlockFlags, BlockVisual, CompiledAssets,
    CompiledAtmosphereAssets, CompiledBiomeAssets, CompiledEntityAssets, EntityAssetKind,
    EntityAssetSource, EntityAssetSymbol, FontPixels, FontTexturePage, GlyphMetrics, Material,
    NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_atmosphere_blob, encode_blob, encode_entity_blob,
    encode_font_catalog,
};
use bedrock_client::args::{ClientArgs, ParseOutcome};
use diagnostics::metrics::{DiagnosticQuadTracker, MetricsCollector};
use meshing::{DiagnosticGeometryCount, DiagnosticGeometrySummary};
use sha2::{Digest, Sha256};
use {
    assets::pinned_world_provenance,
    bedrock_client::asset_startup::{
        ATMOSPHERE_COMPILE_COMMAND, ATMOSPHERE_FILENAME, AssetPathSource, COMPILE_COMMAND,
        DEFAULT_ASSET_PATH, ENTITY_ASSETS_COMPILE_COMMAND, ENTITY_ASSETS_FILENAME, FETCH_COMMAND,
        FONT_ASSETS_COMPILE_COMMAND, FONT_ASSETS_FILENAME, LOCAL_FONT_ASSETS_COMPILE_COMMAND,
        LOCAL_FONT_ASSETS_FILENAME, LoadedAssetKind, atmosphere_asset_path,
        atmosphere_shader_source_sha256, cloud_shader_source_sha256, entity_asset_path,
        font_asset_path, load_runtime_assets, local_font_asset_path, select_asset_path,
        select_asset_path_in_context,
    },
};

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

/// Complete synthetic identity for blobs that are decoded directly and never
/// startup-validated. These bytes never match a real pinned expectation.
const FIXTURE_PROVENANCE: BlobProvenance = BlobProvenance {
    source_manifest_sha256: [0xA5; 32],
    block_registry_sha256: [0x5A; 32],
    light_registry_sha256: [0x33; 32],
    biome_registry_sha256: [0x3C; 32],
};

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
            support: ::assets::VisualSupport::Exact,
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
                ..assets::Material::unvaried()
            },
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 2, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: *pinned_world_provenance(),
    })
    .unwrap()
}

fn synthetic_atmosphere_blob(seed: u8) -> Box<[u8]> {
    synthetic_atmosphere_blob_with_manifest(seed, canonical_vanilla_source_manifest_sha256())
}

fn synthetic_atmosphere_blob_with_manifest(
    seed: u8,
    source_manifest_sha256: [u8; 32],
) -> Box<[u8]> {
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
        source_manifest_sha256,
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap()
}

fn synthetic_entity_blob(seed: u8) -> Box<[u8]> {
    synthetic_entity_blob_with_manifest(seed, canonical_vanilla_source_manifest_sha256())
}

fn synthetic_entity_blob_with_manifest(seed: u8, source_manifest_sha256: [u8; 32]) -> Box<[u8]> {
    encode_entity_blob(&CompiledEntityAssets {
        source_manifest_sha256,
        block_visual_count: 0,
        sources: vec![EntityAssetSource {
            path: "entity/allay.entity.json".into(),
            source_bytes: 1,
            source_sha256: [seed.wrapping_add(1); 32],
        }]
        .into_boxed_slice(),
        symbols: vec![EntityAssetSymbol {
            kind: EntityAssetKind::Entity,
            identifier: "minecraft:allay".into(),
            source_index: 0,
            dependencies: Box::new([]),
        }]
        .into_boxed_slice(),
        geometries: Box::new([]),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    })
    .unwrap()
}

fn synthetic_font_blob(seed: u8) -> Box<[u8]> {
    synthetic_font_blob_with_manifest(seed, canonical_cinnangles_font_source_manifest_sha256())
}

fn synthetic_font_blob_with_manifest(seed: u8, manifest_sha256: [u8; 32]) -> Box<[u8]> {
    let rgba8 = vec![seed, seed, seed, 255].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/default8.png".into(),
        source_bytes: 4,
        source_sha256: [seed; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        pixels: FontPixels::Rgba8(rgba8),
    };
    let glyphs = [GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    }];
    encode_font_catalog(manifest_sha256, &glyphs, &[page]).unwrap()
}

fn canonical_vanilla_source_manifest_sha256() -> [u8; 32] {
    let source = include_str!("../../../assets/vanilla-source.json").replace("\r\n", "\n");
    Sha256::digest(source.as_bytes()).into()
}

fn canonical_cinnangles_font_source_manifest_sha256() -> [u8; 32] {
    let source = include_str!("../../../assets/cinnangles-sans-source.json").replace("\r\n", "\n");
    Sha256::digest(source.as_bytes()).into()
}

fn write_sibling_atmosphere(world_asset_path: &Path, seed: u8) -> PathBuf {
    let path = atmosphere_asset_path(world_asset_path);
    fs::write(&path, synthetic_atmosphere_blob(seed)).unwrap();
    write_sibling_entity(world_asset_path, seed.wrapping_add(0x40));
    path
}

fn write_sibling_entity(world_asset_path: &Path, seed: u8) -> PathBuf {
    let path = entity_asset_path(world_asset_path);
    fs::write(&path, synthetic_entity_blob(seed)).unwrap();
    fs::write(
        font_asset_path(world_asset_path),
        synthetic_font_blob(seed.wrapping_add(1)),
    )
    .unwrap();
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
fn default_asset_path_falls_back_to_the_executable_project_root() {
    let directory = temporary_directory("executable-root-assets");
    let current_directory = directory.join("unrelated-launch-directory");
    let project_root = directory.join("project");
    let executable = project_root.join("target/debug/bedrock-client.exe");
    let expected = project_root.join(DEFAULT_ASSET_PATH);
    fs::create_dir_all(expected.parent().unwrap()).unwrap();
    fs::write(&expected, b"compiled asset placeholder").unwrap();

    let selected = select_asset_path_in_context(None, None, &current_directory, &executable);

    assert_eq!(selected.source, AssetPathSource::Default);
    assert_eq!(selected.path, expected);
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
    let entity_path = write_sibling_entity(&path, 0x41);
    let expected_hash = format!("{:x}", Sha256::digest(&bytes));

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();

    assert_eq!(loaded.kind, LoadedAssetKind::CompiledBlob);
    assert_eq!(Arc::strong_count(&loaded.runtime), 1);
    let pinned = ::assets::vanilla_source();
    assert_eq!(loaded.metrics.source_tag, pinned.tag.as_ref());
    assert_eq!(loaded.metrics.source_sha256, pinned.sha256.as_ref());
    assert_eq!(loaded.metrics.blob_sha256, expected_hash);
    assert_eq!(loaded.metrics.texture_layers, 2);
    assert_eq!(loaded.metrics.texture_bytes_including_mips, 2_728);
    assert_eq!(loaded.metrics.material_count, 2);
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.atmosphere.selected_path(), atmosphere_path);
    assert_eq!(loaded.entities.selected_path(), entity_path);
    assert_eq!(loaded.entities.runtime().sources().len(), 1);
    assert_eq!(loaded.entities.runtime().symbols().len(), 1);
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
fn atmosphere_live_evidence_distinguishes_envelopes_and_is_stable_across_loads() {
    let directory = temporary_directory("atmosphere-evidence");
    let world_path = directory.join("vanilla.mcbea");
    fs::write(&world_path, synthetic_blob()).unwrap();

    let first_blob = synthetic_atmosphere_blob(0x61);
    fs::write(atmosphere_asset_path(&world_path), &first_blob).unwrap();
    write_sibling_entity(&world_path, 0x51);
    let first = load_runtime_assets(select_asset_path(Some(&world_path), None)).unwrap();
    let first_evidence = first.atmosphere.evidence();
    let repeated = load_runtime_assets(select_asset_path(Some(&world_path), None)).unwrap();
    let repeated_evidence = repeated.atmosphere.evidence();
    assert_eq!(first_evidence, repeated_evidence);
    assert_eq!(
        first_evidence.envelope_sha256,
        format!("{:x}", Sha256::digest(&first_blob))
    );

    let second_blob = synthetic_atmosphere_blob(0x62);
    fs::write(atmosphere_asset_path(&world_path), &second_blob).unwrap();
    let second = load_runtime_assets(select_asset_path(Some(&world_path), None)).unwrap();
    let second_evidence = second.atmosphere.evidence();
    assert_ne!(
        first_evidence.envelope_sha256,
        second_evidence.envelope_sha256
    );
    assert_eq!(
        first_evidence.shader_source_sha256,
        second_evidence.shader_source_sha256
    );

    let summary = second.atmosphere.startup_summary();
    let envelope_marker = format!("envelope_sha256={}", second_evidence.envelope_sha256);
    let shader_marker = format!(
        "shader_source_sha256={}",
        second_evidence.shader_source_sha256
    );
    assert!(summary.contains(&envelope_marker), "{summary}");
    assert!(summary.contains(&shader_marker), "{summary}");
    assert!(
        summary.find(&envelope_marker).unwrap() < summary.find(&shader_marker).unwrap(),
        "identity fields must retain one stable order: {summary}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn atmosphere_evidence_summary_contains_only_stable_hashes() {
    let directory = temporary_directory("machine-specific-atmosphere-evidence-path");
    let world_path = directory.join("local-vanilla.mcbea");
    fs::write(&world_path, synthetic_blob()).unwrap();
    let atmosphere_blob = synthetic_atmosphere_blob(0x63);
    fs::write(atmosphere_asset_path(&world_path), &atmosphere_blob).unwrap();
    write_sibling_entity(&world_path, 0x52);

    let loaded = load_runtime_assets(select_asset_path(Some(&world_path), None)).unwrap();
    let selected_path = loaded.atmosphere.selected_path().display().to_string();
    let evidence = loaded.atmosphere.evidence();
    let summary = loaded.atmosphere.startup_summary();

    assert!(
        !summary.contains(&selected_path),
        "selected path leaked: {summary}"
    );
    assert_eq!(
        summary,
        format!(
            "ATMOSPHERE_EVIDENCE envelope_sha256={} shader_source_sha256={} cloud_shader_source_sha256={}",
            evidence.envelope_sha256,
            evidence.shader_source_sha256,
            evidence.cloud_shader_source_sha256
        )
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn atmosphere_shader_identity_hashes_the_exact_embedded_wgsl_source() {
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../../crates/render/src/atmosphere.wgsl"))
    );
    assert_eq!(atmosphere_shader_source_sha256(), expected);
}

#[test]
fn cloud_shader_identity_hashes_the_exact_embedded_wgsl_source() {
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../../crates/render/src/cloud.wgsl"))
    );
    assert_eq!(cloud_shader_source_sha256(), expected);
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
    let mut diagnostics = DiagnosticQuadTracker::default();
    diagnostics.upsert(
        world::SubChunkKey::new(0, 1, 2, 3),
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    collector.record_diagnostic_attribution(diagnostics.snapshot());

    let report = collector.report();
    assert_eq!(report.assets.missing_mapping_count, 7);
    assert_eq!(report.assets.diagnostic_quad_count, 11);
    assert_eq!(report.assets.diagnostic_attribution.total_quad_count, 6);
    assert_eq!(
        report.assets.diagnostic_attribution.top[0].name,
        "minecraft:leaf_litter"
    );
    let marker = report.assets.world_ready_marker(19, 17);
    assert!(marker.starts_with("WORLD_READY "));
    let expected_blob_hash = format!("blob_sha256={}", report.assets.blob_sha256);
    assert!(marker.contains(&expected_blob_hash), "{marker}");
    let pinned = ::assets::vanilla_source();
    let source_tag = format!("source_tag={}", pinned.tag);
    let source_sha256 = format!("source_sha256={}", pinned.sha256);
    for expected in [
        source_tag.as_str(),
        source_sha256.as_str(),
        "resident_sub_chunks=19",
        "visible_sub_chunks=17",
        "diagnostic_attribution_total=6",
        "diagnostic_attribution_top=54|0x200a28f1|minecraft:leaf_litter|6",
        "diagnostic_attribution_omitted_identities=0",
        "diagnostic_attribution_omitted_quads=0",
    ] {
        assert!(marker.contains(expected), "{marker}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_commands_target_only_ignored_local_asset_paths() {
    assert_eq!(FETCH_COMMAND, "make vanilla-assets");
    assert_eq!(COMPILE_COMMAND, "make world-assets");
    assert!(Path::new(DEFAULT_ASSET_PATH).starts_with(".local/assets"));
    assert_eq!(ATMOSPHERE_FILENAME, "vanilla-v1.mcbeatm");
    assert_eq!(ATMOSPHERE_COMPILE_COMMAND, "make atmosphere-assets");
    assert_eq!(ENTITY_ASSETS_FILENAME, "vanilla-v1.mcbeent");
    assert_eq!(ENTITY_ASSETS_COMPILE_COMMAND, "make entity-assets");
    assert_eq!(FONT_ASSETS_FILENAME, "ui-cinnangles-sans-v1.mcbefont");
    assert_eq!(FONT_ASSETS_COMPILE_COMMAND, "make font-assets");
    assert_eq!(LOCAL_FONT_ASSETS_FILENAME, "vanilla-v1.mcbefont");
    assert_eq!(
        LOCAL_FONT_ASSETS_COMPILE_COMMAND,
        "make font-assets-local FONT_PACK_DIR=<reviewed-font-pack>"
    );
    assert_eq!(
        atmosphere_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbeatm")
    );
    assert_eq!(
        entity_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbeent")
    );
    assert_eq!(
        font_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/ui-cinnangles-sans-v1.mcbefont")
    );
    assert_eq!(
        local_font_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbefont")
    );
}

#[test]
fn required_entity_carrier_missing_fails_closed_with_actionable_path() {
    let directory = temporary_directory("missing-entity-assets");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x74),
    )
    .unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    let expected = directory.join(ENTITY_ASSETS_FILENAME);
    assert!(
        message.contains(&expected.display().to_string()),
        "{message}"
    );
    assert!(message.contains("required entity"), "{message}");
    assert!(message.contains(ENTITY_ASSETS_COMPILE_COMMAND), "{message}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_required_entity_carrier_fails_closed_with_rebuild_command() {
    let directory = temporary_directory("malformed-entity-assets");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x75),
    )
    .unwrap();
    let entity_path = entity_asset_path(&path);
    fs::write(&entity_path, b"not MCBEENT3").unwrap();
    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains(&entity_path.display().to_string()),
        "{message}"
    );
    assert!(message.contains("decode"), "{message}");
    assert!(message.contains(ENTITY_ASSETS_COMPILE_COMMAND), "{message}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mismatched_entity_carrier_provenance_fails_closed_with_rebuild_command() {
    let directory = temporary_directory("mismatched-entity-provenance");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x76),
    )
    .unwrap();
    let entity_path = entity_asset_path(&path);
    fs::write(
        &entity_path,
        synthetic_entity_blob_with_manifest(0x77, [0x99; 32]),
    )
    .unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains(&entity_path.display().to_string()),
        "{message}"
    );
    assert!(message.contains("provenance"), "{message}");
    assert!(message.contains(ENTITY_ASSETS_COMPILE_COMMAND), "{message}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn canonical_entity_carrier_provenance_is_portable_across_checkout_line_endings() {
    let directory = temporary_directory("canonical-entity-provenance");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x78),
    )
    .unwrap();
    fs::write(
        entity_asset_path(&path),
        synthetic_entity_blob_with_manifest(0x79, canonical_vanilla_source_manifest_sha256()),
    )
    .unwrap();
    fs::write(font_asset_path(&path), synthetic_font_blob(0x7a)).unwrap();

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    assert_eq!(
        loaded.entities.runtime().source_manifest_sha256(),
        canonical_vanilla_source_manifest_sha256()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn missing_font_carrier_uses_the_bounded_builtin_diagnostic_font() {
    let directory = temporary_directory("missing-font-assets");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x7b),
    )
    .unwrap();
    fs::write(entity_asset_path(&path), synthetic_entity_blob(0x7c)).unwrap();

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    assert!(loaded.fonts.is_diagnostic());
    assert_eq!(loaded.fonts.selected_path(), font_asset_path(&path));
    let summary = loaded.fonts.startup_summary();
    assert!(summary.contains("using bounded diagnostic font fallback"));
    assert!(!summary.contains("loaded required font assets"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn explicit_local_font_carrier_takes_precedence_without_replacing_cinnangles() {
    let directory = temporary_directory("local-font-precedence");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x7d),
    )
    .unwrap();
    fs::write(entity_asset_path(&path), synthetic_entity_blob(0x7e)).unwrap();
    let cinnangles_path = font_asset_path(&path);
    fs::write(&cinnangles_path, synthetic_font_blob(0x7f)).unwrap();
    let local_path = path.with_file_name("vanilla-v1.mcbefont");
    fs::write(
        &local_path,
        synthetic_font_blob_with_manifest(0x80, canonical_vanilla_source_manifest_sha256()),
    )
    .unwrap();

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    assert_eq!(loaded.fonts.selected_path(), local_path);
    assert!(
        cinnangles_path.is_file(),
        "the Cinnangles Sans default must remain intact"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn bundled_cinnangles_font_carrier_is_bound_to_its_manifest() {
    let directory = temporary_directory("cinnangles-font-precedence");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x7d),
    )
    .unwrap();
    fs::write(entity_asset_path(&path), synthetic_entity_blob(0x7e)).unwrap();
    let cinnangles_path = font_asset_path(&path);
    fs::write(
        &cinnangles_path,
        synthetic_font_blob_with_manifest(0x81, canonical_cinnangles_font_source_manifest_sha256()),
    )
    .unwrap();

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    assert_eq!(loaded.fonts.selected_path(), cinnangles_path);

    // A Cinnangles carrier built from any other source fails closed instead of falling back.
    fs::write(
        &cinnangles_path,
        synthetic_font_blob_with_manifest(0x82, canonical_vanilla_source_manifest_sha256()),
    )
    .unwrap();
    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    assert!(
        error.to_string().contains(FONT_ASSETS_COMPILE_COMMAND),
        "{error}"
    );
    fs::remove_dir_all(directory).unwrap();
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
    fs::write(&atmosphere_path, b"not MCBEATM2").unwrap();

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
    let source = include_str!("../../src/app.rs");
    assert!(source.contains("loaded_assets.atmosphere.startup_summary()"));
    assert!(source.contains("loaded_assets.atmosphere.into_parts()"));
    assert!(source.contains(".insert_resource(AtmosphereTextureAssets::new("));
    assert_eq!(
        source
            .matches("loaded_assets.atmosphere.into_parts()")
            .count(),
        1,
        "the required MCBEATM2 runtime must move into render exactly once"
    );
}

include!("assets/provenance_tests.rs");
