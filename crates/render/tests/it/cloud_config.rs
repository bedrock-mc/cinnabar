use assets::{AtmosphereRole, AtmosphereTexture};
use meshing::{CloudFace, PackedCloudQuad};
use render::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCoverageSemantics,
    CloudGeometryDiagnostic, CloudGeometryDiagnosticError, CloudMatchingView, CloudQuality,
    CloudRenderConfig, adjusted_cloud_distance_blocks, adjusted_player_render_distance_blocks,
};

const QUALITIES: [CloudQuality; 4] = [
    CloudQuality::Low,
    CloudQuality::Medium,
    CloudQuality::High,
    CloudQuality::Ultra,
];

#[test]
fn ordinary_player_camera_includes_server_radius_margin_before_adjustment() {
    for (confirmed, expected) in [
        (0.0, 40.0),
        (32.0, 45.0),
        (48.0, 60.0),
        (64.0, 72.0),
        (128.0, 128.0),
        (160.0, 160.0),
        (256.0, 256.0),
    ] {
        assert_eq!(
            adjusted_player_render_distance_blocks(confirmed),
            Some(expected)
        );
    }
    for invalid in [-1.0, 64.5, f32::NAN, f32::INFINITY, i32::MAX as f32] {
        assert_eq!(adjusted_player_render_distance_blocks(invalid), None);
    }
}

#[test]
fn classic_cloud_distance_uses_native_camera_margins_and_minimum() {
    for (blocks, expected) in [
        (0.0, 40.0),
        (32.0, 40.0),
        (64.0, 60.0),
        (65.0, 57.0),
        (80.0, 72.0),
        (81.0, 65.0),
        (128.0, 112.0),
        (256.0, 240.0),
    ] {
        assert_eq!(
            adjusted_cloud_distance_blocks(blocks, 1.0, None),
            Some(expected)
        );
    }
}

#[test]
fn classic_cloud_distance_preserves_caller_and_conditional_platform_coefficients() {
    assert_eq!(
        adjusted_cloud_distance_blocks(128.0, 2.0, None),
        Some(224.0)
    );
    assert_eq!(adjusted_cloud_distance_blocks(128.0, 0.5, None), Some(56.0));
    assert_eq!(adjusted_cloud_distance_blocks(128.0, 0.0, None), Some(40.0));
    assert_eq!(
        adjusted_cloud_distance_blocks(128.0, 2.0, Some(0.5)),
        Some(56.0)
    );
    assert_eq!(
        adjusted_cloud_distance_blocks(128.0, 0.5, Some(2.0)),
        Some(56.0)
    );
    assert_eq!(
        adjusted_cloud_distance_blocks(128.0, 2.0, Some(0.0)),
        Some(40.0)
    );
}

#[test]
fn classic_cloud_distance_does_not_upload_non_finite_or_non_native_inputs() {
    for blocks in [-1.0, 64.5, f32::NAN, f32::INFINITY, i32::MAX as f32] {
        assert_eq!(adjusted_cloud_distance_blocks(blocks, 1.0, None), None);
    }
    for coefficient in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        assert_eq!(
            adjusted_cloud_distance_blocks(128.0, coefficient, None),
            None
        );
        assert_eq!(
            adjusted_cloud_distance_blocks(128.0, 1.0, Some(coefficient)),
            None
        );
    }
}

#[test]
fn native_quality_records_are_exact_and_default_to_high() {
    let expected = [
        (CloudQuality::Low, 1, 64, 2, true, true),
        (CloudQuality::Medium, 2, 64, 3, true, true),
        (CloudQuality::High, 3, 64, 3, true, true),
        (CloudQuality::Ultra, 4, 64, 3, true, true),
    ];

    assert_eq!(CloudQuality::ALL, QUALITIES);
    for (quality, grid, mesh, distance, distance_control, lighting) in expected {
        let config = CloudRenderConfig::native(quality);
        assert_eq!(config.quality(), quality);
        assert_eq!(config.grid_size(), grid);
        assert_eq!(config.mesh_size(), mesh);
        assert_eq!(config.distance_scale(), distance);
        assert_eq!(config.distance_control(), distance_control);
        assert_eq!(config.lighting(), lighting);
    }
    assert_eq!(CloudQuality::default(), CloudQuality::High);
    assert_eq!(
        CloudRenderConfig::default(),
        CloudRenderConfig::native(CloudQuality::High)
    );
}

