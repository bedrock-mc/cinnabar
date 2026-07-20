//! Compiled localization carrier: the pinned pack's `texts/en_US.lang` table
//! reduced to a sorted, bounded key/value string table.
//!
//! The carrier is provenance-pinned like the entity carrier: it embeds the
//! SHA-256 of the canonical `assets/vanilla-source.json` manifest whose
//! archive supplied the language file AND the SHA-256 of the exact
//! `texts/en_US.lang` bytes it was compiled from. The compiler and startup
//! both reject anything that does not match the pinned identities, so a
//! tampered language file beside a canonical manifest cannot reach players.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const LANG_CARRIER_MAGIC: [u8; 8] = *b"MCBELNG1";
pub const LANG_CARRIER_VERSION: u32 = 2;
pub const MAX_LANG_ENTRIES: usize = 32_768;
pub const MAX_LANG_KEY_BYTES: usize = 256;
pub const MAX_LANG_VALUE_BYTES: usize = 1_024;
pub const MAX_LANG_CARRIER_BYTES: usize = 8 * 1024 * 1024;
const HEADER_BYTES: usize = 96;
const HASH_BYTES: usize = 32;

/// SHA-256 of the exact `texts/en_US.lang` bytes (801,848 bytes) inside the
/// pinned official Mojang sample pack v1.26.30.32-preview. The compiler
/// refuses any other source and startup refuses any carrier not compiled
/// from these bytes.
pub const VANILLA_EN_US_LANG_SHA256: [u8; 32] = [
    0xae, 0x1b, 0xab, 0x8a, 0x3a, 0xb0, 0x05, 0x59, 0x21, 0xa9, 0x07, 0x7c, 0x8f, 0x32, 0x44, 0xd5,
    0x3b, 0xb9, 0xa1, 0x65, 0x40, 0x51, 0x0c, 0x4f, 0x0e, 0x07, 0x79, 0x10, 0x3c, 0x52, 0x1e, 0xde,
];

/// One resolved translation entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LangEntry {
    pub key: Box<str>,
    pub value: Arc<str>,
}

/// Decoded, validated localization catalog with binary-search lookup.
pub struct RuntimeLangCatalog {
    source_manifest_sha256: [u8; 32],
    lang_source_sha256: [u8; 32],
    entries: Box<[LangEntry]>,
}

