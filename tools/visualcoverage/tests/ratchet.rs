use assets::{
    ANIMATION_FLAG_BLEND, Animation, BiomeRule, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, Material, ModelFamily, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, RegistryProvenance, RegistryRecord,
    RuntimeAssets, TINT_MAP_BYTES, TextureArray, TextureMip, TexturePage, TextureRef, TintSource,
    VisualKind, encode_blob, read_registry,
};
use visualcoverage::{
    AllowlistEntry, Baseline, Counts, CoverageError, RenderStream, StateIdentity, analyze_bytes,
    analyze_records, baseline_from_snapshot, deterministic_json, parse_baseline, ratchet,
    ratchet_protocol_1001, strict_records,
};

#[test]
fn committed_protocol_baseline_binds_the_complete_corpus_and_all_vines() {
    let baseline = parse_baseline(include_bytes!(
        "../../../crates/assets/data/visual-coverage-v1001.json"
    ))
    .expect("parse committed baseline");
    let records = read_registry(include_bytes!(
        "../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read committed registry");

    assert_eq!(
        baseline.counts,
        Counts {
            names: 1_356,
            states: 16_913,
            air: 1,
        }
    );
    assert_eq!(
        baseline.states,
        records
            .iter()
            .map(StateIdentity::from_record)
            .collect::<Vec<_>>()
    );
    assert!(baseline.expected_vine_diagnostic_masks.is_empty());
    assert!(
        records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:vine")
            .all(|record| !baseline
                .diagnostic_sequential_ids
                .contains(&record.sequential_id))
    );
}

fn fixture_records() -> Vec<RegistryRecord> {
    let all = read_registry(include_bytes!(
        "../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read production registry");
    let mut air = all
        .iter()
        .find(|record| record.flags.contains(BlockFlags::AIR))
        .expect("air")
        .clone();
    let mut stone = all
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .expect("stone")
        .clone();
    let mut vine = all
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:vine"
                && record.model_state.get(assets::ModelStateField::Connections) == Some(3)
        })
        .expect("vine mask 3")
        .clone();
    for (id, record) in [&mut air, &mut stone, &mut vine].into_iter().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 0x8000_1000 + id as u32;
    }
    vec![air, stone, vine]
}

