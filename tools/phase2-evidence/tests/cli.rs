use std::fs;
use std::path::Path;
use std::process::Command;

use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder};
use phase2_evidence::{
    ComparisonManifest, ComparisonRequest, Crop, EvidenceError, EvidenceKind, LabelledSample,
    MAX_CROP_IDENTITY_BYTES, MAX_IMAGE_DIMENSION, MAX_INPUT_BYTES, MAX_LABELLED_SAMPLES,
    MAX_SAMPLE_LABEL_BYTES, Thresholds, compare, compare_files,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
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

fn samples_at_exact_limit() -> Vec<LabelledSample> {
    (0..MAX_LABELLED_SAMPLES)
        .map(|index| LabelledSample {
            label: if index == 0 {
                "s".repeat(MAX_SAMPLE_LABEL_BYTES)
            } else {
                format!("sample-{index}")
            },
            x: 0,
            y: 0,
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
fn direct_request_enforces_identity_label_and_sample_limits_before_image_decode() {
    let png = rgba_png(1, 1, &[[0, 0, 0, 255]]);
    let mut exact = manifest(1, 1);
    exact.crop.identity = "c".repeat(MAX_CROP_IDENTITY_BYTES);
    exact.samples = samples_at_exact_limit();
    let exact_report = compare(ComparisonRequest::from_bytes(
        EvidenceKind::Biome,
        exact,
        png.clone(),
        png.clone(),
    ))
    .expect("exact manifest limits are accepted");
    assert_eq!(exact_report.labelled_samples.len(), MAX_LABELLED_SAMPLES);

    let mut empty_identity = manifest(1, 1);
    empty_identity.crop.identity.clear();
    assert!(matches!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Biome,
            empty_identity,
            Vec::new(),
            Vec::new(),
        )),
        Err(EvidenceError::EmptyCropIdentity)
    ));

    let mut long_identity = manifest(1, 1);
    long_identity.crop.identity = "c".repeat(MAX_CROP_IDENTITY_BYTES + 1);
    assert!(matches!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Biome,
            long_identity,
            Vec::new(),
            Vec::new(),
        )),
        Err(EvidenceError::CropIdentityTooLong { .. })
    ));

    let mut empty_label = manifest(1, 1);
    empty_label.samples.push(LabelledSample {
        label: String::new(),
        x: 0,
        y: 0,
    });
    assert!(matches!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Biome,
            empty_label,
            Vec::new(),
            Vec::new(),
        )),
        Err(EvidenceError::EmptySampleLabel)
    ));

    let mut long_label = manifest(1, 1);
    long_label.samples.push(LabelledSample {
        label: "s".repeat(MAX_SAMPLE_LABEL_BYTES + 1),
        x: 0,
        y: 0,
    });
    assert!(matches!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Biome,
            long_label,
            Vec::new(),
            Vec::new(),
        )),
        Err(EvidenceError::SampleLabelTooLong { .. })
    ));

    let mut too_many = manifest(1, 1);
    too_many.samples = vec![
        LabelledSample {
            label: String::new(),
            x: 0,
            y: 0,
        };
        MAX_LABELLED_SAMPLES + 1
    ];
    assert!(matches!(
        compare(ComparisonRequest::from_bytes(
            EvidenceKind::Biome,
            too_many,
            Vec::new(),
            Vec::new(),
        )),
        Err(EvidenceError::TooManySamples { .. })
    ));
}

