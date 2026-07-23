use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};

use assets::{
    HUD_SOURCE_MANIFEST_SHA256, HudTexture, HudTextureRole, MAX_HUD_TEXTURE_BYTES,
    encode_hud_catalog,
};
use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_SOURCE_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROVENANCE_SOURCES: usize = 128;
// The classic icons sheet is 256x256; every other reviewed source is smaller.
const MAX_SOURCE_IMAGE_SIDE: u32 = 256;
const PINNED_TAG: &str = "v1.26.30.32-preview";
const PINNED_COMMIT: &str = "020f1cf4b2baef78e635d4ce7498eb16a429dcbb";
const PINNED_ARCHIVE: &str = "bedrock-samples-v1.26.30.32-preview-full.zip";
const PINNED_URL: &str = "https://github.com/Mojang/bedrock-samples/releases/download/v1.26.30.32-preview/bedrock-samples-v1.26.30.32-preview-full.zip";
const PINNED_ARCHIVE_SHA256: &str =
    "12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c";
const PINNED_PROTOCOL: u32 = 1001;
const PINNED_PACK_RELATIVE_PATH: &str = "resource_pack";
const PINNED_ARTIFACT_POLICY: &str = "official-mojang-download-local-only";

#[derive(Debug)]
pub struct CompiledHudCarrier {
    pub bytes: Vec<u8>,
    pub report: HudCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub textures: usize,
    pub source_bytes: usize,
    pub decoded_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema: u32,
    tag: Box<str>,
    commit: Box<str>,
    archive: Box<str>,
    url: Box<str>,
    archive_sha256: Box<str>,
    protocol: u32,
    pack_relative_path: Box<str>,
    artifact_policy: Box<str>,
    sources: Vec<SourceRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRecord {
    path: Box<str>,
    bytes: usize,
    sha256: Box<str>,
    width: Option<u32>,
    height: Option<u32>,
    /// Pinned `[x, y, width, height]` region for roles carried as a crop of a
    /// larger reviewed sheet. `width`/`height` describe the cropped texture.
    crop: Option<[u32; 4]>,
}

#[derive(Debug, Error)]
pub enum HudCompileError {
    #[error("HUD source manifest exceeds the {maximum}-byte bound")]
    SourceManifestTooLarge { maximum: usize },
    #[error("HUD source manifest is not the reviewed protocol-1001 identity: {detail}")]
    SourceManifestIdentity { detail: Box<str> },
    #[error("reviewed HUD source {path} could not be read: {source}")]
    SourceRead {
        path: Box<Path>,
        #[source]
        source: std::io::Error,
    },
    #[error("reviewed HUD source {path} exceeds the {maximum}-byte bound")]
    SourceTooLarge { path: Box<Path>, maximum: usize },
    #[error("reviewed HUD source {path} does not match Mojang bedrock-samples v1.26.30.32-preview")]
    SourceIdentity { path: Box<Path> },
    #[error("required HUD texture {path} is not a bounded PNG: {detail}")]
    TextureDecode { path: Box<Path>, detail: Box<str> },
    #[error("required HUD texture carrier is invalid: {0}")]
    Carrier(#[from] assets::HudCatalogError),
}

pub fn compile_hud_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledHudCarrier, HudCompileError> {
    if source_manifest.len() > MAX_SOURCE_MANIFEST_BYTES {
        return Err(HudCompileError::SourceManifestTooLarge {
            maximum: MAX_SOURCE_MANIFEST_BYTES,
        });
    }
    let canonical_manifest = canonical_line_endings(source_manifest);
    let source_manifest_sha256: [u8; 32] = Sha256::digest(&canonical_manifest).into();
    if source_manifest_sha256 != HUD_SOURCE_MANIFEST_SHA256 {
        return Err(HudCompileError::SourceManifestIdentity {
            detail: "manifest bytes differ from assets/hud-source-v1001.json".into(),
        });
    }
    let manifest =
        serde_json::from_slice::<SourceManifest>(&canonical_manifest).map_err(|error| {
            HudCompileError::SourceManifestIdentity {
                detail: error.to_string().into_boxed_str(),
            }
        })?;
    validate_manifest_contract(&manifest)?;

    let mut verified = BTreeMap::<Box<str>, Vec<u8>>::new();
    let mut seen_regions = BTreeMap::<(Box<str>, Option<[u32; 4]>), ()>::new();
    let mut total_source_bytes = 0usize;
    for source in &manifest.sources {
        let path = root.join(source.path.as_ref());
        if source.bytes > MAX_SOURCE_BYTES {
            return Err(HudCompileError::SourceTooLarge {
                path: path.into_boxed_path(),
                maximum: MAX_SOURCE_BYTES,
            });
        }
        if seen_regions
            .insert((source.path.clone(), source.crop), ())
            .is_some()
        {
            return Err(HudCompileError::SourceManifestIdentity {
                detail: format!("duplicate source region {}", source.path).into_boxed_str(),
            });
        }
        let expected_hash = decode_sha256(&source.sha256).ok_or_else(|| {
            HudCompileError::SourceManifestIdentity {
                detail: format!("invalid SHA-256 for {}", source.path).into_boxed_str(),
            }
        })?;
        if let Some(bytes) = verified.get(source.path.as_ref()) {
            // A sheet cropped by several roles repeats its path; every record
            // must pin identical file bytes.
            if bytes.len() != source.bytes || Sha256::digest(bytes).as_slice() != expected_hash {
                return Err(HudCompileError::SourceIdentity {
                    path: path.into_boxed_path(),
                });
            }
            continue;
        }
        let bytes = fs::read(&path).map_err(|source| HudCompileError::SourceRead {
            path: path.clone().into_boxed_path(),
            source,
        })?;
        total_source_bytes = total_source_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_TOTAL_SOURCE_BYTES)
            .ok_or(HudCompileError::SourceTooLarge {
                path: path.clone().into_boxed_path(),
                maximum: MAX_TOTAL_SOURCE_BYTES,
            })?;
        if bytes.len() != source.bytes || Sha256::digest(&bytes).as_slice() != expected_hash {
            return Err(HudCompileError::SourceIdentity {
                path: path.into_boxed_path(),
            });
        }
        verified.insert(source.path.clone(), bytes);
    }