fn texture_array(layers: u32) -> TextureArray {
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .map(|size| TextureMip {
            size,
            rgba8: vec![0x44; size as usize * size as usize * 4 * layers as usize]
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    TextureArray { layers, mips }
}

fn visual(kind: VisualKind) -> BlockVisual {
    BlockVisual {
        faces: match kind {
            VisualKind::Diagnostic | VisualKind::Invisible => [DIAGNOSTIC_MATERIAL; 6],
            _ => [1; 6],
        },
        flags: if kind == VisualKind::Cube {
            BlockFlags::CUBE_GEOMETRY
        } else {
            BlockFlags::empty()
        },
        kind,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

fn blob(records: &[RegistryRecord], kinds: &[VisualKind]) -> Vec<u8> {
    let mut hashed = records
        .iter()
        .map(|record| (record.network_hash, record.sequential_id))
        .collect::<Vec<_>>();
    hashed.sort_unstable();
    let compiled = CompiledAssets {
        visuals: kinds.iter().copied().map(visual).collect(),
        hashed: hashed.into_boxed_slice(),
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
        texture_pages: vec![TexturePage::new(texture_array(2))].into_boxed_slice(),
        biomes: CompiledBiomeAssets {
            tint_maps_rgb8: vec![0; TINT_MAP_BYTES].into_boxed_slice(),
            rules: vec![BiomeRule {
                id: 0,
                name: "minecraft:plains".into(),
                flags: 0,
                grass: TintSource::direct(0),
                foliage: TintSource::direct(0),
                dry_foliage: TintSource::direct(0),
                water: TintSource::direct(0),
                temperature_bits: 0,
                downfall_bits: 0,
            }]
            .into_boxed_slice(),
        },
    };
    encode_blob(&compiled).expect("encode fixture").into_vec()
}

fn registry_bytes(records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    let names = records
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    let valentine_names = records
        .iter()
        .filter(|record| record.provenance.contains(RegistryProvenance::VALENTINE))
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    let valentine_states = records
        .iter()
        .filter(|record| record.provenance.contains(RegistryProvenance::VALENTINE))
        .count() as u32;
    bytes.extend_from_slice(&names.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&valentine_names.to_le_bytes());
    bytes.extend_from_slice(&valentine_states.to_le_bytes());
    bytes.extend_from_slice(&(names - valentine_names).to_le_bytes());
    bytes.extend_from_slice(&((records.len() as u32) - valentine_states).to_le_bytes());
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(record.flags.bits());
        bytes.push(record.model_family as u8);
        bytes.push(record.contributor_role as u8);
        bytes.push(record.model_state.mask());
        bytes.push(record.face_coverage);
        bytes.push(record.collision_seed.confidence as u8);
        bytes.push(record.provenance.bits());
        bytes.push(record.collision_seed.boxes.len() as u8);
        bytes.extend_from_slice(&record.collision_seed.shape_id.to_le_bytes());
        bytes.extend_from_slice(&(record.name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(record.canonical_state.len() as u32).to_le_bytes());
        for field in [
            assets::ModelStateField::Orientation,
            assets::ModelStateField::Half,
            assets::ModelStateField::Open,
            assets::ModelStateField::Hinge,
            assets::ModelStateField::Connections,
            assets::ModelStateField::Growth,
            assets::ModelStateField::LiquidDepth,
            assets::ModelStateField::Flags,
        ] {
            bytes.extend_from_slice(&record.model_state.get(field).unwrap_or(0).to_le_bytes());
        }
        for collision_box in &record.collision_seed.boxes {
            for value in [
                collision_box.min_x,
                collision_box.min_y,
                collision_box.min_z,
                collision_box.max_x,
                collision_box.max_y,
                collision_box.max_z,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(record.name.as_bytes());
        bytes.extend_from_slice(record.canonical_state.as_bytes());
    }
    bytes
}

fn baseline(report: &visualcoverage::CoverageSnapshot) -> Baseline {
    Baseline {
        schema: "cinnabar-visual-coverage-baseline-v1".into(),
        protocol: 1001,
        registry_sha256: report.registry_sha256.clone(),
        counts: report.counts,
        states: report.states.clone(),
        diagnostic_sequential_ids: report
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist: Vec::new(),
        expected_vine_diagnostic_masks: vec![3],
    }
}

#[test]
fn exact_inventory_rejects_missing_duplicate_and_non_contiguous_ids() {
    let records = fixture_records();
    let kinds = [
        VisualKind::Invisible,
        VisualKind::Cube,
        VisualKind::Diagnostic,
    ];
    let good = analyze_bytes(&registry_bytes(&records), &blob(&records, &kinds)).unwrap();
    assert_eq!(
        good.counts,
        Counts {
            names: 3,
            states: 3,
            air: 1
        }
    );

    let mut missing = records.clone();
    missing.remove(1);
    assert!(matches!(
        visualcoverage::analyze_records(
            &missing,
            &RuntimeAssets::decode(&blob(&records, &kinds)).unwrap(),
            "r",
            "a"
        ),
        Err(CoverageError::NonContiguousSequentialId {
            expected: 1,
            actual: 2
        })
    ));

    let mut duplicate = records.clone();
    duplicate[2].sequential_id = 1;
    assert!(matches!(
        visualcoverage::analyze_records(
            &duplicate,
            &RuntimeAssets::decode(&blob(&records, &kinds)).unwrap(),
            "r",
            "a"
        ),
        Err(CoverageError::DuplicateSequentialId(1))
    ));

    let mut non_contiguous = records.clone();
    non_contiguous[2].sequential_id = 7;
    assert!(matches!(
        visualcoverage::analyze_records(
            &non_contiguous,
            &RuntimeAssets::decode(&blob(&records, &kinds)).unwrap(),
            "r",
            "a"
        ),
        Err(CoverageError::NonContiguousSequentialId {
            expected: 2,
            actual: 7
        })
    ));
}

#[test]
fn ratchet_rejects_registry_identity_changes_and_visual_blob_mismatch() {
    let records = fixture_records();
    let registry = registry_bytes(&records);
    let assets = blob(
        &records,
        &[
            VisualKind::Invisible,
            VisualKind::Cube,
            VisualKind::Diagnostic,
        ],
    );
    let current = analyze_bytes(&registry, &assets).unwrap();
    let expected = baseline(&current);

    let mut changed = records.clone();
    changed[1].canonical_state = r#"{"changed":true}"#.into();
    let error = ratchet(
        analyze_bytes(
            &registry_bytes(&changed),
            &blob(
                &changed,
                &[
                    VisualKind::Invisible,
                    VisualKind::Cube,
                    VisualKind::Diagnostic,
                ],
            ),
        )
        .unwrap(),
        &expected,
    )
    .unwrap_err();
    assert!(matches!(error, CoverageError::RegistryHashMismatch { .. }));

    let mut mismatched = records.clone();
    mismatched[1].network_hash = 0x9000_0000;
    let error = analyze_bytes(
        &registry,
        &blob(
            &mismatched,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CoverageError::LookupMismatch {
            sequential_id: 1,
            ..
        }
    ));

    let visual_orphan = analyze_bytes(
        &registry,
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
                VisualKind::Cube,
            ],
        ),
    )
    .unwrap_err();
    assert!(matches!(
        visual_orphan,
        CoverageError::RuntimeCardinalityMismatch {
            registry: 3,
            visuals: 4,
            hashes: 3,
        }
    ));

    let mut orphan_records = records.clone();
    let mut orphan = records[1].clone();
    orphan.sequential_id = 3;
    orphan.network_hash = 0x9fff_0003;
    orphan_records.push(orphan);
    let full_orphan = analyze_bytes(
        &registry,
        &blob(
            &orphan_records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
                VisualKind::Cube,
            ],
        ),
    )
    .unwrap_err();
    assert!(matches!(
        full_orphan,
        CoverageError::RuntimeCardinalityMismatch {
            registry: 3,
            visuals: 4,
            hashes: 4,
        }
    ));

    let mut swapped = records.clone();
    let first_hash = swapped[1].network_hash;
    swapped[1].network_hash = swapped[2].network_hash;
    swapped[2].network_hash = first_hash;
    let equal_visual_swap = analyze_bytes(
        &registry,
        &blob(
            &swapped,
            &[VisualKind::Invisible, VisualKind::Cube, VisualKind::Cube],
        ),
    )
    .unwrap_err();
    assert!(matches!(
        equal_visual_swap,
        CoverageError::LookupMismatch {
            sequential_id: 1,
            ..
        }
    ));
}

#[test]
fn diagnostic_regression_fails_and_shrinkage_is_exact() {
    let records = fixture_records();
    let registry = registry_bytes(&records);
    let initial = analyze_bytes(
        &registry,
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    let expected = baseline(&initial);

    let regression = analyze_bytes(
        &registry,
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Diagnostic,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    assert!(matches!(
        ratchet(regression, &expected),
        Err(CoverageError::DiagnosticRegression { .. })
    ));

    let shrink = analyze_bytes(
        &registry,
        &blob(
            &records,
            &[VisualKind::Invisible, VisualKind::Cube, VisualKind::Cube],
        ),
    )
    .unwrap();
    let report = ratchet(
        shrink,
        &Baseline {
            expected_vine_diagnostic_masks: vec![],
            ..expected
        },
    )
    .unwrap();
    assert_eq!(report.added_diagnostics, Vec::<StateIdentity>::new());
    assert_eq!(report.removed_diagnostics.len(), 1);
    assert_eq!(report.removed_diagnostics[0].name, "minecraft:vine");
}

#[test]
fn invisible_laundering_requires_a_source_cited_exact_allowlist_entry() {
    let records = fixture_records();
    let registry = registry_bytes(&records);
    let diagnostic_assets = blob(
        &records,
        &[
            VisualKind::Invisible,
            VisualKind::Cube,
            VisualKind::Diagnostic,
        ],
    );
    let initial = analyze_bytes(&registry, &diagnostic_assets).unwrap();
    let expected = baseline(&initial);

    let laundered = analyze_bytes(
        &registry,
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Invisible,
            ],
        ),
    )
    .unwrap();
    assert!(matches!(
        ratchet(
            laundered.clone(),
            &Baseline {
                expected_vine_diagnostic_masks: vec![],
                ..expected.clone()
            }
        ),
        Err(CoverageError::UnreviewedInvisible { .. })
    ));

    let vine = StateIdentity::from_record(&records[2]);
    let accepted = ratchet(
        laundered,
        &Baseline {
            invisible_allowlist: vec![AllowlistEntry {
                state: vine,
                authority: "Dragonfly protocol-1001 registry: vine is explicitly no-draw".into(),
                source: "https://github.com/df-mc/dragonfly".into(),
            }],
            expected_vine_diagnostic_masks: vec![],
            ..expected
        },
    )
    .unwrap();
    assert_eq!(
        accepted
            .invisible_decisions
            .iter()
            .filter(|decision| decision.allowed)
            .count(),
        2
    );
}

#[test]
fn vine_diagnostic_masks_are_an_explicit_exact_assertion() {
    let records = fixture_records();
    let snapshot = analyze_bytes(
        &registry_bytes(&records),
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    let mut expected = baseline(&snapshot);
    expected.expected_vine_diagnostic_masks = (0..16).collect();
    assert!(matches!(
        ratchet(snapshot, &expected),
        Err(CoverageError::VineDiagnosticsMismatch { .. })
    ));
}

#[test]
fn checked_json_is_byte_identical_and_sorted() {
    let records = fixture_records();
    let snapshot = analyze_bytes(
        &registry_bytes(&records),
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    let report = ratchet(
        snapshot,
        &baseline(
            &analyze_bytes(
                &registry_bytes(&records),
                &blob(
                    &records,
                    &[
                        VisualKind::Invisible,
                        VisualKind::Cube,
                        VisualKind::Diagnostic,
                    ],
                ),
            )
            .unwrap(),
        ),
    )
    .unwrap();
    let first = deterministic_json(&report).unwrap();
    let second = deterministic_json(&report).unwrap();
    assert_eq!(first, second);
    assert!(first.ends_with(b"\n"));
}

#[test]
fn production_ratchet_and_cli_reject_a_matching_but_noncanonical_small_corpus() {
    let records = fixture_records();
    let registry = registry_bytes(&records);
    let assets = blob(
        &records,
        &[
            VisualKind::Invisible,
            VisualKind::Cube,
            VisualKind::Diagnostic,
        ],
    );
    let expected = baseline(&analyze_bytes(&registry, &assets).unwrap());
    let directory = tempfile::tempdir().unwrap();
    let registry_path = directory.path().join("registry.bin");
    let assets_path = directory.path().join("assets.mcbea");
    let baseline_path = directory.path().join("baseline.json");
    let report_path = directory.path().join("report.json");
    std::fs::write(&registry_path, registry).unwrap();
    std::fs::write(&assets_path, assets).unwrap();
    std::fs::write(&baseline_path, deterministic_json(&expected).unwrap()).unwrap();

    assert!(matches!(
        ratchet_protocol_1001(
            analyze_bytes(
                &std::fs::read(&registry_path).unwrap(),
                &std::fs::read(&assets_path).unwrap(),
            )
            .unwrap(),
            &expected,
        ),
        Err(CoverageError::NonCanonicalProtocolInventory(_))
    ));

    let run = {
        std::process::Command::new(env!("CARGO_BIN_EXE_visualcoverage"))
            .args([
                "ratchet",
                "--registry",
                registry_path.to_str().unwrap(),
                "--assets",
                assets_path.to_str().unwrap(),
                "--baseline",
                baseline_path.to_str().unwrap(),
                "--out",
                report_path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    assert!(
        !run.status.success(),
        "noncanonical protocol corpus unexpectedly passed"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("NonCanonicalProtocolInventory"),
        "unexpected stderr: {stderr}"
    );
    assert!(!report_path.exists());
}

#[test]
fn baseline_generation_rejects_a_noncanonical_protocol_inventory() {
    let records = fixture_records();
    let snapshot = analyze_bytes(
        &registry_bytes(&records),
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Invisible,
                VisualKind::Cube,
            ],
        ),
    )
    .unwrap();
    assert!(matches!(
        baseline_from_snapshot(&snapshot, Vec::new()),
        Err(CoverageError::NonCanonicalProtocolInventory(_))
    ));
}

#[test]
fn baseline_parser_rejects_unknown_fields_and_oversized_input() {
    let records = fixture_records();
    let snapshot = analyze_bytes(
        &registry_bytes(&records),
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    let mut value = serde_json::to_value(baseline(&snapshot)).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(matches!(
        parse_baseline(&serde_json::to_vec(&value).unwrap()),
        Err(CoverageError::Json(_))
    ));
    assert!(matches!(
        parse_baseline(&vec![b' '; visualcoverage::MAX_BASELINE_BYTES + 1]),
        Err(CoverageError::BaselineTooLarge)
    ));
}

#[test]
fn current_model_family_is_recorded_for_diagnostic_counts() {
    let records = fixture_records();
    let snapshot = analyze_bytes(
        &registry_bytes(&records),
        &blob(
            &records,
            &[
                VisualKind::Invisible,
                VisualKind::Cube,
                VisualKind::Diagnostic,
            ],
        ),
    )
    .unwrap();
    let family = StateIdentity::from_record(&records[2]).model_family;
    assert_eq!(snapshot.diagnostics_by_family.get(&family), Some(&1));
    assert_eq!(snapshot.diagnostics_by_name.get("minecraft:vine"), Some(&1));
}

fn strict_fixture_records(families: &[ModelFamily]) -> Vec<RegistryRecord> {
    let all = read_registry(include_bytes!(
        "../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read production registry");
    families
        .iter()
        .enumerate()
        .map(|(index, &family)| {
            let mut record = all
                .iter()
                .find(|record| record.model_family == family)
                .unwrap_or_else(|| panic!("missing fixture record for {family:?}"))
                .clone();
            record.sequential_id = index as u32;
            record.network_hash = 0x9100_0000 + index as u32;
            record
        })
        .collect()
}

fn strict_no_draw(flags: BlockFlags, role: ContributorRole) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags,
        kind: VisualKind::Invisible,
        contributor_role: role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

fn strict_cube(faces: [u32; 6]) -> BlockVisual {
    BlockVisual {
        faces,
        flags: BlockFlags::CUBE_GEOMETRY,
        kind: VisualKind::Cube,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

fn strict_model(kind: VisualKind, template: u32) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags: BlockFlags::empty(),
        kind,
        contributor_role: ContributorRole::Primary,
        model_template: template,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

fn strict_liquid(faces: [u32; 6], variant: u32) -> BlockVisual {
    BlockVisual {
        faces,
        flags: BlockFlags::empty(),
        kind: VisualKind::Liquid,
        contributor_role: ContributorRole::LiquidAdditional,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant,
    }
}

fn strict_diagnostic(flags: BlockFlags, role: ContributorRole) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags,
        kind: VisualKind::Diagnostic,
        contributor_role: role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

fn strict_quad(material: u32) -> ModelQuad {
    ModelQuad {
        positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material,
        flags: 0,
    }
}

fn strict_runtime(
    records: &[RegistryRecord],
    visuals: Vec<BlockVisual>,
    mut materials: Vec<Material>,
    templates: Vec<ModelTemplate>,
    quads: Vec<ModelQuad>,
    animations: Vec<Animation>,
    frames: Vec<TextureRef>,
) -> RuntimeAssets {
    if animations.is_empty() {
        for material in &mut materials {
            material.animation = NO_ANIMATION;
        }
    }
    let mut hashed = records
        .iter()
        .map(|record| (record.network_hash, record.sequential_id))
        .collect::<Vec<_>>();
    hashed.sort_unstable();
    let compiled = CompiledAssets {
        visuals: visuals.into_boxed_slice(),
        hashed: hashed.into_boxed_slice(),
        materials: materials.into_boxed_slice(),
        model_templates: templates.into_boxed_slice(),
        model_quads: quads.into_boxed_slice(),
        animations: animations.into_boxed_slice(),
        animation_frames: frames.into_boxed_slice(),
        texture_pages: vec![TexturePage::new(texture_array(8))].into_boxed_slice(),
        biomes: CompiledBiomeAssets {
            tint_maps_rgb8: vec![0; TINT_MAP_BYTES].into_boxed_slice(),
            rules: vec![BiomeRule {
                id: 0,
                name: "minecraft:plains".into(),
                flags: 0,
                grass: TintSource::direct(0),
                foliage: TintSource::direct(0),
                dry_foliage: TintSource::direct(0),
                water: TintSource::direct(0),
                temperature_bits: 0,
                downfall_bits: 0,
            }]
            .into_boxed_slice(),
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).expect("encode strict fixture"))
        .expect("decode strict fixture")
}

fn strict_materials() -> Vec<Material> {
    vec![
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
        Material {
            texture: TextureRef::new(0, 2).unwrap(),
            flags: 0,
            animation: 0,
        },
        Material {
            texture: TextureRef::new(0, 3).unwrap(),
            flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
            animation: 0,
        },
        Material {
            texture: TextureRef::new(0, 4).unwrap(),
            flags: MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
            animation: 0,
        },
    ]
}

fn strict_animations() -> (Vec<Animation>, Vec<TextureRef>) {
    (
        vec![Animation {
            frame_start: 0,
            frame_count: 2,
            ticks_per_frame: 1,
            atlas_index: 0,
            atlas_tile_variant: 0,
            replicate: 1,
            flags: ANIMATION_FLAG_BLEND,
        }],
        vec![
            TextureRef::new(0, 5).unwrap(),
            TextureRef::new(0, 6).unwrap(),
        ],
    )
}

fn strict_baseline(
    snapshot: &visualcoverage::CoverageSnapshot,
    invisible: &[StateIdentity],
) -> Baseline {
    Baseline {
        schema: visualcoverage::BASELINE_SCHEMA.into(),
        protocol: 1001,
        registry_sha256: snapshot.registry_sha256.clone(),
        counts: snapshot.counts,
        states: snapshot.states.clone(),
        diagnostic_sequential_ids: snapshot
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist: invisible
            .iter()
            .cloned()
            .map(|state| AllowlistEntry {
                state,
                authority: "Reviewed no-draw fixture".into(),
                source: "https://example.invalid/strict-fixture".into(),
            })
            .collect(),
        expected_vine_diagnostic_masks: snapshot.vine_diagnostic_masks.clone(),
    }
}

fn strict_snapshot(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
) -> visualcoverage::CoverageSnapshot {
    analyze_records(records, runtime, "registry-hash", "assets-hash")
        .expect("analyze strict fixture")
}

#[test]
fn strict_rejects_non_air_diagnostics_and_unknown_families() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Cube]);
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_diagnostic(BlockFlags::CUBE_GEOMETRY, ContributorRole::Primary),
        ],
        strict_materials(),
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let snapshot = strict_snapshot(&records, &runtime);
    let expected = snapshot.states[1].clone();
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::NonAirDiagnostic { state }) if state == expected
    ));

    let mut unknown = records.clone();
    unknown[1].model_family = ModelFamily::Unknown;
    let runtime = strict_runtime(
        &unknown,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([1; 6]),
        ],
        strict_materials(),
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let snapshot = strict_snapshot(&unknown, &runtime);
    let expected = snapshot.states[1].clone();
    assert!(matches!(
        strict_records(
            &unknown,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::UnsupportedModelFamily { state, family })
            if state == expected && family == "unknown"
    ));
}

#[test]
fn strict_requires_air_no_draw_and_source_cited_invisibles() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Invisible]);
    let runtime = strict_runtime(
        &records,
        vec![
            strict_diagnostic(BlockFlags::AIR, ContributorRole::Air),
            strict_no_draw(BlockFlags::empty(), ContributorRole::Primary),
        ],
        strict_materials(),
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let snapshot = strict_snapshot(&records, &runtime);
    let expected_air = snapshot.states[0].clone();
    let invisible = snapshot.states[1].clone();
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, std::slice::from_ref(&invisible)),
            false,
        ),
        Err(CoverageError::InvalidAirRoute { state, kind })
            if state == expected_air && kind == "diagnostic"
    ));

    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_no_draw(BlockFlags::empty(), ContributorRole::Primary),
        ],
        strict_materials(),
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::UnreviewedInvisible { state }) if state == snapshot.states[1]
    ));

    let mut drawable_invisible = strict_no_draw(BlockFlags::empty(), ContributorRole::Primary);
    drawable_invisible.faces[0] = 1;
    drawable_invisible.animation = 0;
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            drawable_invisible,
        ],
        strict_materials(),
        vec![],
        vec![],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    let invisible = snapshot.states[1].clone();
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, std::slice::from_ref(&invisible)),
            false,
        ),
        Err(CoverageError::InvalidInvisibleRoute { state, kind })
            if state == invisible && kind == "invisible"
    ));
}