#[test]
fn cli_enforces_identity_label_and_sample_exact_plus_one_and_empty_limits() {
    let directory = tempdir().expect("temporary directory");
    let native_path = directory.path().join("native.png");
    let cinnabar_path = directory.path().join("cinnabar.png");
    let manifest_path = directory.path().join("manifest.json");
    fs::write(&native_path, rgba_png(1, 1, &[[0, 0, 0, 255]])).expect("write native PNG");
    fs::write(&cinnabar_path, rgba_png(1, 1, &[[0, 0, 0, 255]])).expect("write Cinnabar PNG");

    let mut exact = manifest(1, 1);
    exact.crop.identity = "c".repeat(MAX_CROP_IDENTITY_BYTES);
    exact.samples = samples_at_exact_limit();
    fs::write(
        &manifest_path,
        serde_json::to_vec(&exact).expect("serialize exact manifest"),
    )
    .expect("write exact manifest");
    let exact_output = directory.path().join("exact.json");
    assert!(
        run_compare(
            EvidenceKind::Biome,
            &manifest_path,
            &native_path,
            &cinnabar_path,
            &exact_output,
        )
        .status
        .success()
    );

    let invalid_manifests = [
        {
            let mut value = manifest(1, 1);
            value.crop.identity.clear();
            ("empty-identity", value, "crop identity must not be empty")
        },
        {
            let mut value = manifest(1, 1);
            value.crop.identity = "c".repeat(MAX_CROP_IDENTITY_BYTES + 1);
            ("long-identity", value, "crop identity exceeds")
        },
        {
            let mut value = manifest(1, 1);
            value.samples.push(LabelledSample {
                label: String::new(),
                x: 0,
                y: 0,
            });
            ("empty-label", value, "sample label must not be empty")
        },
        {
            let mut value = manifest(1, 1);
            value.samples.push(LabelledSample {
                label: "s".repeat(MAX_SAMPLE_LABEL_BYTES + 1),
                x: 0,
                y: 0,
            });
            ("long-label", value, "sample label exceeds")
        },
        {
            let mut value = manifest(1, 1);
            value.samples = vec![
                LabelledSample {
                    label: String::new(),
                    x: 0,
                    y: 0,
                };
                MAX_LABELLED_SAMPLES + 1
            ];
            ("too-many-samples", value, "sample count exceeds")
        },
    ];
    for (name, invalid, expected_error) in invalid_manifests {
        fs::write(
            &manifest_path,
            serde_json::to_vec(&invalid).expect("serialize invalid manifest"),
        )
        .expect("write invalid manifest");
        let output_path = directory.path().join(format!("{name}.json"));
        let output = run_compare(
            EvidenceKind::Biome,
            &manifest_path,
            &native_path,
            &cinnabar_path,
            &output_path,
        );
        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "{name} emitted unexpected error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output_path.exists(), "{name} created an output report");
    }
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
    let report_text = std::str::from_utf8(&report_bytes).expect("report is UTF-8");
    let report: Value = serde_json::from_str(report_text).expect("report is JSON");
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

    let repeat_output_path = directory.path().join("comparison-repeat.json");
    let repeated = run_compare(
        EvidenceKind::Celestial,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &repeat_output_path,
    );
    assert!(repeated.status.success());
    let repeated_report_bytes = fs::read(&repeat_output_path).expect("read repeated report");
    assert_eq!(report_bytes, repeated_report_bytes);
    assert_eq!(
        sha256_hex(&report_bytes),
        sha256_hex(&repeated_report_bytes)
    );

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
    let native_before_alias = fs::read(&native_path).expect("read native before alias check");
    fs::hard_link(&native_path, &hard_link_path).expect("create input hard link");
    let hard_link_alias = run_compare(
        EvidenceKind::Cloud,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &hard_link_path,
    );
    assert!(!hard_link_alias.status.success());
    assert!(
        String::from_utf8_lossy(&hard_link_alias.stderr)
            .contains("output aliases the native input")
    );
    assert_eq!(
        fs::read(&native_path).expect("read native after alias check"),
        native_before_alias
    );
    assert_eq!(
        fs::read(&hard_link_path).expect("read hard link after alias check"),
        native_before_alias
    );

    let direct_alias = compare_files(
        EvidenceKind::Cloud,
        &manifest_path,
        &native_path,
        &cinnabar_path,
        &hard_link_path,
    );
    assert!(matches!(
        direct_alias,
        Err(EvidenceError::OutputAliasesInput { role: "native" })
    ));
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