    let mut textures = Vec::with_capacity(HudTextureRole::ALL.len());
    let mut decoded_bytes = 0usize;
    for role in HudTextureRole::ALL {
        let path = root.join(role.source_path());
        let source = verified.get(role.source_path()).ok_or_else(|| {
            HudCompileError::SourceManifestIdentity {
                detail: format!("missing reviewed source {}", role.source_path()).into_boxed_str(),
            }
        })?;
        let mut reader = ImageReader::with_format(std::io::Cursor::new(source), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_alloc = Some(MAX_HUD_TEXTURE_BYTES as u64);
        limits.max_image_width = Some(MAX_SOURCE_IMAGE_SIDE);
        limits.max_image_height = Some(MAX_SOURCE_IMAGE_SIDE);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|error| HudCompileError::TextureDecode {
                path: path.clone().into_boxed_path(),
                detail: error.to_string().into_boxed_str(),
            })?;
        let decoded = image.into_rgba8();
        let record = manifest
            .sources
            .iter()
            .find(|record| {
                record.path.as_ref() == role.source_path() && record.crop == role.source_crop()
            })
            .ok_or_else(|| HudCompileError::SourceManifestIdentity {
                detail: format!("missing reviewed region for {}", role.source_path())
                    .into_boxed_str(),
            })?;
        let (rgba8, width, height) = if let Some([x, y, crop_width, crop_height]) =
            role.source_crop()
        {
            let (sheet_width, sheet_height) = decoded.dimensions();
            let in_bounds = x
                .checked_add(crop_width)
                .is_some_and(|right| right <= sheet_width)
                && y.checked_add(crop_height)
                    .is_some_and(|bottom| bottom <= sheet_height);
            if !in_bounds || crop_width == 0 || crop_height == 0 {
                return Err(HudCompileError::TextureDecode {
                    path: path.clone().into_boxed_path(),
                    detail: "pinned crop region is outside the reviewed sheet".into(),
                });
            }
            let mut cropped = Vec::with_capacity(crop_width as usize * crop_height as usize * 4);
            for row in y..y + crop_height {
                let row_start = ((row as usize * sheet_width as usize) + x as usize) * 4;
                cropped.extend_from_slice(
                    &decoded.as_raw()[row_start..row_start + crop_width as usize * 4],
                );
            }
            (cropped.into_boxed_slice(), crop_width, crop_height)
        } else {
            let (width, height) = decoded.dimensions();
            (decoded.into_raw().into_boxed_slice(), width, height)
        };
        if record.width != Some(width)
            || record.height != Some(height)
            || [width, height] != role.expected_size()
        {
            return Err(HudCompileError::SourceIdentity {
                path: path.into_boxed_path(),
            });
        }
        decoded_bytes = decoded_bytes.checked_add(rgba8.len()).ok_or_else(|| {
            HudCompileError::TextureDecode {
                path: path.clone().into_boxed_path(),
                detail: "decoded HUD byte count overflow".into(),
            }
        })?;
        textures.push(HudTexture {
            role,
            source_bytes: u32::try_from(source.len()).map_err(|_| {
                HudCompileError::SourceTooLarge {
                    path: path.clone().into_boxed_path(),
                    maximum: MAX_SOURCE_BYTES,
                }
            })?,
            source_sha256: Sha256::digest(source).into(),
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8,
        });
    }
    let bytes = encode_hud_catalog(source_manifest_sha256, &textures)?;
    Ok(CompiledHudCarrier {
        report: HudCompileReport {
            source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            textures: textures.len(),
            source_bytes: total_source_bytes,
            decoded_bytes,
        },
        bytes,
    })
}

fn validate_manifest_contract(manifest: &SourceManifest) -> Result<(), HudCompileError> {
    if manifest.schema != 1
        || manifest.tag.as_ref() != PINNED_TAG
        || manifest.commit.as_ref() != PINNED_COMMIT
        || manifest.archive.as_ref() != PINNED_ARCHIVE
        || manifest.url.as_ref() != PINNED_URL
        || manifest.archive_sha256.as_ref() != PINNED_ARCHIVE_SHA256
        || manifest.protocol != PINNED_PROTOCOL
        || manifest.pack_relative_path.as_ref() != PINNED_PACK_RELATIVE_PATH
        || manifest.artifact_policy.as_ref() != PINNED_ARTIFACT_POLICY
        || manifest.sources.len() > MAX_PROVENANCE_SOURCES
    {
        return Err(HudCompileError::SourceManifestIdentity {
            detail: "reviewed sample release or protocol contract mismatch".into(),
        });
    }
    Ok(())
}

fn canonical_line_endings(bytes: &[u8]) -> Cow<'_, [u8]> {
    if !bytes.windows(2).any(|pair| pair == b"\r\n") {
        return Cow::Borrowed(bytes);
    }
    let mut canonical = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes.get(index..index + 2) == Some(b"\r\n") {
            canonical.push(b'\n');
            index += 2;
        } else {
            canonical.push(bytes[index]);
            index += 1;
        }
    }
    Cow::Owned(canonical)
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(decoded)
}
