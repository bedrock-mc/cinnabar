use super::{support::*, *};

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