#[test]
fn calibration_report_refuses_an_uncalibrated_mapping() {
    let mut harness = CloudCalibrationHarness::default();
    for quality in QUALITIES {
        harness
            .record_matching_view(matching_view(quality))
            .unwrap();
    }

    assert_eq!(
        harness.publish(),
        Err(CloudCalibrationError::UncalibratedMapping {
            quality: CloudQuality::Low,
        })
    );
}

#[test]
fn calibration_report_records_matching_views_and_derived_coverage_by_quality() {
    let mut harness = CloudCalibrationHarness::default();
    for (index, quality) in QUALITIES.into_iter().enumerate() {
        let view = matching_view(quality);
        let semantics = CloudCoverageSemantics::try_new(
            64_000 + index as u32,
            128_000 + index as u32,
            192_000 + index as u32,
            256_000 + index as u32,
        )
        .unwrap();
        harness.record_matching_view(view).unwrap();
        harness
            .record_coverage_semantics(quality, semantics)
            .unwrap();
    }

    let report = harness.publish().unwrap();
    assert_eq!(report.records().len(), 4);
    for (index, quality) in QUALITIES.into_iter().enumerate() {
        let record = report.record(quality);
        assert_eq!(record.config(), CloudRenderConfig::native(quality));
        assert_eq!(record.matching_view().quality(), quality);
        assert_eq!(
            record.coverage_semantics().mesh_size_world_milliblocks(),
            64_000 + index as u32
        );
        assert_eq!(
            record.coverage_semantics().grid_size_world_milliblocks(),
            128_000 + index as u32
        );
        assert_eq!(
            record
                .coverage_semantics()
                .distance_scale_world_milliblocks(),
            192_000 + index as u32
        );
        assert_eq!(
            record.coverage_semantics().coverage_radius_milliblocks(),
            256_000 + index as u32
        );
    }
}

#[test]
fn calibration_inputs_are_bounded_and_duplicate_records_are_rejected() {
    assert!(CloudCoverageSemantics::try_new(0, 1, 1, 1).is_err());
    assert!(CloudCoverageSemantics::try_new(u32::MAX, 1, 1, 1).is_err());
    assert!(
        CloudMatchingView::try_new(CloudQuality::High, [0; 3], 0, 0, [0; 32], [1; 32]).is_err()
    );
    assert!(
        CloudMatchingView::try_new(CloudQuality::High, [0; 3], 180_001, 0, [1; 32], [2; 32],)
            .is_err()
    );

    let mut harness = CloudCalibrationHarness::default();
    let view = matching_view(CloudQuality::High);
    harness.record_matching_view(view).unwrap();
    assert_eq!(
        harness.record_matching_view(view),
        Err(CloudCalibrationError::DuplicateMatchingView {
            quality: CloudQuality::High,
        })
    );
}

#[test]
fn provisional_geometry_diagnostic_is_exact_bounded_and_parser_stable() {
    let diagnostic = CloudGeometryDiagnostic::try_new(
        CloudRenderConfig::default(),
        [0xab; 32],
        13_356,
        7_654,
        61_232,
        9,
        256_000,
        128_000,
        132_000,
    )
    .unwrap();

    assert_eq!(diagnostic.config(), CloudRenderConfig::default());
    assert_eq!(diagnostic.occupied_texels(), 13_356);
    assert_eq!(diagnostic.quad_count(), 7_654);
    assert_eq!(diagnostic.quad_bytes(), 61_232);
    assert_eq!(diagnostic.instance_count(), 9);
    assert_eq!(diagnostic.texture_period_milliblocks(), 256_000);
    assert_eq!(diagnostic.underside_y_milliblocks(), 128_000);
    assert_eq!(diagnostic.top_y_milliblocks(), 132_000);
    assert_eq!(
        diagnostic.marker_fields(),
        concat!(
            "calibrated=false quality=High occupied_texels=13356 quad_count=7654 ",
            "quad_bytes=61232 instance_count=9 texture_period_milliblocks=256000 ",
            "underside_y_milliblocks=128000 top_y_milliblocks=132000 ",
            "native_grid_size=3 native_mesh_size=64 native_distance_scale=3 ",
            "native_distance_control=true native_lighting=true asset_identity_sha256=",
            "abababababababababababababababababababababababababababababababab"
        )
    );
}