#[test]
fn strict_rejects_empty_or_diagnostic_cube_model_and_liquid_routes() {
    let records = strict_fixture_records(&[
        ModelFamily::Air,
        ModelFamily::Cube,
        ModelFamily::Cross,
        ModelFamily::Liquid,
    ]);

    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([0; 6]),
            strict_model(VisualKind::Cross, 0),
            strict_liquid([3; 6], 0),
        ],
        strict_materials(),
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![strict_quad(1)],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::EmptyVisibleRoute { state, kind })
            if state == snapshot.states[1] && kind == "cube"
    ));

    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([1; 6]),
            strict_model(VisualKind::Cross, 0),
            strict_liquid([3; 6], 0),
        ],
        strict_materials(),
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 0,
            flags: 0,
        }],
        vec![],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::EmptyVisibleRoute { state, kind })
            if state == snapshot.states[2] && kind == "cross"
    ));

    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([1; 6]),
            strict_model(VisualKind::Cross, 0),
            strict_liquid([3; 6], 16),
        ],
        strict_materials(),
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![strict_quad(1)],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::InvalidLiquidDepth { state, variant })
            if state == snapshot.states[3] && variant == 16
    ));

    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([1; 6]),
            strict_model(VisualKind::Cross, 0),
            strict_liquid([3, 4, 3, 4, 3, 4], 0),
        ],
        strict_materials(),
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![strict_quad(1)],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::InvalidLiquidMaterials { state, material_ids })
            if state == snapshot.states[3] && material_ids == vec![3, 4]
    ));

    let mut diagnostic_texture_materials = strict_materials();
    diagnostic_texture_materials[1].texture = TextureRef::DIAGNOSTIC;
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([1; 6]),
            strict_model(VisualKind::Cross, 0),
            strict_liquid([3; 6], 0),
        ],
        diagnostic_texture_materials,
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![strict_quad(1)],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    assert!(matches!(
        strict_records(
            &records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::DiagnosticTextureReference {
            state,
            material_id: 1,
        }) if state == snapshot.states[1]
    ));
}

