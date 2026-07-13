use assets::{
    BiomeRule, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, Material, NO_ANIMATION, NO_MODEL_TEMPLATE, RegistryProvenance,
    RegistryRecord, RuntimeAssets, TINT_MAP_BYTES, TextureArray, TextureMip, TexturePage,
    TextureRef, TintSource, VisualKind, encode_blob, read_registry,
};
use visualcoverage::{
    AllowlistEntry, Baseline, Counts, CoverageError, StateIdentity, analyze_bytes,
    baseline_from_snapshot, deterministic_json, parse_baseline, ratchet, ratchet_protocol_1001,
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