#[test]
fn provisional_geometry_diagnostic_rejects_inconsistent_or_unbounded_values() {
    let valid = || {
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [1; 32],
            13_356,
            7_654,
            61_232,
            9,
            256_000,
            128_000,
            132_000,
        )
    };
    assert!(valid().is_ok());
    assert_eq!(
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [0; 32],
            13_356,
            7_654,
            61_232,
            9,
            256_000,
            128_000,
            132_000,
        ),
        Err(CloudGeometryDiagnosticError::InvalidAssetIdentity)
    );
    assert_eq!(
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [1; 32],
            65_537,
            7_654,
            61_232,
            9,
            256_000,
            128_000,
            132_000,
        ),
        Err(CloudGeometryDiagnosticError::TooManyOccupiedTexels {
            actual: 65_537,
            max: 65_536,
        })
    );
    assert_eq!(
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [1; 32],
            13_356,
            7_654,
            61_224,
            9,
            256_000,
            128_000,
            132_000,
        ),
        Err(CloudGeometryDiagnosticError::InconsistentQuadBytes {
            actual: 61_224,
            expected: 61_232,
        })
    );
    assert_eq!(
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [1; 32],
            13_356,
            7_654,
            61_232,
            0,
            256_000,
            128_000,
            132_000,
        ),
        Err(CloudGeometryDiagnosticError::InvalidInstanceCount)
    );
    assert_eq!(
        CloudGeometryDiagnostic::try_new(
            CloudRenderConfig::default(),
            [1; 32],
            13_356,
            7_654,
            61_232,
            9,
            256_000,
            132_000,
            128_000,
        ),
        Err(CloudGeometryDiagnosticError::InvalidWorldBounds)
    );
}

#[test]
fn runtime_geometry_diagnostic_counts_validated_occupancy_and_packed_bytes() {
    let mut rgba8 = vec![255; 256 * 256 * 4];
    for texel in rgba8.as_chunks_mut::<4>().0 {
        texel[3] = 1;
    }
    rgba8[3] = 255;
    rgba8[7] = 128;
    let texture = AtmosphereTexture {
        role: AtmosphereRole::Clouds,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: 7_880,
        source_sha256: [2; 32],
        pixels_sha256: [4; 32],
        width: 256,
        height: 256,
        rgba8: rgba8.into(),
    };
    let records = [
        PackedCloudQuad::try_pack(0, 0, 1, 1, CloudFace::Up).unwrap(),
        PackedCloudQuad::try_pack(0, 0, 1, 1, CloudFace::Down).unwrap(),
    ];

    let diagnostic = CloudGeometryDiagnostic::from_runtime_layout(
        CloudRenderConfig::default(),
        [5; 32],
        &texture,
        &records,
        9,
        256_000,
        128_000,
        132_000,
    )
    .unwrap();

    assert_eq!(diagnostic.occupied_texels(), 2);
    assert_eq!(diagnostic.quad_count(), 2);
    assert_eq!(diagnostic.quad_bytes(), 16);
}

fn matching_view(quality: CloudQuality) -> CloudMatchingView {
    let marker = quality as u8 + 1;
    CloudMatchingView::try_new(
        quality,
        [1_000, 129_000, -1_000],
        45_000,
        -15_000,
        [marker; 32],
        [marker + 4; 32],
    )
    .unwrap()
}
