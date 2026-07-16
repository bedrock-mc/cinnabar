use super::{support::*, *};

#[test]
fn strict_rejects_diagnostic_stair_topology_variants() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Stair]);
    let visuals = vec![
        strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
        strict_model(VisualKind::Model, 0),
    ];

    let runtime = strict_runtime(
        &records,
        visuals.clone(),
        strict_materials(),
        strict_stair_templates(),
        vec![
            strict_quad(1),
            strict_quad(0),
            strict_quad(1),
            strict_quad(1),
            strict_quad(1),
        ],
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
        Err(CoverageError::DiagnosticMaterialReference {
            state,
            material_id: 0,
        }) if state == snapshot.states[1]
    ));

    let mut diagnostic_frames = strict_animations().1;
    diagnostic_frames[0] = TextureRef::DIAGNOSTIC;
    let runtime = strict_runtime(
        &records,
        visuals,
        strict_materials(),
        strict_stair_templates(),
        vec![
            strict_quad(1),
            strict_quad(1),
            strict_quad(2),
            strict_quad(1),
            strict_quad(1),
        ],
        strict_animations().0,
        diagnostic_frames,
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
        Err(CoverageError::DiagnosticAnimationFrameReference {
            state,
            animation_id: 0,
        }) if state == snapshot.states[1]
    ));
}

#[test]
fn strict_traverses_every_connected_pane_and_fence_template_variant() {
    for (family, flag, counts, diagnostic_offset) in [
        (
            ModelFamily::Pane,
            MODEL_TEMPLATE_FLAG_PANE,
            (0_u32..16)
                .map(|mask| 6 + mask.count_ones() * 4)
                .collect::<Vec<_>>(),
            15_usize,
        ),
        (
            ModelFamily::Fence,
            MODEL_TEMPLATE_FLAG_FENCE_WOOD,
            std::iter::once(6)
                .chain((0_u32..16).map(|mask| mask.count_ones() * 8))
                .collect::<Vec<_>>(),
            16,
        ),
    ] {
        let records = strict_fixture_records(&[ModelFamily::Air, family]);
        let visuals = vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_model(VisualKind::Model, 0),
        ];
        let mut start = 0_u32;
        let templates = counts
            .iter()
            .map(|&quad_count| {
                let template = ModelTemplate {
                    quad_start: start,
                    quad_count,
                    flags: flag,
                };
                start += quad_count;
                template
            })
            .collect::<Vec<_>>();
        let diagnostic_quad = templates[diagnostic_offset].quad_start as usize;
        let mut quads = vec![strict_quad(1); start as usize];
        quads[diagnostic_quad] = strict_quad(0);
        let runtime = strict_runtime(
            &records,
            visuals,
            strict_materials(),
            templates,
            quads,
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
            Err(CoverageError::DiagnosticMaterialReference {
                state,
                material_id: 0,
            }) if state == snapshot.states[1]
        ));
    }
}

#[test]
fn strict_traverses_compound_model_continuations() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Gate]);
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_model(VisualKind::Model, 0),
        ],
        strict_materials(),
        vec![
            ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
            },
            ModelTemplate {
                quad_start: 1,
                quad_count: 1,
                flags: 0,
            },
        ],
        vec![strict_quad(1), strict_quad(0)],
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
        Err(CoverageError::DiagnosticMaterialReference {
            state,
            material_id: 0,
        }) if state == snapshot.states[1]
    ));
}

#[test]
fn strict_reports_all_reachable_stair_topology_materials_and_animations() {
    let records = strict_fixture_records(&[ModelFamily::Air, ModelFamily::Stair]);
    let mut materials = strict_materials();
    materials.push(Material {
        texture: TextureRef::new(0, 7).unwrap(),
        flags: 0,
        animation: NO_ANIMATION,
    });
    let runtime = strict_runtime(
        &records,
        vec![
            strict_no_draw(BlockFlags::AIR, ContributorRole::Air),
            strict_model(VisualKind::Model, 0),
        ],
        materials,
        strict_stair_templates(),
        vec![
            strict_quad(1),
            strict_quad(2),
            strict_quad(3),
            strict_quad(4),
            strict_quad(5),
        ],
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
    .expect("all reachable stair topology templates are drawable");

    assert_eq!(report.routes[1].model_template, Some(0));
    assert_eq!(report.routes[1].material_ids, vec![1, 2, 3, 4, 5]);
    assert_eq!(report.routes[1].animation_ids, vec![0]);
}

#[test]
fn strict_bytes_computes_and_binds_production_input_hashes() {
    let mut records = read_registry(include_bytes!(
        "../../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .unwrap();
    for record in &mut records {
        if !record.flags.contains(BlockFlags::AIR) {
            record.model_family = ModelFamily::Cube;
        }
    }
    let registry = registry_bytes(&records);
    let visuals = records
        .iter()
        .map(|record| {
            if record.flags.contains(BlockFlags::AIR) {
                strict_no_draw(BlockFlags::AIR, ContributorRole::Air)
            } else {
                strict_cube([1; 6])
            }
        })
        .collect();
    let assets = strict_blob(
        &records,
        visuals,
        strict_materials(),
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let snapshot = analyze_bytes(&registry, &assets).unwrap();
    let expected_registry_hash = format!("{:x}", Sha256::digest(&registry));
    let expected_assets_hash = format!("{:x}", Sha256::digest(&assets));
    let report = strict_bytes(&registry, &assets, &strict_baseline(&snapshot, &[])).unwrap();

    assert_eq!(report.registry_sha256, expected_registry_hash);
    assert_eq!(report.assets_sha256, expected_assets_hash);
    assert_eq!(report.routes.len(), 16_913);
}
