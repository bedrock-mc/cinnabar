use std::{fs, path::Path};

use assets::{HudTexture, HudTextureRole, MAX_HUD_TEXTURE_BYTES, encode_hud_catalog};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_TEXTURE_BYTES: usize = 1024 * 1024;

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

#[derive(Debug, Error)]
pub enum HudCompileError {
    #[error("required HUD pack manifest {path} could not be read: {source}")]
    ManifestRead {
        path: Box<Path>,
        #[source]
        source: std::io::Error,
    },
    #[error("required HUD pack manifest exceeds the {maximum}-byte bound")]
    ManifestTooLarge { maximum: usize },
    #[error("required HUD pack manifest {path} is not valid JSON: {detail}")]
    ManifestDecode { path: Box<Path>, detail: Box<str> },
    #[error("required HUD texture {path} could not be read: {source}")]
    TextureRead {
        path: Box<Path>,
        #[source]
        source: std::io::Error,
    },
    #[error("required HUD texture {path} exceeds the {maximum}-byte source bound")]
    TextureTooLarge { path: Box<Path>, maximum: usize },
    #[error("required HUD texture {path} is not a bounded PNG: {detail}")]
    TextureDecode { path: Box<Path>, detail: Box<str> },
    #[error("required HUD texture carrier is invalid: {0}")]
    Carrier(#[from] assets::HudCatalogError),
}

pub fn compile_hud_assets(root: &Path) -> Result<CompiledHudCarrier, HudCompileError> {
    let manifest_path = root.join("manifest.json");
    let manifest_metadata =
        fs::metadata(&manifest_path).map_err(|source| HudCompileError::ManifestRead {
            path: manifest_path.clone().into_boxed_path(),
            source,
        })?;
    if manifest_metadata.len() > MAX_SOURCE_MANIFEST_BYTES as u64 {
        return Err(HudCompileError::ManifestTooLarge {
            maximum: MAX_SOURCE_MANIFEST_BYTES,
        });
    }
    let manifest = fs::read(&manifest_path).map_err(|source| HudCompileError::ManifestRead {
        path: manifest_path.clone().into_boxed_path(),
        source,
    })?;
    serde_json::from_slice::<serde_json::Value>(&manifest).map_err(|error| {
        HudCompileError::ManifestDecode {
            path: manifest_path.into_boxed_path(),
            detail: error.to_string().into_boxed_str(),
        }
    })?;
    let source_manifest_sha256 = Sha256::digest(&manifest).into();
    let mut textures = Vec::with_capacity(HudTextureRole::ALL.len());
    let mut source_bytes = 0usize;
    let mut decoded_bytes = 0usize;
    for role in HudTextureRole::ALL {
        let path = root.join(role.source_path());
        let metadata = fs::metadata(&path).map_err(|source| HudCompileError::TextureRead {
            path: path.clone().into_boxed_path(),
            source,
        })?;
        if metadata.len() > MAX_SOURCE_TEXTURE_BYTES as u64 {
            return Err(HudCompileError::TextureTooLarge {
                path: path.clone().into_boxed_path(),
                maximum: MAX_SOURCE_TEXTURE_BYTES,
            });
        }
        let source = fs::read(&path).map_err(|source| HudCompileError::TextureRead {
            path: path.clone().into_boxed_path(),
            source,
        })?;
        let mut reader = ImageReader::with_format(std::io::Cursor::new(&source), ImageFormat::Png);
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
        let rgba8 = rgba8.into_raw().into_boxed_slice();
        let source_len = source.len();
        let source_len_u32 =
            u32::try_from(source_len).map_err(|_| HudCompileError::TextureTooLarge {
                path: path.clone().into_boxed_path(),
                maximum: MAX_SOURCE_TEXTURE_BYTES,
            })?;
        source_bytes =
            source_bytes
                .checked_add(source_len)
                .ok_or(HudCompileError::TextureTooLarge {
                    path: path.clone().into_boxed_path(),
                    maximum: MAX_SOURCE_TEXTURE_BYTES,
                })?;
        decoded_bytes =
            decoded_bytes
                .checked_add(rgba8.len())
                .ok_or(HudCompileError::TextureDecode {
                    path: path.clone().into_boxed_path(),
                    detail: "decoded HUD byte count overflow".into(),
                })?;
        textures.push(HudTexture {
            role,
            source_bytes: source_len_u32,
            source_sha256: Sha256::digest(&source).into(),
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
            source_bytes,
            decoded_bytes,
        },
        bytes,
    })
}
