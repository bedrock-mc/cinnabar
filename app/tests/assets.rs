#![allow(dead_code)]

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
    CompiledAtmosphereAssets, CompiledBiomeAssets, CompiledEntityAssets, EntityAssetKind,
    EntityAssetSource, EntityAssetSymbol, FontTexturePage, GlyphMetrics, Material, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, TextureArray, TextureMip,
    TexturePage, TextureRef, VisualKind, encode_atmosphere_blob, encode_blob, encode_entity_blob,
    encode_font_catalog,
};
use bedrock_client::args::{ClientArgs, ParseOutcome};
use bedrock_client::asset_startup::{
    ATMOSPHERE_COMPILE_COMMAND, ATMOSPHERE_FILENAME, AssetPathSource, COMPILE_COMMAND,
    DEFAULT_ASSET_PATH, ENTITY_ASSETS_COMPILE_COMMAND, ENTITY_ASSETS_FILENAME, FETCH_COMMAND,
    FONT_ASSETS_COMPILE_COMMAND, FONT_ASSETS_FILENAME, LoadedAssetKind, atmosphere_asset_path,
    atmosphere_shader_source_sha256, cloud_shader_source_sha256, entity_asset_path,
    font_asset_path, load_runtime_assets, select_asset_path, select_asset_path_in_context,
};
use bedrock_client::metrics::{DIAGNOSTIC_TOP_LIMIT, DiagnosticQuadTracker, MetricsCollector};
use client_world::{BackingBlockIdentity, BlockEntityVisualRoute, adjudicate_block_entity_visual};
use meshing::{DiagnosticGeometryCount, DiagnosticGeometrySummary};
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
    })
    .unwrap()
}

fn synthetic_font_blob(seed: u8) -> Box<[u8]> {
    let rgba8 = vec![seed, seed, seed, 255].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/default8.png".into(),
        source_bytes: 4,
        source_sha256: [seed; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        rgba8,
    };
    let glyphs = [GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    }];
    encode_font_catalog(canonical_vanilla_source_manifest_sha256(), &glyphs, &[page]).unwrap()
}

fn canonical_vanilla_source_manifest_sha256() -> [u8; 32] {
    let source = include_str!("../../assets/vanilla-source.json").replace("\r\n", "\n");
    Sha256::digest(source.as_bytes()).into()
}

fn block_entity_nbt(id: Option<&str>, fields: &[(u8, &str, u8)]) -> world::BlockEntityNbt {
    let mut bytes = vec![10, 0];
    if let Some(id) = id {
        bytes.extend([8, 2, b'i', b'd', id.len() as u8]);
        bytes.extend_from_slice(id.as_bytes());
    }
    for &(tag, name, value) in fields {
        bytes.extend([tag, name.len() as u8]);
        bytes.extend_from_slice(name.as_bytes());
        match tag {
            1 => bytes.push(value),
            3 => bytes.push(value << 1),
            _ => panic!("unsupported test tag"),
        }
    }
    bytes.push(0);
    world::BlockEntityNbt::decode_prefix(&bytes).unwrap().0
}

fn backing(sequential_id: u32, network_hash: u32) -> BackingBlockIdentity {
    let kind = if sequential_id == 846
        || matches!(
            sequential_id,
            1_936
                | 2_699..=2_702
                | 7_069..=7_080
                | 8_516
                | 13_947..=13_950
                | 14_587..=14_590
                | 15_143..=15_146
                | 15_321..=15_324
                | 15_688..=15_691
        ) {
        VisualKind::Cube
    } else if matches!(
        sequential_id,
        13..=28
            | 837..=842
            | 1_992..=1_997
            | 2_018..=2_023
            | 5_393..=5_398
            | 6_438..=6_453
            | 6_883..=6_888
            | 8_510..=8_515
            | 9_120..=9_125
            | 9_209..=9_214
            | 10_237..=10_252
            | 11_064..=11_079
            | 12_171..=12_186
            | 12_620..=12_635
            | 13_126..=13_141
            | 13_336..=13_347
            | 13_849..=13_864
            | 14_513..=14_528
            | 14_533..=14_548
            | 14_691..=14_722
            | 14_941..=14_946
            | 15_347..=15_352
    ) {
        VisualKind::Model
    } else {
        VisualKind::Diagnostic
    };
    let runtime = runtime_with_block_identity(sequential_id, network_hash, kind);
    BackingBlockIdentity::from_runtime(network_hash, NetworkIdMode::Hashed, &runtime)
}