impl std::fmt::Debug for RuntimeLangCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeLangCatalog")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RuntimeLangCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, LangCatalogError> {
        if bytes.len() > MAX_LANG_CARRIER_BYTES {
            return Err(LangCatalogError::Invalid("language carrier exceeds bound"));
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != LANG_CARRIER_MAGIC
            || read_u32(bytes, 8)? != LANG_CARRIER_VERSION
        {
            return Err(LangCatalogError::Invalid(
                "unsupported language carrier header",
            ));
        }
        let count = read_u32(bytes, 12)? as usize;
        let source_manifest_sha256 = read_array::<32>(bytes, 16)?;
        let lang_source_sha256 = read_array::<32>(bytes, 48)?;
        let entries_offset = read_usize(bytes, 80)?;
        let entries_end = read_usize(bytes, 88)?;
        if count > MAX_LANG_ENTRIES
            || entries_offset != HEADER_BYTES
            || entries_end < entries_offset
            || bytes.len()
                != entries_end
                    .checked_add(HASH_BYTES)
                    .ok_or(LangCatalogError::Invalid(
                        "language carrier length overflow",
                    ))?
        {
            return Err(LangCatalogError::Invalid(
                "noncanonical language carrier layout",
            ));
        }
        if Sha256::digest(&bytes[..entries_end]).as_slice() != &bytes[entries_end..] {
            return Err(LangCatalogError::Invalid(
                "language carrier envelope hash mismatch",
            ));
        }
        let mut cursor = entries_offset;
        let mut entries: Vec<LangEntry> = Vec::with_capacity(count);
        for _ in 0..count {
            let key_length = usize::from(read_u16(bytes, cursor)?);
            cursor += 2;
            let key = read_str(bytes, cursor, key_length, MAX_LANG_KEY_BYTES)?;
            cursor += key_length;
            let value_length = usize::from(read_u16(bytes, cursor)?);
            cursor += 2;
            let value = read_str(bytes, cursor, value_length, MAX_LANG_VALUE_BYTES)?;
            cursor += value_length;
            if cursor > entries_end {
                return Err(LangCatalogError::Invalid(
                    "language entry runs past the payload",
                ));
            }
            // Strictly ascending keys give canonical bytes and binary search.
            if entries
                .last()
                .is_some_and(|previous| previous.key.as_ref() >= key)
            {
                return Err(LangCatalogError::Invalid(
                    "language entries are not strictly sorted",
                ));
            }
            entries.push(LangEntry {
                key: key.into(),
                value: Arc::from(value),
            });
        }
        if cursor != entries_end {
            return Err(LangCatalogError::Invalid("trailing language payload"));
        }
        Ok(Self {
            source_manifest_sha256,
            lang_source_sha256,
            entries: entries.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    /// SHA-256 of the exact `texts/en_US.lang` bytes this carrier was
    /// compiled from.
    #[must_use]
    pub const fn lang_source_sha256(&self) -> [u8; 32] {
        self.lang_source_sha256
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn entries(&self) -> &[LangEntry] {
        &self.entries
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The localized template for a translation key, if pinned.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<Arc<str>> {
        self.entries
            .binary_search_by(|entry| entry.key.as_ref().cmp(key))
            .ok()
            .map(|index| Arc::clone(&self.entries[index].value))
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LangCatalogError {
    #[error("invalid language carrier: {0}")]
    Invalid(&'static str),
}

/// Encodes a sorted entry table into the canonical carrier bytes.
pub fn encode_lang_catalog(
    source_manifest_sha256: [u8; 32],
    lang_source_sha256: [u8; 32],
    entries: &[LangEntry],
) -> Result<Vec<u8>, LangCatalogError> {
    if entries.len() > MAX_LANG_ENTRIES {
        return Err(LangCatalogError::Invalid("too many language entries"));
    }
    let mut payload = Vec::new();
    let mut previous_key: Option<&str> = None;
    for entry in entries {
        if entry.key.is_empty()
            || entry.key.len() > MAX_LANG_KEY_BYTES
            || entry.value.len() > MAX_LANG_VALUE_BYTES
        {
            return Err(LangCatalogError::Invalid("language entry exceeds bounds"));
        }
        if previous_key.is_some_and(|previous| previous >= entry.key.as_ref()) {
            return Err(LangCatalogError::Invalid(
                "language entries are not strictly sorted",
            ));
        }
        payload.extend_from_slice(&(entry.key.len() as u16).to_le_bytes());
        payload.extend_from_slice(entry.key.as_bytes());
        payload.extend_from_slice(&(entry.value.len() as u16).to_le_bytes());
        payload.extend_from_slice(entry.value.as_bytes());
        previous_key = Some(entry.key.as_ref());
    }
    let entries_end = HEADER_BYTES
        .checked_add(payload.len())
        .filter(|end| end + HASH_BYTES <= MAX_LANG_CARRIER_BYTES)
        .ok_or(LangCatalogError::Invalid("language carrier exceeds bound"))?;
    let mut bytes = vec![0u8; HEADER_BYTES];
    bytes[..8].copy_from_slice(&LANG_CARRIER_MAGIC);
    bytes[8..12].copy_from_slice(&LANG_CARRIER_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes[16..48].copy_from_slice(&source_manifest_sha256);
    bytes[48..80].copy_from_slice(&lang_source_sha256);
    bytes[80..88].copy_from_slice(&(HEADER_BYTES as u64).to_le_bytes());
    bytes[88..96].copy_from_slice(&(entries_end as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LangCatalogError> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LangCatalogError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_usize(bytes: &[u8], offset: usize) -> Result<usize, LangCatalogError> {
    usize::try_from(u64::from_le_bytes(read_array(bytes, offset)?))
        .map_err(|_| LangCatalogError::Invalid("language carrier offset exceeds platform"))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], LangCatalogError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(LangCatalogError::Invalid("language carrier field overflow"))?,
        )
        .ok_or(LangCatalogError::Invalid(
            "truncated language carrier field",
        ))?
        .try_into()
        .map_err(|_| LangCatalogError::Invalid("invalid language carrier field"))
}

fn read_str(
    bytes: &[u8],
    offset: usize,
    length: usize,
    maximum: usize,
) -> Result<&str, LangCatalogError> {
    if length == 0 && maximum == MAX_LANG_KEY_BYTES {
        return Err(LangCatalogError::Invalid("empty language key"));
    }
    if length > maximum {
        return Err(LangCatalogError::Invalid("language field exceeds bound"));
    }
    let raw = bytes
        .get(
            offset
                ..offset
                    .checked_add(length)
                    .ok_or(LangCatalogError::Invalid("language field overflow"))?,
        )
        .ok_or(LangCatalogError::Invalid("truncated language field"))?;
    std::str::from_utf8(raw).map_err(|_| LangCatalogError::Invalid("language field is not UTF-8"))
}
