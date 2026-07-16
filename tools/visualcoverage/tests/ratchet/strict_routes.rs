use super::{support::*, *};

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
