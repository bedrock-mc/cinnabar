use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const HUD_CARRIER_MAGIC: [u8; 8] = *b"MCBEHUD1";
pub const HUD_CARRIER_SCHEMA: u32 = 1;
pub const MAX_HUD_CARRIER_BYTES: usize = 64 * 1024;
const HEADER_BYTES: usize = 48;
const HASH_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum HudTextureRole {
    HeartBackground = 0,
    HeartFull = 1,
    HeartHalf = 2,
}

impl HudTextureRole {
    pub const ALL: [Self; 3] = [Self::HeartBackground, Self::HeartFull, Self::HeartHalf];

    pub const fn source_path(self) -> &'static str {
        match self {
            Self::HeartBackground => "textures/ui/heart_background.png",
            Self::HeartFull => "textures/ui/heart.png",
            Self::HeartHalf => "textures/ui/heart_half.png",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HudTexture {
    pub role: HudTextureRole,
    pub source_path: Box<str>,
    pub source_bytes: u32,
    pub source_sha256: [u8; 32],
    pub pixels_sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HudPayload {
    textures: Box<[HudTexture]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HudCatalogIdentity {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeHudCatalog {
    identity: HudCatalogIdentity,
    textures: Box<[HudTexture]>,
}

impl RuntimeHudCatalog {
    pub fn decode(
        bytes: &[u8],
        expected_source_manifest_sha256: [u8; 32],
    ) -> Result<Self, HudCatalogError> {
        if expected_source_manifest_sha256 == [0; 32] {
            return Err(HudCatalogError::SourceManifestMismatch);
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_HUD_CARRIER_BYTES {
            return Err(invalid_carrier("HUD carrier length is outside its bound"));
        }
        if bytes[..8] != HUD_CARRIER_MAGIC {
            return Err(invalid_carrier("HUD carrier magic is invalid"));
        }
        if u32_at(bytes, 8)? != HUD_CARRIER_SCHEMA {
            return Err(invalid_carrier("HUD carrier schema is unsupported"));
        }
        let source_manifest_sha256 = array_at(bytes, 12)?;
        if source_manifest_sha256 != expected_source_manifest_sha256 {
            return Err(HudCatalogError::SourceManifestMismatch);
        }
        let payload_bytes = usize::try_from(u32_at(bytes, 44)?)
            .map_err(|_| invalid_carrier("HUD payload length overflow"))?;
        let hash_offset = HEADER_BYTES
            .checked_add(payload_bytes)
            .ok_or_else(|| invalid_carrier("HUD payload length overflow"))?;
        if hash_offset.checked_add(HASH_BYTES) != Some(bytes.len()) {
            return Err(invalid_carrier("HUD carrier sections are not contiguous"));
        }
        let expected_hash: [u8; 32] = Sha256::digest(&bytes[..hash_offset]).into();
        let carrier_sha256 = array_at(bytes, hash_offset)?;
        if carrier_sha256 != expected_hash {
            return Err(HudCatalogError::CarrierHashMismatch);
        }
        let payload: HudPayload = serde_json::from_slice(&bytes[HEADER_BYTES..hash_offset])
            .map_err(|source| HudCatalogError::InvalidCarrier {
                detail: format!("HUD payload JSON is invalid: {source}").into(),
            })?;
        validate_textures(&payload.textures)
            .map_err(|detail| HudCatalogError::InvalidCarrier { detail })?;
        Ok(Self {
            identity: HudCatalogIdentity {
                schema: HUD_CARRIER_SCHEMA,
                source_manifest_sha256,
                carrier_sha256,
            },
            textures: payload.textures,
        })
    }

    pub const fn identity(&self) -> HudCatalogIdentity {
        self.identity
    }

    pub fn textures(&self) -> &[HudTexture] {
        &self.textures
    }

    pub fn texture(&self, role: HudTextureRole) -> &HudTexture {
        &self.textures[role as usize]
    }
}

pub fn encode_hud_catalog(
    source_manifest_sha256: [u8; 32],
    textures: &[HudTexture],
) -> Result<Box<[u8]>, HudCatalogError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(HudCatalogError::SourceManifestMismatch);
    }
    validate_textures(textures).map_err(|detail| HudCatalogError::InvalidCatalog { detail })?;
    let payload = serde_json::to_vec(&HudPayload {
        textures: textures.into(),
    })
    .map_err(|source| HudCatalogError::InvalidCatalog {
        detail: format!("HUD payload JSON could not be encoded: {source}").into(),
    })?;
    let total = HEADER_BYTES
        .checked_add(payload.len())
        .and_then(|value| value.checked_add(HASH_BYTES))
        .filter(|value| *value <= MAX_HUD_CARRIER_BYTES)
        .ok_or_else(|| invalid_catalog("HUD carrier exceeds its byte bound"))?;
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(&HUD_CARRIER_MAGIC);
    bytes.extend_from_slice(&HUD_CARRIER_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&source_manifest_sha256);
    bytes.extend_from_slice(
        &u32::try_from(payload.len())
            .map_err(|_| invalid_catalog("HUD payload length exceeds u32"))?
            .to_le_bytes(),
    );
    debug_assert_eq!(bytes.len(), HEADER_BYTES);
    bytes.extend_from_slice(&payload);
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&digest);
    Ok(bytes.into_boxed_slice())
}

#[derive(Debug, Error)]
pub enum HudCatalogError {
    #[error("HUD carrier source manifest does not match the required startup provenance")]
    SourceManifestMismatch,
    #[error("HUD carrier SHA-256 does not match its payload")]
    CarrierHashMismatch,
    #[error("invalid compiled HUD catalog: {detail}")]
    InvalidCatalog { detail: Box<str> },
    #[error("invalid MCBEHUD1 carrier: {detail}")]
    InvalidCarrier { detail: Box<str> },
}

fn validate_textures(textures: &[HudTexture]) -> Result<(), Box<str>> {
    if textures.len() != HudTextureRole::ALL.len() {
        return Err("HUD catalog must contain the exact required texture roles".into());
    }
    for (texture, role) in textures.iter().zip(HudTextureRole::ALL) {
        if texture.role != role || texture.source_path.as_ref() != role.source_path() {
            return Err("HUD textures must use canonical role and source-path order".into());
        }
        if texture.width != 9 || texture.height != 9 {
            return Err("HUD heart textures must preserve the pinned 9x9 geometry".into());
        }
        if texture.source_bytes == 0 || texture.source_sha256 == [0; 32] {
            return Err("HUD texture source provenance is missing".into());
        }
        if texture.rgba8.len() != 9 * 9 * 4 {
            return Err("HUD texture decoded RGBA8 length is invalid".into());
        }
        if texture.pixels_sha256 != <[u8; 32]>::from(Sha256::digest(&texture.rgba8)) {
            return Err("HUD texture decoded-pixel SHA-256 is invalid".into());
        }
    }
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, HudCatalogError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| invalid_carrier("HUD carrier field is truncated"))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], HudCatalogError> {
    bytes
        .get(offset..offset + 32)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| invalid_carrier("HUD carrier hash is truncated"))
}

fn invalid_catalog(detail: impl Into<Box<str>>) -> HudCatalogError {
    HudCatalogError::InvalidCatalog {
        detail: detail.into(),
    }
}

fn invalid_carrier(detail: impl Into<Box<str>>) -> HudCatalogError {
    HudCatalogError::InvalidCarrier {
        detail: detail.into(),
    }
}