#[test]
fn strict_reports_exact_render_stream_material_template_and_animation_routes() {
    let records = strict_fixture_records(&[
        ModelFamily::Air,
        ModelFamily::Cube,
        ModelFamily::Cross,
        ModelFamily::Slab,
        ModelFamily::Liquid,
        ModelFamily::Invisible,
    ]);
    let (animations, frames) = strict_animations();
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([2, 1, 2, 1, 2, 1]),
            strict_model(VisualKind::Cross, 0),
            strict_model(VisualKind::Model, 1),
            strict_liquid([3; 6], 7),
            strict_no_draw(BlockFlags::empty(), ContributorRole::Primary),
        ],
        strict_materials(),
        vec![
            ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            },
            ModelTemplate {
                quad_start: 1,
                quad_count: 1,
                flags: 0,
            },
        ],
        vec![strict_quad(2), strict_quad(1)],
        animations,
        frames,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    let invisible = snapshot.states[5].clone();
    let report = strict_records(
        &records,
        &runtime,
        snapshot.clone(),
        &strict_baseline(&snapshot, std::slice::from_ref(&invisible)),
        false,
    )
    .expect("strict graph passes");

    assert_eq!(
        report
            .routes
            .iter()
            .map(|route| route.state.sequential_id)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(report.routes[0].render_stream, RenderStream::NoDraw);
    assert_eq!(report.routes[0].material_ids, Vec::<u32>::new());
    assert_eq!(report.routes[1].render_stream, RenderStream::Cube);
    assert_eq!(report.routes[1].material_ids, vec![1, 2]);
    assert_eq!(report.routes[1].animation_ids, vec![0]);
    assert_eq!(report.routes[2].render_stream, RenderStream::Model);
    assert_eq!(report.routes[2].model_template, Some(0));
    assert_eq!(report.routes[2].material_ids, vec![2]);
    assert_eq!(report.routes[2].animation_ids, vec![0]);
    assert_eq!(report.routes[3].render_stream, RenderStream::Model);
    assert_eq!(report.routes[3].model_template, Some(1));
    assert_eq!(report.routes[3].material_ids, vec![1]);
    assert_eq!(report.routes[4].render_stream, RenderStream::Liquid);
    assert_eq!(report.routes[4].material_ids, vec![3]);
    assert_eq!(report.routes[4].animation_ids, vec![0]);
    assert_eq!(report.routes[5].render_stream, RenderStream::NoDraw);
    assert_eq!(report.invisible_decisions.len(), 2);
    assert_eq!(report.states_by_stream[&RenderStream::NoDraw], 2);
    assert_eq!(report.states_by_stream[&RenderStream::Cube], 1);
    assert_eq!(report.states_by_stream[&RenderStream::Model], 2);
    assert_eq!(report.states_by_stream[&RenderStream::Liquid], 1);
}

