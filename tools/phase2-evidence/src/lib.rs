use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use image::codecs::png::PngDecoder;
use image::{DynamicImage, ImageDecoder, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum EvidenceKind {
    Biome,
    Lighting,
    FogAir,
    FogWater,
    FogLava,
    Celestial,
    Cloud,
}

impl EvidenceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Biome => "biome",
            Self::Lighting => "lighting",
            Self::FogAir => "fog-air",
            Self::FogWater => "fog-water",
            Self::FogLava => "fog-lava",
            Self::Celestial => "celestial",
            Self::Cloud => "cloud",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub identity: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    pub maximum_channel_error_rgb8: u8,
    pub maximum_channel_error_linear: f64,
    pub mean_squared_error_linear: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelledSample {
    pub label: String,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonManifest {
    pub crop: Crop,
    pub thresholds: Thresholds,
    #[serde(default)]
    pub allow_alpha_mismatch: bool,
    #[serde(default)]
    pub samples: Vec<LabelledSample>,
}

#[derive(Debug, Clone)]
pub struct ComparisonRequest {
    kind: EvidenceKind,
    manifest: RequestManifest,
    native_png: Vec<u8>,
    cinnabar_png: Vec<u8>,
}

#[derive(Debug, Clone)]
enum RequestManifest {
    Exact(ComparisonManifest),
    WholeImage,
}

impl ComparisonRequest {
    #[must_use]
    pub fn from_bytes(
        kind: EvidenceKind,
        manifest: ComparisonManifest,
        native_png: Vec<u8>,
        cinnabar_png: Vec<u8>,
    ) -> Self {
        Self {
            kind,
            manifest: RequestManifest::Exact(manifest),
            native_png,
            cinnabar_png,
        }
    }

    #[must_use]
    pub fn synthetic(kind: EvidenceKind, native_png: Vec<u8>, cinnabar_png: Vec<u8>) -> Self {
        Self {
            kind,
            manifest: RequestManifest::WholeImage,
            native_png,
            cinnabar_png,
        }
    }

    #[must_use]
    pub fn dimension_mismatch() -> Self {
        Self::synthetic(
            EvidenceKind::Biome,
            encode_test_png(1, 1, &[[0, 0, 0, 255]]),
            encode_test_png(2, 1, &[[0, 0, 0, 255], [0, 0, 0, 255]]),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelledSampleReport {
    pub label: String,
    pub x: u32,
    pub y: u32,
    pub maximum_channel_error_rgb8: u8,
    pub maximum_channel_error_linear: f64,
    pub mean_squared_error_linear: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonReport {
    pub schema: String,
    pub kind: EvidenceKind,
    pub native_pixel_sha256: String,
    pub cinnabar_pixel_sha256: String,
    pub crop: Crop,
    pub sample_count: u64,
    pub maximum_channel_error_rgb8: u8,
    pub maximum_channel_error_linear: f64,
    pub mean_squared_error_linear: f64,
    pub labelled_samples: Vec<LabelledSampleReport>,
    pub thresholds: Thresholds,
    pub passed: bool,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("{role} exceeds the {MAX_INPUT_BYTES}-byte input limit")]
    InputTooLarge { role: &'static str },
    #[error("failed to read {role}: {source}")]
    Read {
        role: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse comparison manifest: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("failed to decode {role} PNG: {source}")]
    Image {
        role: &'static str,
        #[source]
        source: image::ImageError,
    },
    #[error(
        "{role} image dimensions {width}x{height} exceed the {MAX_IMAGE_DIMENSION}x{MAX_IMAGE_DIMENSION} limit"
    )]
    ImageDimensions {
        role: &'static str,
        width: u32,
        height: u32,
    },
    #[error("native and Cinnabar image dimensions differ")]
    DimensionMismatch,
    #[error("crop identity must not be empty")]
    EmptyCropIdentity,
    #[error("crop must have non-zero width and height")]
    EmptyCrop,
    #[error("crop lies outside the {role} image")]
    CropOutOfBounds { role: &'static str },
    #[error("threshold {name} must be finite and non-negative")]
    InvalidThreshold { name: &'static str },
    #[error("duplicate sample label {0:?}")]
    DuplicateSampleLabel(String),
    #[error("sample {label:?} lies outside the comparison crop")]
    SampleOutOfBounds { label: String },
    #[error("{field} must be an identity, not an absolute path")]
    AbsolutePathIdentity { field: &'static str },
    #[error("native and Cinnabar alpha differ at ({x}, {y})")]
    AlphaMismatch { x: u32, y: u32 },
    #[error("output aliases the {role} input")]
    OutputAliasesInput { role: &'static str },
    #[error("failed to write comparison report: {0}")]
    Write(#[source] std::io::Error),
}

pub fn compare(request: ComparisonRequest) -> Result<ComparisonReport, EvidenceError> {
    validate_buffer_size("native PNG", request.native_png.len())?;
    validate_buffer_size("Cinnabar PNG", request.cinnabar_png.len())?;

    let native = decode_png("native", &request.native_png)?;
    let cinnabar = decode_png("Cinnabar", &request.cinnabar_png)?;
    if native.dimensions() != cinnabar.dimensions() {
        return Err(EvidenceError::DimensionMismatch);
    }

    let manifest = match request.manifest {
        RequestManifest::Exact(manifest) => manifest,
        RequestManifest::WholeImage => ComparisonManifest {
            crop: Crop {
                identity: "whole-image".to_owned(),
                x: 0,
                y: 0,
                width: native.width(),
                height: native.height(),
            },
            thresholds: Thresholds {
                maximum_channel_error_rgb8: u8::MAX,
                maximum_channel_error_linear: 1.0,
                mean_squared_error_linear: 1.0,
            },
            allow_alpha_mismatch: false,
            samples: Vec::new(),
        },
    };
    validate_manifest(&manifest, native.dimensions(), cinnabar.dimensions())?;

    let native_hash = hash_crop(&native, &manifest.crop);
    let cinnabar_hash = hash_crop(&cinnabar, &manifest.crop);
    let mut squared_error_sum = 0.0;
    let mut maximum_channel_error_rgb8 = 0;
    let mut maximum_channel_error_linear = 0.0_f64;

    for y in manifest.crop.y..manifest.crop.y + manifest.crop.height {
        for x in manifest.crop.x..manifest.crop.x + manifest.crop.width {
            let native_pixel = native.get_pixel(x, y).0;
            let cinnabar_pixel = cinnabar.get_pixel(x, y).0;
            if !manifest.allow_alpha_mismatch && native_pixel[3] != cinnabar_pixel[3] {
                return Err(EvidenceError::AlphaMismatch { x, y });
            }
            let error = pixel_error(native_pixel, cinnabar_pixel);
            squared_error_sum += error.squared_error_sum;
            maximum_channel_error_rgb8 = maximum_channel_error_rgb8.max(error.maximum_rgb8);
            maximum_channel_error_linear = maximum_channel_error_linear.max(error.maximum_linear);
        }
    }

    let sample_count = u64::from(manifest.crop.width) * u64::from(manifest.crop.height);
    let mean_squared_error_linear = squared_error_sum / (sample_count as f64 * 3.0);
    let labelled_samples = manifest
        .samples
        .iter()
        .map(|sample| {
            let native_pixel = native.get_pixel(sample.x, sample.y).0;
            let cinnabar_pixel = cinnabar.get_pixel(sample.x, sample.y).0;
            let error = pixel_error(native_pixel, cinnabar_pixel);
            LabelledSampleReport {
                label: sample.label.clone(),
                x: sample.x,
                y: sample.y,
                maximum_channel_error_rgb8: error.maximum_rgb8,
                maximum_channel_error_linear: error.maximum_linear,
                mean_squared_error_linear: error.squared_error_sum / 3.0,
            }
        })
        .collect();
    let thresholds = manifest.thresholds;
    let passed = maximum_channel_error_rgb8 <= thresholds.maximum_channel_error_rgb8
        && maximum_channel_error_linear <= thresholds.maximum_channel_error_linear
        && mean_squared_error_linear <= thresholds.mean_squared_error_linear;

    Ok(ComparisonReport {
        schema: "phase2-evidence-comparison-v1".to_owned(),
        kind: request.kind,
        native_pixel_sha256: native_hash,
        cinnabar_pixel_sha256: cinnabar_hash,
        crop: manifest.crop,
        sample_count,
        maximum_channel_error_rgb8,
        maximum_channel_error_linear,
        mean_squared_error_linear,
        labelled_samples,
        thresholds,
        passed,
    })
}

pub fn compare_files(
    kind: EvidenceKind,
    manifest_path: &Path,
    native_path: &Path,
    cinnabar_path: &Path,
    output_path: &Path,
) -> Result<ComparisonReport, EvidenceError> {
    reject_output_alias(output_path, native_path, "native")?;
    reject_output_alias(output_path, cinnabar_path, "Cinnabar")?;

    let manifest_bytes = read_bounded(manifest_path, "manifest")?;
    let manifest: ComparisonManifest = serde_json::from_slice(&manifest_bytes)?;
    let native_png = read_bounded(native_path, "native PNG")?;
    let cinnabar_png = read_bounded(cinnabar_path, "Cinnabar PNG")?;
    let report = compare(ComparisonRequest::from_bytes(
        kind,
        manifest,
        native_png,
        cinnabar_png,
    ))?;
    let mut output = serde_json::to_vec_pretty(&report)?;
    output.push(b'\n');
    let mut output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(EvidenceError::Write)?;
    output_file
        .write_all(&output)
        .map_err(EvidenceError::Write)?;
    Ok(report)
}

fn validate_buffer_size(role: &'static str, length: usize) -> Result<(), EvidenceError> {
    if length as u64 > MAX_INPUT_BYTES {
        return Err(EvidenceError::InputTooLarge { role });
    }
    Ok(())
}

fn read_bounded(path: &Path, role: &'static str) -> Result<Vec<u8>, EvidenceError> {
    let file = File::open(path).map_err(|source| EvidenceError::Read { role, source })?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| EvidenceError::Read { role, source })?;
    validate_buffer_size(role, bytes.len())?;
    Ok(bytes)
}

fn decode_png(role: &'static str, bytes: &[u8]) -> Result<RgbaImage, EvidenceError> {
    let decoder = PngDecoder::new(Cursor::new(bytes))
        .map_err(|source| EvidenceError::Image { role, source })?;
    let (width, height) = decoder.dimensions();
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(EvidenceError::ImageDimensions {
            role,
            width,
            height,
        });
    }
    DynamicImage::from_decoder(decoder)
        .map(DynamicImage::into_rgba8)
        .map_err(|source| EvidenceError::Image { role, source })
}

fn validate_manifest(
    manifest: &ComparisonManifest,
    native_dimensions: (u32, u32),
    cinnabar_dimensions: (u32, u32),
) -> Result<(), EvidenceError> {
    if manifest.crop.identity.is_empty() {
        return Err(EvidenceError::EmptyCropIdentity);
    }
    reject_absolute_path_identity(&manifest.crop.identity, "crop identity")?;
    if manifest.crop.width == 0 || manifest.crop.height == 0 {
        return Err(EvidenceError::EmptyCrop);
    }
    validate_crop(&manifest.crop, native_dimensions, "native")?;
    validate_crop(&manifest.crop, cinnabar_dimensions, "Cinnabar")?;
    validate_finite_non_negative(
        manifest.thresholds.maximum_channel_error_linear,
        "maximum_channel_error_linear",
    )?;
    validate_finite_non_negative(
        manifest.thresholds.mean_squared_error_linear,
        "mean_squared_error_linear",
    )?;

    let crop_right = manifest.crop.x + manifest.crop.width;
    let crop_bottom = manifest.crop.y + manifest.crop.height;
    let mut labels = HashSet::with_capacity(manifest.samples.len());
    for sample in &manifest.samples {
        reject_absolute_path_identity(&sample.label, "sample label")?;
        if !labels.insert(sample.label.as_str()) {
            return Err(EvidenceError::DuplicateSampleLabel(sample.label.clone()));
        }
        if sample.x < manifest.crop.x
            || sample.x >= crop_right
            || sample.y < manifest.crop.y
            || sample.y >= crop_bottom
        {
            return Err(EvidenceError::SampleOutOfBounds {
                label: sample.label.clone(),
            });
        }
    }
    Ok(())
}

fn reject_absolute_path_identity(value: &str, field: &'static str) -> Result<(), EvidenceError> {
    let bytes = value.as_bytes();
    let windows_drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if Path::new(value).is_absolute()
        || value.starts_with('/')
        || value.starts_with('\\')
        || windows_drive_path
    {
        return Err(EvidenceError::AbsolutePathIdentity { field });
    }
    Ok(())
}

fn validate_crop(
    crop: &Crop,
    (image_width, image_height): (u32, u32),
    role: &'static str,
) -> Result<(), EvidenceError> {
    let Some(right) = crop.x.checked_add(crop.width) else {
        return Err(EvidenceError::CropOutOfBounds { role });
    };
    let Some(bottom) = crop.y.checked_add(crop.height) else {
        return Err(EvidenceError::CropOutOfBounds { role });
    };
    if right > image_width || bottom > image_height {
        return Err(EvidenceError::CropOutOfBounds { role });
    }
    Ok(())
}

fn validate_finite_non_negative(value: f64, name: &'static str) -> Result<(), EvidenceError> {
    if !value.is_finite() || value < 0.0 {
        return Err(EvidenceError::InvalidThreshold { name });
    }
    Ok(())
}

fn hash_crop(image: &RgbaImage, crop: &Crop) -> String {
    let mut digest = Sha256::new();
    for y in crop.y..crop.y + crop.height {
        for x in crop.x..crop.x + crop.width {
            digest.update(image.get_pixel(x, y).0);
        }
    }
    format!("{:x}", digest.finalize())
}

#[derive(Debug, Clone, Copy)]
struct PixelError {
    maximum_rgb8: u8,
    maximum_linear: f64,
    squared_error_sum: f64,
}

fn pixel_error(native: [u8; 4], cinnabar: [u8; 4]) -> PixelError {
    let mut maximum_rgb8 = 0;
    let mut maximum_linear = 0.0_f64;
    let mut squared_error_sum = 0.0;
    for channel in 0..3 {
        maximum_rgb8 = maximum_rgb8.max(native[channel].abs_diff(cinnabar[channel]));
        let linear_error =
            (srgb8_to_linear(native[channel]) - srgb8_to_linear(cinnabar[channel])).abs();
        maximum_linear = maximum_linear.max(linear_error);
        squared_error_sum += linear_error * linear_error;
    }
    PixelError {
        maximum_rgb8,
        maximum_linear,
        squared_error_sum,
    }
}

fn srgb8_to_linear(channel: u8) -> f64 {
    let srgb = f64::from(channel) / 255.0;
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn reject_output_alias(
    output_path: &Path,
    input_path: &Path,
    role: &'static str,
) -> Result<(), EvidenceError> {
    let input = canonicalize_for_alias(input_path, role)?;
    let output = canonicalize_for_alias(output_path, "output")?;
    if input == output {
        return Err(EvidenceError::OutputAliasesInput { role });
    }
    Ok(())
}

fn canonicalize_for_alias(path: &Path, role: &'static str) -> Result<PathBuf, EvidenceError> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|source| EvidenceError::Read { role, source });
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|source| EvidenceError::Read { role, source })?;
    let file_name = path.file_name().ok_or_else(|| EvidenceError::Read {
        role,
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name"),
    })?;
    Ok(parent.join(file_name))
}

fn encode_test_png(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;

    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&bytes, width, height, image::ExtendedColorType::Rgba8)
        .expect("synthetic dimensions are valid");
    encoded
}
