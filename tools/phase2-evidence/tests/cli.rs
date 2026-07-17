use std::fs;
use std::path::Path;
use std::process::Command;

use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder};
use phase2_evidence::{
    ComparisonManifest, ComparisonRequest, Crop, EvidenceError, EvidenceKind, LabelledSample,
    MAX_IMAGE_DIMENSION, MAX_INPUT_BYTES, Thresholds, compare,
};
use serde_json::Value;
use tempfile::tempdir;

fn rgba_png(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut encoded = Vec::new();
    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    PngEncoder::new(&mut encoded)
        .write_image(&bytes, width, height, ColorType::Rgba8.into())
        .expect("encode synthetic PNG");
    encoded
}

fn manifest(width: u32, height: u32) -> ComparisonManifest {
    ComparisonManifest {
        crop: Crop {
            identity: "whole-frame".to_owned(),
            x: 0,
            y: 0,
            width,
            height,
        },
        thresholds: Thresholds {
            maximum_channel_error_rgb8: 1,
            maximum_channel_error_linear: 0.01,
            mean_squared_error_linear: 0.001,
        },
        allow_alpha_mismatch: false,
        samples: Vec::new(),
    }
}

#[test]
fn comparison_rejects_mismatched_dimensions_and_hashes_pixels_in_linear_rgb() {
    let request = ComparisonRequest::synthetic(
        EvidenceKind::Biome,
        rgba_png(2, 1, &[[128, 64, 32, 255], [255, 255, 255, 255]]),
        rgba_png(2, 1, &[[128, 64, 32, 255], [254, 255, 255, 255]]),
    );
    let report = compare(request).expect("bounded comparison");
    assert_eq!(report.sample_count, 2);
    assert_eq!(report.maximum_channel_error_rgb8, 1);
    assert!(report.mean_squared_error_linear.is_finite());
    assert!(report.mean_squared_error_linear > 0.000_01);
    assert!(report.mean_squared_error_linear < 0.000_02);
    assert_ne!(report.native_pixel_sha256, report.cinnabar_pixel_sha256);
    assert_eq!(report.native_pixel_sha256.len(), 64);
    assert_eq!(report.cinnabar_pixel_sha256.len(), 64);
    assert!(compare(ComparisonRequest::dimension_mismatch()).is_err());
}

#[test]
fn manifest_rejects_duplicate_labels_non_finite_thresholds_and_invalid_crops() {
    let png = rgba_png(1, 1, &[[0, 0, 0, 255]]);
    let mut duplicate = manifest(1, 1);
    duplicate.samples = vec![
        LabelledSample {
            label: "centre".to_owned(),
            x: 0,
            y: 0,
        },
        LabelledSample {
            label: "centre".to_owned(),
            x: 0,
            y: 0,
        },
    ];
    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Lighting,
            duplicate,
            png.clone(),
            png.clone(),
        ))
        .is_err()
    );

    let mut path_label = manifest(1, 1);
    path_label.samples.push(LabelledSample {
        label: r"C:\captures\native.png".to_owned(),
        x: 0,
        y: 0,
    });
    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Lighting,
            path_label,
            png.clone(),
            png.clone(),
        ))
        .is_err()
    );

    let mut path_identity = manifest(1, 1);
    path_identity.crop.identity = "/tmp/native.png".to_owned();
    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Lighting,
            path_identity,
            png.clone(),
            png.clone(),
        ))
        .is_err()
    );

    let mut non_finite = manifest(1, 1);
    non_finite.thresholds.mean_squared_error_linear = f64::NAN;
    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::FogAir,
            non_finite,
            png.clone(),
            png.clone(),
        ))
        .is_err()
    );

    let mut invalid_crop = manifest(2, 1);
    invalid_crop.crop.x = u32::MAX;
    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Cloud,
            invalid_crop,
            png.clone(),
            png,
        ))
        .is_err()
    );
}

#[test]
fn alpha_mismatch_is_explicit_and_labelled_samples_use_linear_rgb() {
    let native = rgba_png(1, 1, &[[128, 64, 32, 255]]);
    let cinnabar = rgba_png(1, 1, &[[128, 64, 32, 254]]);
    let mut comparison_manifest = manifest(1, 1);
    comparison_manifest.samples.push(LabelledSample {
        label: "centre".to_owned(),
        x: 0,
        y: 0,
    });

    assert!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::FogWater,
            comparison_manifest.clone(),
            native.clone(),
            cinnabar.clone(),
        ))
        .is_err()
    );

    comparison_manifest.allow_alpha_mismatch = true;
    let report = compare(ComparisonRequest::from_bytes(
        EvidenceKind::FogWater,
        comparison_manifest,
        native,
        cinnabar,
    ))
    .expect("manifest permits alpha mismatch");
    assert_eq!(report.labelled_samples.len(), 1);
    assert_eq!(report.labelled_samples[0].label, "centre");
    assert_eq!(report.labelled_samples[0].mean_squared_error_linear, 0.0);
}

