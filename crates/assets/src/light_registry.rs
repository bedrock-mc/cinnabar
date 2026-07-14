use sha2::{Digest, Sha256};

use crate::AssetError;

const LIGHT_REGISTRY_MAGIC: &[u8; 8] = b"LREG1001";
const LIGHT_REGISTRY_PROTOCOL: u32 = 1001;
const LIGHT_REGISTRY_HEADER_BYTES: usize = 48;
const HASH_BYTES: usize = 32;
const MAX_LIGHT_RECORDS: usize = 65_536;

/// Authoritative per-runtime-state light emission and filtering nibbles.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LightProperties {
    emission: u8,
    filter: u8,
}

impl LightProperties {
    pub const OPAQUE_DARK: Self = Self {
        emission: 0,
        filter: 15,
    };
    pub fn new(emission: u8, filter: u8) -> Result<Self, AssetError> {
        if emission > 15 || filter > 15 {
            return Err(invalid("light emission and filter must fit one nibble"));
        }
        Ok(Self { emission, filter })
    }

    #[must_use]
    pub const fn emission(self) -> u8 {
        self.emission
    }

    #[must_use]
    pub const fn filter(self) -> u8 {
        self.filter
    }

    #[must_use]
    pub const fn packed(self) -> u8 {
        self.emission | self.filter << 4
    }

    const fn from_packed(packed: u8) -> Self {
        Self {
            emission: packed & 0x0f,
            filter: packed >> 4,
        }
    }
}

/// Decodes a strict `LREG1001` artifact bound to the exact BREG bytes and state count.
pub fn read_light_registry(
    bytes: &[u8],
    breg: &[u8],
    expected_count: usize,
) -> Result<Box<[LightProperties]>, AssetError> {
    if expected_count > MAX_LIGHT_RECORDS {
        return Err(invalid("LREG1001 record count exceeds limit"));
    }
    let expected_len = LIGHT_REGISTRY_HEADER_BYTES
        .checked_add(expected_count)
        .and_then(|length| length.checked_add(HASH_BYTES))
        .ok_or_else(|| invalid("LREG1001 length overflow"))?;
    if bytes.len() != expected_len {
        return Err(invalid(
            "LREG1001 length does not match the BREG state count",
        ));
    }
    if &bytes[..8] != LIGHT_REGISTRY_MAGIC
        || u32::from_le_bytes(bytes[8..12].try_into().expect("fixed protocol"))
            != LIGHT_REGISTRY_PROTOCOL
    {
        return Err(invalid("invalid LREG1001 header"));
    }
    let count = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed count")) as usize;
    if count != expected_count {
        return Err(invalid("LREG1001 record count does not match BREG"));
    }
    if &bytes[16..48] != Sha256::digest(breg).as_slice() {
        return Err(invalid("LREG1001 BREG SHA-256 mismatch"));
    }
    let payload_end = LIGHT_REGISTRY_HEADER_BYTES + count;
    if &bytes[payload_end..] != Sha256::digest(&bytes[..payload_end]).as_slice() {
        return Err(invalid("LREG1001 SHA-256 mismatch"));
    }
    Ok(bytes[LIGHT_REGISTRY_HEADER_BYTES..payload_end]
        .iter()
        .copied()
        .map(LightProperties::from_packed)
        .collect::<Vec<_>>()
        .into_boxed_slice())
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