fn runtime_with_block_identity(
    sequential_id: u32,
    network_hash: u32,
    kind: VisualKind,
) -> ::assets::RuntimeAssets {
    let flags = if kind == VisualKind::Cube {
        BlockFlags::CUBE_GEOMETRY
    } else {
        BlockFlags::empty()
    };
    let visual = BlockVisual {
        faces: [0; 6],
        flags,
        kind,
        contributor_role: ::assets::ContributorRole::Primary,
        model_template: if kind == VisualKind::Model {
            0
        } else {
            NO_MODEL_TEMPLATE
        },
        animation: NO_ANIMATION,
        variant: 0,
    };
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| TextureMip {
            size,
            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let count = sequential_id as usize + 1;
    let blob = encode_blob(&CompiledAssets {
        visuals: vec![visual; count].into_boxed_slice(),
        light_properties: vec![::assets::LightProperties::default(); count].into_boxed_slice(),
        hashed: vec![(network_hash, sequential_id)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: if kind == VisualKind::Model {
            vec![ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        model_quads: if kind == VisualKind::Model {
            vec![ModelQuad {
                positions: [[0; 3]; 4],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    })
    .unwrap();
    ::assets::RuntimeAssets::decode(&blob).unwrap()
}

#[test]
fn block_entity_visuals_classify_static_logical_deferred_and_unknown_routes() {
    let static_routes = [
        ("Barrel", 7069, 198_111_737),
        ("BlastFurnace", 15_143, 2_142_573_020),
        ("BlastFurnace", 13_947, 1_464_259_042),
        ("Furnace", 15_688, 3_463_497_305),
        ("Furnace", 14_587, 2_568_407_871),
        ("Smoker", 2_699, 3_435_179_109),
        ("Smoker", 15_321, 2_080_399_355),
    ];
    for (id, sequential_id, network_hash) in static_routes {
        assert!(matches!(
            adjudicate_block_entity_visual(
                &block_entity_nbt(Some(id), &[]),
                backing(sequential_id, network_hash),
            ),
            BlockEntityVisualRoute::ExistingBlockState {
                additional_refs: 0,
                ..
            }
        ));
    }

    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(Some("Jukebox"), &[]),
            backing(8_516, 1_605_519_270),
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw {
            additional_refs: 0,
            ..
        }
    ));
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 24), (1, "powered", 1)]),
            backing(1_936, 166_024_317),
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw {
            additional_refs: 0,
            ..
        }
    ));

    for (id, sequential_id, network_hash) in [
        ("Banner", 10_321, 909_112_222),
        ("Beacon", 846, 561_914_719),
        ("Bed", 13_095, 313_457_523),
        ("BrewingStand", 15_128, 1_742_101_107),
        ("Campfire", 10_421, 2_722_749_277),
        ("Chest", 14_039, 741_882_976),
        ("CopperGolemStatue", 2_648, 2_018_082_108),
        ("DecoratedPot", 13_157, 340_115_056),
        ("EnchantTable", 13_163, 1_230_080_101),
        ("EnderChest", 6_870, 1_106_211_301),
        ("GlowItemFrame", 1_047, 209_894_407),
        ("Hopper", 13_514, 3_036_911_681),
        ("ItemFrame", 6_477, 2_475_575_814),
        ("Lectern", 13_559, 3_354_404_216),
        ("Sign", 13, 1_222_845_996),
        ("Skull", 33, 235_396_048),
    ] {
        let source = block_entity_nbt(Some(id), &[(3, "note", 25), (1, "powered", 7)]);
        assert!(matches!(
            adjudicate_block_entity_visual(&source, backing(sequential_id, network_hash)),
            BlockEntityVisualRoute::Deferred { .. }
        ));
        assert!(matches!(
            adjudicate_block_entity_visual(&source, backing(1_936, 166_024_317)),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }

    for (source, identity) in [
        (block_entity_nbt(Some("Barrel"), &[]), backing(42, 84)),
        (
            block_entity_nbt(Some("Jukebox"), &[]),
            backing(1_936, 166_024_317),
        ),
        (
            block_entity_nbt(Some("NotAReviewedSource"), &[]),
            backing(7_069, 198_111_737),
        ),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&source, identity),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_deferred_routes_require_exact_backing_identity() {
    let beacon = block_entity_nbt(Some("Beacon"), &[]);
    let runtime = runtime_with_block_identity(846, 561_914_719, VisualKind::Cube);
    for identity in [
        BackingBlockIdentity::from_runtime(0, NetworkIdMode::Hashed, &runtime),
        BackingBlockIdentity::from_runtime(
            561_914_719,
            NetworkIdMode::Hashed,
            &runtime_with_block_identity(846, 561_914_719, VisualKind::Invisible),
        ),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&beacon, identity),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_fail_closed_at_every_note_identity_boundary() {
    let valid_backing = backing(1_936, 166_024_317);
    for fields in [
        vec![],
        vec![(1, "note", 0)],
        vec![(1, "powered", 0)],
        vec![(1, "note", 25), (1, "powered", 0)],
        vec![(1, "note", 24), (1, "powered", 2)],
        vec![(3, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (3, "powered", 1)],
        vec![(1, "note", 24), (1, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (1, "powered", 1), (1, "powered", 1)],
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&block_entity_nbt(None, &fields), valid_backing),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 0), (1, "powered", 0)]),
            backing(42, 84),
        ),
        BlockEntityVisualRoute::Unknown { .. }
    ));
}

