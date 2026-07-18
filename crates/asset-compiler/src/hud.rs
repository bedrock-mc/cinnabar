use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use assets::{HudCatalogError, HudTexture, HudTextureRole, encode_hud_catalog};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_SOURCE_BYTES: u64 = 64 * 1024;
const PINNED_SOURCE_MANIFEST_SHA256: [u8; 32] = [
    0xc6, 0xd5, 0xf5, 0x6b, 0x94, 0x2d, 0x70, 0x3a, 0x7a, 0xcd, 0x1f, 0x83, 0xb2, 0xcd,
    0xdb, 0x76, 0x33, 0x06, 0x9e, 0x13, 0x41, 0x2a, 0xd5, 0xa1, 0xc3, 0xbe, 0xae, 0x66,
    0x6e, 0x2e, 0xc6, 0xf6,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledHudCarrier {
    pub bytes: Box<[u8]>,
    pub textures: Box<[HudTexture]>,
    pub source_manifest_sha256: [u8; 32],
}

#[derive(Debug, Error)]
pub enum HudCompileError {
    #[error("HUD source manifest is not the reviewed vanilla pin")]
    SourceManifestMismatch,
    #[error("failed to read HUD texture {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("HUD texture {path} is empty or exceeds its source-byte bound")]
    SourceTooLarge { path: PathBuf },
    #[error("failed to decode HUD texture {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("HUD texture {path} must be exactly 9x9, found {width}x{height}")]
    WrongDimensions {
        path: PathBuf,
        width: u32,
        height: u32,
    },
    #[error("HUD texture path escapes the canonical resource-pack root: {path}")]
    EscapedSource { path: PathBuf },
    #[error(transparent)]
    Carrier(#[from] HudCatalogError),
}

pub fn compile_hud_assets(
    pack: &Path,
    source_manifest: &[u8],
) -> Result<CompiledHudCarrier, HudCompileError> {
    let canonical_manifest = canonical_line_endings(source_manifest);
    let source_manifest_sha256: [u8; 32] = Sha256::digest(canonical_manifest).into();
    if source_manifest_sha256 != PINNED_SOURCE_MANIFEST_SHA256 {
        return Err(HudCompileError::SourceManifestMismatch);
    }
    let canonical_pack = pack
        .canonicalize()
        .map_err(|source| HudCompileError::Io {
            path: pack.to_path_buf(),
            source,
        })?;
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| compile_texture(&canonical_pack, role))
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let bytes = encode_hud_catalog(source_manifest_sha256, &textures)?;
    Ok(CompiledHudCarrier {
        bytes,
        textures,
        source_manifest_sha256,
    })
}

fn canonical_line_endings(source: &[u8]) -> Vec<u8> {
    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        if source[index] == b'\r' {
            canonical.push(b'\n');
            index += 1;
            if source.get(index) == Some(&b'\n') {
                index += 1;
            }
        } else {
            canonical.push(source[index]);
            index += 1;
        }
    }
    canonical
}

fn compile_texture(pack: &Path, role: HudTextureRole) -> Result<HudTexture, HudCompileError> {
    let candidate = pack.join(role.source_path());
    let path = candidate
        .canonicalize()
        .map_err(|source| HudCompileError::Io {
            path: candidate.clone(),
            source,
        })?;
    if !path.starts_with(pack) {
        return Err(HudCompileError::EscapedSource { path });
    }
    let file = File::open(&path).map_err(|source| HudCompileError::Io {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| HudCompileError::Io {
            path: path.clone(),
            source,
        })?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(HudCompileError::SourceTooLarge { path });
    }
    let dimensions = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
        .into_dimensions()
        .map_err(|source| HudCompileError::Decode {
            path: path.clone(),
            source: Box::new(source),
        })?;
    if dimensions != (9, 9) {
        return Err(HudCompileError::WrongDimensions {
            path,
            width: dimensions.0,
            height: dimensions.1,
        });
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(9);
    limits.max_image_height = Some(9);
    limits.max_alloc = Some(9 * 9 * 4);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .map_err(|source| HudCompileError::Decode {
            path: path.clone(),
            source: Box::new(source),
        })?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    Ok(HudTexture {
        role,
        source_path: role.source_path().into(),
        source_bytes: u32::try_from(bytes.len())
            .map_err(|_| HudCompileError::SourceTooLarge { path: path.clone() })?,
        source_sha256: Sha256::digest(&bytes).into(),
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: dimensions.0,
        height: dimensions.1,
        rgba8,
    })
}
