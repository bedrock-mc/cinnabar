use super::{support::*, *};

#[test]
fn strict_atomic_writer_preserves_destinations_and_cleans_temps_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("strict.json");
    std::fs::write(&output, b"reviewed-report\n").unwrap();

    let error = write_deterministic_json_atomic(&output, &SerializationFailure).unwrap_err();
    assert!(matches!(error, CoverageError::Json(_)));
    assert_eq!(std::fs::read(&output).unwrap(), b"reviewed-report\n");
    assert_no_atomic_temp_artifacts(directory.path());

    let replacement_blocker = directory.path().join("blocked.json");
    std::fs::create_dir(&replacement_blocker).unwrap();
    let error = write_deterministic_json_atomic(
        &replacement_blocker,
        &serde_json::json!({"schema": "strict-test"}),
    )
    .unwrap_err();
    assert!(matches!(error, CoverageError::ReportWrite { .. }));
    assert!(replacement_blocker.is_dir());
    assert_no_atomic_temp_artifacts(directory.path());

    write_deterministic_json_atomic(&output, &serde_json::json!({"schema": "strict-test"}))
        .unwrap();
    assert_eq!(
        std::fs::read(&output).unwrap(),
        b"{\n  \"schema\": \"strict-test\"\n}\n"
    );
    assert_no_atomic_temp_artifacts(directory.path());
}

#[test]
fn strict_cli_preserves_preexisting_output_on_semantic_failure() {
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
    let report_path = directory.path().join("strict.json");
    std::fs::write(&registry_path, registry).unwrap();
    std::fs::write(&assets_path, assets).unwrap();
    std::fs::write(&baseline_path, deterministic_json(&expected).unwrap()).unwrap();
    std::fs::write(&report_path, b"reviewed-report\n").unwrap();

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
    assert!(!run.status.success());
    assert_eq!(std::fs::read(&report_path).unwrap(), b"reviewed-report\n");
    assert_no_atomic_temp_artifacts(directory.path());
}

#[test]
fn strict_rejects_each_diagnostic_transitive_and_unsupported_liquid_route() {
    let model_records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Cross]);
    let runtime = strict_runtime(
        &model_records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_model(VisualKind::Cross, 0),
        ],
        strict_materials(),
        vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        vec![strict_quad(0)],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&model_records, &runtime);
    assert!(matches!(
        strict_records(
            &model_records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::DiagnosticMaterialReference {
            state,
            material_id: 0,
        }) if state == snapshot.states[1]
    ));

    let liquid_records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Liquid]);
    let runtime = strict_runtime(
        &liquid_records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_liquid([0; 6], 0),
        ],
        strict_materials(),
        vec![],
        vec![],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&liquid_records, &runtime);
    assert!(matches!(
        strict_records(
            &liquid_records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::DiagnosticMaterialReference {
            state,
            material_id: 0,
        }) if state == snapshot.states[1]
    ));

    let runtime = strict_runtime(
        &liquid_records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_liquid([1; 6], 0),
        ],
        strict_materials(),
        vec![],
        vec![],
        strict_animations().0,
        strict_animations().1,
    );
    let snapshot = strict_snapshot(&liquid_records, &runtime);
    assert!(matches!(
        strict_records(
            &liquid_records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::InvalidLiquidMaterials {
            state,
            material_ids,
        }) if state == snapshot.states[1] && material_ids == vec![1]
    ));

    let mut diagnostic_frames = strict_animations().1;
    diagnostic_frames[0] = TextureRef::DIAGNOSTIC;
    let cube_records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Cube]);
    let runtime = strict_runtime(
        &cube_records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_cube([2; 6]),
        ],
        strict_materials(),
        vec![],
        vec![],
        strict_animations().0,
        diagnostic_frames,
    );
    let snapshot = strict_snapshot(&cube_records, &runtime);
    assert!(matches!(
        strict_records(
            &cube_records,
            &runtime,
            snapshot.clone(),
            &strict_baseline(&snapshot, &[]),
            false,
        ),
        Err(CoverageError::DiagnosticAnimationFrameReference {
            state,
            animation_id: 0,
        }) if state == snapshot.states[1]
    ));
}