#[test]
fn block_entity_visuals_route_digest_is_stable_and_identity_bound() {
    let first = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[]),
        backing(7_069, 198_111_737),
    );
    let repeated = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[(1, "irrelevant", 9)]),
        backing(7_069, 198_111_737),
    );
    let other_state = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[]),
        backing(7_070, 501_043_176),
    );
    assert_eq!(first.route_digest(), repeated.route_digest());
    assert_ne!(first.route_digest(), other_state.route_digest());

    let chest = block_entity_nbt(Some("Chest"), &[]);
    let runtime = runtime_with_block_identity(14_039, 741_882_976, VisualKind::Diagnostic);
    let sequential = adjudicate_block_entity_visual(
        &chest,
        BackingBlockIdentity::from_runtime(14_039, NetworkIdMode::Sequential, &runtime),
    );
    let hashed = adjudicate_block_entity_visual(
        &chest,
        BackingBlockIdentity::from_runtime(741_882_976, NetworkIdMode::Hashed, &runtime),
    );
    assert_eq!(sequential.route_digest(), hashed.route_digest());
}

#[test]
fn block_entity_visuals_route_digest_distinguishes_unresolved_hashed_states() {
    let source = block_entity_nbt(Some("Barrel"), &[]);
    let runtime = runtime_with_block_identity(7_069, 198_111_737, VisualKind::Cube);
    let first = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(1, NetworkIdMode::Hashed, &runtime),
    );
    let second = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(2, NetworkIdMode::Hashed, &runtime),
    );
    assert!(matches!(first, BlockEntityVisualRoute::Unknown { .. }));
    assert!(matches!(second, BlockEntityVisualRoute::Unknown { .. }));
    assert_ne!(first.route_digest(), second.route_digest());
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
        Sha256::digest(include_bytes!("../../crates/render/src/atmosphere.wgsl"))
    );
    assert_eq!(atmosphere_shader_source_sha256(), expected);
}