#[test]
fn comparison_enforces_encoded_input_dimension_and_threshold_bounds() {
    let oversized_input = vec![0; (MAX_INPUT_BYTES + 1) as usize];
    let error = compare(ComparisonRequest::synthetic(
        EvidenceKind::FogLava,
        oversized_input,
        rgba_png(1, 1, &[[0, 0, 0, 255]]),
    ))
    .expect_err("oversized encoded input must fail closed");
    assert!(matches!(error, EvidenceError::InputTooLarge { .. }));

    let too_wide = rgba_png(
        MAX_IMAGE_DIMENSION + 1,
        1,
        &vec![[0, 0, 0, 255]; (MAX_IMAGE_DIMENSION + 1) as usize],
    );
    let error = compare(ComparisonRequest::synthetic(
        EvidenceKind::FogAir,
        too_wide.clone(),
        too_wide,
    ))
    .expect_err("oversized decoded dimensions must fail closed");
    assert!(matches!(error, EvidenceError::ImageDimensions { .. }));

    let native = rgba_png(1, 1, &[[128, 64, 32, 255]]);
    let cinnabar = rgba_png(1, 1, &[[127, 64, 32, 255]]);
    let mut strict = manifest(1, 1);
    strict.thresholds.maximum_channel_error_rgb8 = 0;
    strict.thresholds.maximum_channel_error_linear = 0.0;
    strict.thresholds.mean_squared_error_linear = 0.0;
    let report = compare(ComparisonRequest::from_bytes(
        EvidenceKind::Biome,
        strict,
        native,
        cinnabar,
    ))
    .expect("comparison remains a valid report");
    assert!(!report.passed);
}

#[test]
fn cli_writes_a_path_free_report_and_rejects_an_output_input_alias() {
    let directory = tempdir().expect("temporary directory");
    let native_path = directory.path().join("native.png");
    let cinnabar_path = directory.path().join("cinnabar.png");
    let manifest_path = directory.path().join("manifest.json");
    let output_path = directory.path().join("comparison.json");
    let native = rgba_png(1, 1, &[[128, 64, 32, 255]]);
    let cinnabar = rgba_png(1, 1, &[[127, 64, 32, 255]]);
    fs::write(&native_path, native).expect("write native PNG");
    fs::write(&cinnabar_path, cinnabar).expect("write Cinnabar PNG");
    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest(1, 1)).expect("serialize manifest"),
    )
    .expect("write manifest");

    let output = run_compare(
        EvidenceKind::Celestial,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &output_path,
    );
    assert!(
        output.status.success(),
        "comparator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(&output_path).expect("read report");
    let report_text = String::from_utf8(report_bytes).expect("report is UTF-8");
    let report: Value = serde_json::from_str(&report_text).expect("report is JSON");
    assert_eq!(report["kind"], "celestial");
    assert_eq!(report["crop"]["identity"], "whole-frame");
    assert_eq!(report["passed"], true);
    for path in [&native_path, &cinnabar_path, &manifest_path, &output_path] {
        assert!(
            !report_text.contains(&path.display().to_string()),
            "report leaked path {}",
            path.display()
        );
    }

    let mut strict = manifest(1, 1);
    strict.thresholds.maximum_channel_error_rgb8 = 0;
    strict.thresholds.maximum_channel_error_linear = 0.0;
    strict.thresholds.mean_squared_error_linear = 0.0;
    fs::write(
        &manifest_path,
        serde_json::to_vec(&strict).expect("serialize strict manifest"),
    )
    .expect("write strict manifest");
    let failed_report_path = directory.path().join("failed-comparison.json");
    let failed = run_compare(
        EvidenceKind::Celestial,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &failed_report_path,
    );
    assert!(!failed.status.success());
    let failed_report: Value = serde_json::from_slice(
        &fs::read(&failed_report_path).expect("failed comparison still writes a report"),
    )
    .expect("failed report is JSON");
    assert_eq!(failed_report["passed"], false);

    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest(1, 1)).expect("serialize permissive manifest"),
    )
    .expect("restore permissive manifest");

    let alias = run_compare(
        EvidenceKind::Cloud,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &native_path,
    );
    assert!(!alias.status.success());

    let hard_link_path = directory.path().join("native-hard-link.png");
    fs::hard_link(&native_path, &hard_link_path).expect("create input hard link");
    let hard_link_alias = run_compare(
        EvidenceKind::Cloud,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &hard_link_path,
    );
    assert!(!hard_link_alias.status.success());
}

fn run_compare(
    kind: EvidenceKind,
    manifest: &Path,
    native: &Path,
    cinnabar: &Path,
    output: &Path,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_phase2-evidence"))
        .args([
            "compare",
            "--kind",
            kind.as_str(),
            "--manifest",
            manifest.to_str().expect("UTF-8 manifest path"),
            "--native",
            native.to_str().expect("UTF-8 native path"),
            "--cinnabar",
            cinnabar.to_str().expect("UTF-8 Cinnabar path"),
            "--out",
            output.to_str().expect("UTF-8 output path"),
        ])
        .output()
        .expect("run phase2-evidence")
}
