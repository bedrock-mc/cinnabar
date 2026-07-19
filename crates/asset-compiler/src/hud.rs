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
const MAX_PROVENANCE_SOURCES: usize = 64;
const PINNED_PACKAGE_NAME: &str = "Microsoft.MinecraftUWP";
const PINNED_PACKAGE_VERSION: &str = "1.26.3301.0";
const PINNED_PACKAGE_ARCHITECTURE: &str = "x64";
const PINNED_PUBLISHER_ID: &str = "8wekyb3d8bbwe";
const PINNED_PROTOCOL: u32 = 1001;
const PINNED_PACK_RELATIVE_PATH: &str = "data/resource_packs/vanilla";
const PINNED_ARTIFACT_POLICY: &str = "local-owned-client-input-only";

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
    package_name: Box<str>,
    package_version: Box<str>,
    package_architecture: Box<str>,
    publisher_id: Box<str>,
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
    #[error("reviewed HUD source {path} does not match Microsoft.MinecraftUWP 1.26.3301.0")]
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
    let mut total_source_bytes = 0usize;
    for source in &manifest.sources {
        let path = root.join(source.path.as_ref());
        if source.bytes > MAX_SOURCE_BYTES {
            return Err(HudCompileError::SourceTooLarge {
                path: path.into_boxed_path(),
                maximum: MAX_SOURCE_BYTES,
            });
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
        let expected_hash = decode_sha256(&source.sha256).ok_or_else(|| {
            HudCompileError::SourceManifestIdentity {
                detail: format!("invalid SHA-256 for {}", source.path).into_boxed_str(),
            }
        })?;
        if bytes.len() != source.bytes || Sha256::digest(&bytes).as_slice() != expected_hash {
            return Err(HudCompileError::SourceIdentity {
                path: path.into_boxed_path(),
            });
        }
        if verified.insert(source.path.clone(), bytes).is_some() {
            return Err(HudCompileError::SourceManifestIdentity {
                detail: format!("duplicate source path {}", source.path).into_boxed_str(),
            });
        }
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
        limits.max_image_width = Some(64);
        limits.max_image_height = Some(64);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|error| HudCompileError::TextureDecode {
                path: path.clone().into_boxed_path(),
                detail: error.to_string().into_boxed_str(),
            })?;
        let rgba8 = image.into_rgba8();
        let (width, height) = rgba8.dimensions();
        let record = manifest
            .sources
            .iter()
            .find(|record| record.path.as_ref() == role.source_path())
            .expect("verified role source must retain its manifest record");
        if record.width != Some(width)
            || record.height != Some(height)
            || [width, height] != role.expected_size()
        {
            return Err(HudCompileError::SourceIdentity {
                path: path.into_boxed_path(),
            });
        }
        let rgba8 = rgba8.into_raw().into_boxed_slice();
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
        || manifest.package_name.as_ref() != PINNED_PACKAGE_NAME
        || manifest.package_version.as_ref() != PINNED_PACKAGE_VERSION
        || manifest.package_architecture.as_ref() != PINNED_PACKAGE_ARCHITECTURE
        || manifest.publisher_id.as_ref() != PINNED_PUBLISHER_ID
        || manifest.protocol != PINNED_PROTOCOL
        || manifest.pack_relative_path.as_ref() != PINNED_PACK_RELATIVE_PATH
        || manifest.artifact_policy.as_ref() != PINNED_ARTIFACT_POLICY
        || manifest.sources.len() > MAX_PROVENANCE_SOURCES
    {
        return Err(HudCompileError::SourceManifestIdentity {
            detail: "reviewed package or protocol contract mismatch".into(),
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