#[test]
fn cloud_shader_identity_hashes_the_exact_embedded_wgsl_source() {
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../crates/render/src/cloud.wgsl"))
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
    for expected in [
        "source_tag=v1.26.30.32-preview",
        "source_sha256=12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c",
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
fn diagnostic_quad_tracker_keeps_the_current_resident_total() {
    let first = world::SubChunkKey::new(0, 1, 2, 3);
    let second = world::SubChunkKey::new(0, 4, 5, 6);
    let mut tracker = DiagnosticQuadTracker::default();

    tracker.upsert(
        first,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(Some(54), 54, 9)]),
    );
    tracker.upsert(
        second,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(0),
            973_836_165,
            4,
        )]),
    );
    assert_eq!(tracker.total(), 13);

    tracker.upsert(
        first,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(0),
            973_836_165,
            2,
        )]),
    );
    assert_eq!(tracker.total(), 6);
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.top[0].sequential_id, Some(0));
    assert_eq!(snapshot.top[0].network_id, 973_836_165);
    assert_eq!(snapshot.top[0].name, "minecraft:cyan_terracotta");
    assert_eq!(snapshot.top[0].quad_count, 6);

    tracker.remove(second);
    assert_eq!(tracker.total(), 2);

    tracker.upsert(first, DiagnosticGeometrySummary::default());
    assert_eq!(tracker.total(), 0);
    tracker.remove(first);
    assert_eq!(tracker.total(), 0);
}

#[test]
fn diagnostic_tracker_reports_hashed_identity_with_canonical_name() {
    let key = world::SubChunkKey::new(0, 1, 2, 3);
    let mut tracker = DiagnosticQuadTracker::default();
    tracker.upsert(
        key,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_quad_count, 6);
    assert_eq!(snapshot.top.len(), 1);
    assert_eq!(snapshot.top[0].sequential_id, Some(54));
    assert_eq!(snapshot.top[0].network_id, 537_536_753);
    assert_eq!(snapshot.top[0].name, "minecraft:leaf_litter");
}

#[test]
fn diagnostic_top_reporting_is_bounded_and_deterministic() {
    let counts = (0..DIAGNOSTIC_TOP_LIMIT + 3)
        .rev()
        .map(|id| DiagnosticGeometryCount::new(Some(id as u32), id as u32, 1));
    let summary = DiagnosticGeometrySummary::from_counts(counts);
    let mut tracker = DiagnosticQuadTracker::default();
    tracker.upsert(world::SubChunkKey::new(0, 0, 0, 0), summary);

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.top.len(), DIAGNOSTIC_TOP_LIMIT);
    assert_eq!(snapshot.omitted_identity_count, 3);
    assert_eq!(snapshot.omitted_quad_count, 3);
    assert!(
        snapshot
            .top
            .windows(2)
            .all(|pair| pair[0].sequential_id < pair[1].sequential_id)
    );
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
            "cargo run -p asset-compiler --bin assetc -- compile ",
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
    assert_eq!(ENTITY_ASSETS_FILENAME, "vanilla-v1.mcbeent");
    assert_eq!(ENTITY_ASSETS_COMPILE_COMMAND, "make entity-assets");
    assert_eq!(FONT_ASSETS_FILENAME, "vanilla-v1.mcbefont");
    assert_eq!(
        FONT_ASSETS_COMPILE_COMMAND,
        "make font-assets FONT_PACK_DIR=<reviewed-font-pack>"
    );
    assert_eq!(
        atmosphere_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbeatm")
    );
    assert_eq!(
        entity_asset_path(Path::new(DEFAULT_ASSET_PATH)),
        PathBuf::from(".local/assets/compiled/vanilla-v1.mcbeent")
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
    let source = include_str!("../src/app.rs");
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
            "ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock crates/assets/Cargo.toml ",
            "crates/asset-compiler/Cargo.toml Makefile $(wildcard crates/assets/src/*.rs) ",
            "$(wildcard crates/assets/src/*/*.rs) $(wildcard crates/asset-compiler/src/*.rs) ",
            "$(wildcard crates/asset-compiler/src/*/*.rs) ",
            "$(wildcard crates/asset-compiler/src/*/*/*.rs)"
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
fn make_client_passes_no_vsync_only_when_requested() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    assert!(makefile.contains("NO_VSYNC ?= 0"));
    assert!(makefile.contains("$(if $(filter 1,$(NO_VSYNC)),--no-vsync)"));
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
        "CINNABAR_CLOUDS_PNG ?=",
        "Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png",
        "CLOUDS_OVERRIDE_PREREQUISITE = FORCE_CINNABAR_CLOUDS_OVERRIDE",
        "$(VANILLA_SOURCE_MANIFEST) $(CLOUDS_OVERRIDE_PREREQUISITE)",
        "FORCE_CINNABAR_CLOUDS_OVERRIDE:",
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
        "$(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override \"$(CINNABAR_CLOUDS_PNG)\")",
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
fn make_assets_and_client_refresh_the_entity_carrier_and_report() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "ENTITY_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeent",
        "ENTITY_ASSET_REPORT ?= .local/assets/compiled/entity-assets.json",
        concat!(
            "ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- ",
            "entity-assets --pack \"$(PACK_DIR)\" --source-manifest \"$(VANILLA_SOURCE_MANIFEST)\" ",
            "--out \"$(ENTITY_ASSET_BLOB)\" --report \"$(ENTITY_ASSET_REPORT)\""
        ),
        "entity-assets: $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
        "$(ENTITY_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)",
        "$(ENTITY_ASSET_REPORT): $(ENTITY_ASSET_BLOB)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
    ] {
        assert!(
            makefile.contains(contract),
            "missing entity asset Makefile contract: {contract}"
        );
    }
    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .unwrap();
    assert!(phony.split_whitespace().any(|word| word == "entity-assets"));
    assert!(
        !phony
            .split_whitespace()
            .any(|word| { word == "$(ENTITY_ASSET_BLOB)" || word == "$(ENTITY_ASSET_REPORT)" })
    );
}