#[test]
fn strict_json_is_hash_bound_sorted_and_byte_identical() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Cube]);
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([2, 1, 2, 1, 2, 1]),
        ],
        strict_materials(),
        vec![],
        vec![],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&records, &runtime);
    let report = strict_records(
        &records,
        &runtime,
        snapshot.clone(),
        &strict_baseline(&snapshot, &[]),
        false,
    )
    .expect("strict graph passes");
    assert_eq!(report.registry_sha256, "registry-hash");
    assert_eq!(report.assets_sha256, "assets-hash");
    assert_eq!(report.schema, visualcoverage::STRICT_REPORT_SCHEMA);
    let first = deterministic_json(&report).unwrap();
    let second = deterministic_json(&report).unwrap();
    assert_eq!(first, second);
    assert!(first.ends_with(b"\n"));
    assert!(
        first
            .windows(b"\"no_draw\"".len())
            .any(|window| window == b"\"no_draw\"")
    );
}

#[test]
#[ignore = "requires CINNABAR_REAL_PACK pointing at the ignored pinned MCBEAS04 blob"]
fn strict_cli_rejects_the_current_real_pack_until_zero_diagnostics() {
    let assets_path = std::env::var_os("CINNABAR_REAL_PACK")
        .map(std::path::PathBuf::from)
        .expect("set CINNABAR_REAL_PACK to the ignored pinned vanilla-v1001.mcbea");
    assert!(assets_path.is_file(), "missing real pack: {assets_path:?}");
    let baseline_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/assets/data/visual-coverage-v1001.json");
    let registry_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/assets/data/block-registry-v1001.bin");
    let expected = parse_baseline(&std::fs::read(&baseline_path).unwrap()).unwrap();
    assert_eq!(expected.diagnostic_sequential_ids.len(), 14_941);

    let directory = tempfile::tempdir().unwrap();
    let report_path = directory.path().join("strict.json");
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_visualcoverage"))
        .args([
            "strict",
            "--registry",
            registry_path.to_str().unwrap(),
            "--assets",
            assets_path.to_str().unwrap(),
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--out",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !run.status.success(),
        "real pack unexpectedly passed strict"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("NonAirDiagnostic") || stderr.contains("UnsupportedModelFamily"),
        "unexpected stderr: {stderr}"
    );
    assert!(!report_path.exists());
}