#[test]
fn make_exposes_explicit_font_compilation_without_blocking_default_launch() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "FONT_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbefont",
        "FONT_ASSET_REPORT ?= .local/assets/compiled/font-assets.json",
        "FONT_PACK_DIR ?= .local/assets/font-source",
        concat!(
            "FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- ",
            "font-assets --pack \"$(FONT_PACK_DIR)\" --source-manifest \"$(VANILLA_SOURCE_MANIFEST)\" ",
            "--out \"$(FONT_ASSET_BLOB)\" --report \"$(FONT_ASSET_REPORT)\""
        ),
        "font-assets: $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)",
        "$(FONT_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)",
        "$(FONT_ASSET_REPORT): $(FONT_ASSET_BLOB)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
    ] {
        assert!(
            makefile.contains(contract),
            "missing font asset Makefile contract: {contract}"
        );
    }
    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .unwrap();
    assert!(phony.split_whitespace().any(|word| word == "font-assets"));
    for default_target in ["assets:", "client:"] {
        let line = makefile
            .lines()
            .find(|line| line.starts_with(default_target))
            .unwrap();
        assert!(!line.contains("FONT_ASSET"));
    }
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

    let clouds_override = temporary.join("clouds.png");
    fs::write(&clouds_override, b"synthetic override prerequisite").unwrap();
    let mut override_assignments = assignments.to_vec();
    override_assignments.push(format!(
        "CINNABAR_CLOUDS_PNG={}",
        make_path(&clouds_override)
    ));
    run_make_atmosphere(root, &override_assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 4);

    let mut default_assignments = assignments.to_vec();
    default_assignments.push("CINNABAR_CLOUDS_PNG=".to_owned());
    run_make_atmosphere(root, &default_assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 5);

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

#[test]
fn app_composition_layout_remains_split_by_runtime_and_acceptance_owner() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let main = fs::read_to_string(source_root.join("main.rs")).unwrap();
    let library = fs::read_to_string(source_root.join("lib.rs")).unwrap();
    let app = fs::read_to_string(source_root.join("app.rs")).unwrap();

    assert!(library.contains("pub use app::run;"));
    assert!(app.contains("pub fn run(args: args::ClientArgs) -> Result<()>"));
    assert!(main.contains("run(*args)"));
    assert!(!main.contains("add_systems"));

    for relative in [
        "runtime/endpoint.rs",
        "runtime/shutdown.rs",
        "runtime/network.rs",
        "runtime/world.rs",
        "runtime/visibility.rs",
        "runtime/telemetry.rs",
        "acceptance/world_ready.rs",
        "acceptance/teleport.rs",
        "acceptance/remesh.rs",
        "acceptance/mutation.rs",
        "acceptance/proofs.rs",
        "acceptance/markers.rs",
    ] {
        assert!(source_root.join(relative).is_file(), "missing {relative}");
    }
}
