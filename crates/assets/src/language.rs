use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const LANGUAGE_CARRIER_MAGIC: [u8; 9] = *b"MCBELANG1";
pub const LANGUAGE_CARRIER_SCHEMA: u32 = 1;
pub const MAX_LANGUAGE_ENTRIES: usize = 16_384;
pub const MAX_LANGUAGE_KEY_BYTES: usize = 1_024;
pub const MAX_LANGUAGE_VALUE_BYTES: usize = 8 * 1_024;
pub const MAX_LANGUAGE_TOTAL_BYTES: usize = 8 * 1_024 * 1_024;

const HEADER_BYTES: usize = 80;
const ENTRY_BYTES: usize = 24;
const HASH_BYTES: usize = 32;
const MAX_CARRIER_BYTES: usize = 16 * 1_024 * 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageCatalogIdentity {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledLanguageCatalog {
    identity: LanguageCatalogIdentity,
    translations: BTreeMap<Box<str>, Box<str>>,
}

pub type RuntimeLanguageCatalog = CompiledLanguageCatalog;

impl CompiledLanguageCatalog {
    pub fn decode(
        bytes: &[u8],
        expected_source_manifest_sha256: [u8; 32],
    ) -> Result<Self, LanguageCatalogError> {
        if expected_source_manifest_sha256 == [0; 32] {
            return Err(LanguageCatalogError::SourceManifestMismatch);
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid_catalog("carrier is shorter than its header"));
        }
        if bytes[..LANGUAGE_CARRIER_MAGIC.len()] != LANGUAGE_CARRIER_MAGIC {
            return Err(invalid_catalog("carrier magic is invalid"));
        }
        let schema = read_u32(bytes, 9)?;
        if schema != LANGUAGE_CARRIER_SCHEMA {
            return Err(invalid_catalog("carrier schema is unsupported"));
        }
        let count = usize::try_from(read_u32(bytes, 13)?)
            .map_err(|_| invalid_catalog("translation count overflows usize"))?;
        if count > MAX_LANGUAGE_ENTRIES {
            return Err(invalid_catalog("translation count exceeds its bound"));
        }
        let source_manifest_sha256 = array_at(bytes, 17)?;
        if source_manifest_sha256 != expected_source_manifest_sha256 {
            return Err(LanguageCatalogError::SourceManifestMismatch);
        }
        let entries_offset = usize::try_from(read_u64(bytes, 49)?)
            .map_err(|_| invalid_catalog("entry offset overflows usize"))?;
        let strings_offset = usize::try_from(read_u64(bytes, 57)?)
            .map_err(|_| invalid_catalog("string offset overflows usize"))?;
        let hash_offset = usize::try_from(read_u64(bytes, 65)?)
            .map_err(|_| invalid_catalog("hash offset overflows usize"))?;
        if entries_offset < HEADER_BYTES
            || strings_offset < entries_offset
            || hash_offset < strings_offset
            || hash_offset.checked_add(HASH_BYTES) != Some(bytes.len())
        {
            return Err(invalid_catalog("carrier offsets are not monotonic"));
        }
        let entries_end = entries_offset
            .checked_add(
                count
                    .checked_mul(ENTRY_BYTES)
                    .ok_or_else(|| invalid_catalog("entry table size overflows usize"))?,
            )
            .ok_or_else(|| invalid_catalog("entry table end overflows usize"))?;
        if entries_end > strings_offset {
            return Err(invalid_catalog("entry table overlaps string storage"));
        }
        let stored_hash = array_at(bytes, hash_offset)?;
        let actual_hash: [u8; 32] = Sha256::digest(&bytes[..hash_offset]).into();
        if stored_hash != actual_hash {
            return Err(LanguageCatalogError::CarrierHashMismatch);
        }

        let mut translations = BTreeMap::new();
        let mut total_bytes = 0usize;
        let mut previous_key: Option<&str> = None;
        for index in 0..count {
            let offset = entries_offset + index * ENTRY_BYTES;
            let key_offset = usize::try_from(read_u64(bytes, offset)?)
                .map_err(|_| invalid_catalog("key offset overflows usize"))?;
            let key_length = usize::try_from(read_u32(bytes, offset + 8)?)
                .map_err(|_| invalid_catalog("key length overflows usize"))?;
            let value_offset = usize::try_from(read_u64(bytes, offset + 12)?)
                .map_err(|_| invalid_catalog("value offset overflows usize"))?;
            let value_length = usize::try_from(read_u32(bytes, offset + 20)?)
                .map_err(|_| invalid_catalog("value length overflows usize"))?;
            if key_length == 0 || key_length > MAX_LANGUAGE_KEY_BYTES {
                return Err(invalid_catalog("translation key exceeds its bound"));
            }
            if value_length > MAX_LANGUAGE_VALUE_BYTES {
                return Err(invalid_catalog("translation value exceeds its bound"));
            }
            let key_end = key_offset
                .checked_add(key_length)
                .ok_or_else(|| invalid_catalog("key range overflows usize"))?;
            let value_end = value_offset
                .checked_add(value_length)
                .ok_or_else(|| invalid_catalog("value range overflows usize"))?;
            if key_offset < strings_offset
                || value_offset < strings_offset
                || key_end > hash_offset
                || value_end > hash_offset
            {
                return Err(invalid_catalog(
                    "translation range is outside string storage",
                ));
            }
            let key = std::str::from_utf8(&bytes[key_offset..key_end])
                .map_err(|_| invalid_catalog("translation key is not UTF-8"))?;
            let value = std::str::from_utf8(&bytes[value_offset..value_end])
                .map_err(|_| invalid_catalog("translation value is not UTF-8"))?;
            if previous_key.is_some_and(|previous| previous >= key) {
                return Err(invalid_catalog("translation keys are not strictly sorted"));
            }
            previous_key = Some(key);
            total_bytes = total_bytes
                .checked_add(key.len())
                .and_then(|total| total.checked_add(value.len()))
                .ok_or_else(|| invalid_catalog("translation storage size overflows usize"))?;
            if total_bytes > MAX_LANGUAGE_TOTAL_BYTES {
                return Err(invalid_catalog("translation storage exceeds its bound"));
            }
            translations.insert(key.into(), value.into());
        }

        Ok(Self {
            identity: LanguageCatalogIdentity {
                schema,
                source_manifest_sha256,
                carrier_sha256: stored_hash,
            },
            translations,
        })
    }

    pub const fn identity(&self) -> LanguageCatalogIdentity {
        self.identity
    }

    pub fn translations(&self) -> &BTreeMap<Box<str>, Box<str>> {
        &self.translations
    }
}

#[derive(Debug, Error)]
pub enum LanguageCatalogError {
    #[error("language carrier source manifest does not match the required startup provenance")]
    SourceManifestMismatch,
    #[error("language carrier SHA-256 does not match its payload")]
    CarrierHashMismatch,
    #[error("invalid MCBELANG1 carrier: {detail}")]
    InvalidCatalog { detail: Box<str> },
    #[error("language file is not valid UTF-8")]
    InvalidUtf8,
    #[error("language file line {line}: {detail}")]
    InvalidLine { line: usize, detail: Box<str> },
}

pub fn parse_language_bytes(
    bytes: &[u8],
) -> Result<BTreeMap<Box<str>, Box<str>>, LanguageCatalogError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LanguageCatalogError::InvalidUtf8)?;
    let mut translations = BTreeMap::new();
    let mut total_bytes = 0usize;
    for (line_number, raw_line) in text.lines().enumerate() {
        if raw_line.len() > MAX_LANGUAGE_LINE_BYTES {
            return Err(invalid_line(line_number, "line exceeds its byte bound"));
        }
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.len() > MAX_LANGUAGE_KEY_BYTES {
            return Err(invalid_line(
                line_number,
                "key is empty or exceeds its byte bound",
            ));
        }
        let value = unescape_language_value(value.trim(), line_number)?;
        if value.len() > MAX_LANGUAGE_VALUE_BYTES {
            return Err(invalid_line(line_number, "value exceeds its byte bound"));
        }
        total_bytes = total_bytes
            .checked_add(key.len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or_else(|| invalid_line(line_number, "catalog size overflows usize"))?;
        if total_bytes > MAX_LANGUAGE_TOTAL_BYTES {
            return Err(invalid_line(line_number, "catalog exceeds its byte bound"));
        }
        if translations.len() == MAX_LANGUAGE_ENTRIES && !translations.contains_key(key) {
            return Err(invalid_line(line_number, "catalog exceeds its entry bound"));
        }
        translations.insert(key.into(), value.into_boxed_str());
    }
    Ok(translations)
}

pub fn encode_language_catalog(
    source_manifest_sha256: [u8; 32],
    translations: &BTreeMap<Box<str>, Box<str>>,
) -> Result<Box<[u8]>, LanguageCatalogError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(LanguageCatalogError::SourceManifestMismatch);
    }
    validate_translations(translations)?;
    let entries_offset = HEADER_BYTES;
    let strings_offset = entries_offset
        .checked_add(
            translations
                .len()
                .checked_mul(ENTRY_BYTES)
                .ok_or_else(|| invalid_catalog("entry table size overflows usize"))?,
        )
        .ok_or_else(|| invalid_catalog("string offset overflows usize"))?;
    let mut bytes = Vec::with_capacity(strings_offset + MAX_LANGUAGE_TOTAL_BYTES.min(1024));
    bytes.extend_from_slice(&LANGUAGE_CARRIER_MAGIC);
    push_u32(&mut bytes, LANGUAGE_CARRIER_SCHEMA);
    push_u32(
        &mut bytes,
        u32::try_from(translations.len())
            .map_err(|_| invalid_catalog("translation count overflows u32"))?,
    );
    bytes.extend_from_slice(&source_manifest_sha256);
    push_u64(&mut bytes, entries_offset)?;
    push_u64(&mut bytes, strings_offset)?;
    let hash_offset_position = bytes.len();
    push_u64(&mut bytes, 0)?;
    bytes.resize(HEADER_BYTES, 0);
    bytes.resize(strings_offset, 0);

    let mut entry_offsets = Vec::with_capacity(translations.len());
    for (key, value) in translations {
        let key_offset = bytes.len();
        bytes.extend_from_slice(key.as_bytes());
        let value_offset = bytes.len();
        bytes.extend_from_slice(value.as_bytes());
        entry_offsets.push((key_offset, key.len(), value_offset, value.len()));
    }
    let mut offset = entries_offset;
    for (key_offset, key_length, value_offset, value_length) in entry_offsets {
        push_u64_at(&mut bytes, offset, key_offset)?;
        push_u32_at(
            &mut bytes,
            offset + 8,
            u32::try_from(key_length).map_err(|_| invalid_catalog("key length overflows u32"))?,
        )?;
        push_u64_at(&mut bytes, offset + 12, value_offset)?;
        push_u32_at(
            &mut bytes,
            offset + 20,
            u32::try_from(value_length)
                .map_err(|_| invalid_catalog("value length overflows u32"))?,
        )?;
        offset += ENTRY_BYTES;
    }
    let hash_offset = bytes.len();
    let hash_offset_bytes = u64::try_from(hash_offset)
        .map_err(|_| invalid_catalog("hash offset overflows u64"))?
        .to_le_bytes();
    bytes[hash_offset_position..hash_offset_position + 8].copy_from_slice(&hash_offset_bytes);
    let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    if bytes.len() > MAX_CARRIER_BYTES {
        return Err(invalid_catalog("language carrier exceeds its byte bound"));
    }
    Ok(bytes.into_boxed_slice())
}

const MAX_LANGUAGE_LINE_BYTES: usize = 16 * 1024;

fn validate_translations(
    translations: &BTreeMap<Box<str>, Box<str>>,
) -> Result<(), LanguageCatalogError> {
    if translations.len() > MAX_LANGUAGE_ENTRIES {
        return Err(invalid_catalog("translation count exceeds its bound"));
    }
    let mut total_bytes = 0usize;
    for (key, value) in translations {
        if key.is_empty() || key.len() > MAX_LANGUAGE_KEY_BYTES {
            return Err(invalid_catalog("translation key exceeds its bound"));
        }
        if value.len() > MAX_LANGUAGE_VALUE_BYTES {
            return Err(invalid_catalog("translation value exceeds its bound"));
        }
        total_bytes = total_bytes
            .checked_add(key.len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or_else(|| invalid_catalog("translation storage size overflows usize"))?;
    }
    if total_bytes > MAX_LANGUAGE_TOTAL_BYTES {
        return Err(invalid_catalog("translation storage exceeds its bound"));
    }
    Ok(())
}

fn unescape_language_value(
    value: &str,
    _line_number: usize,
) -> Result<String, LanguageCatalogError> {
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            result.push('\\');
            break;
        };
        result.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            other => other,
        });
    }
    Ok(result)
}

fn invalid_catalog(detail: impl Into<Box<str>>) -> LanguageCatalogError {
    LanguageCatalogError::InvalidCatalog {
        detail: detail.into(),
    }
}

fn invalid_line(line: usize, detail: impl Into<Box<str>>) -> LanguageCatalogError {
    LanguageCatalogError::InvalidLine {
        line: line + 1,
        detail: detail.into(),
    }
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], LanguageCatalogError> {
    let end = offset
        .checked_add(32)
        .ok_or_else(|| invalid_catalog("hash range overflows usize"))?;
    bytes
        .get(offset..end)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| invalid_catalog("hash range is outside the carrier"))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LanguageCatalogError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid_catalog("u32 range overflows usize"))?;
    bytes
        .get(offset..end)
        .map(|value| u32::from_le_bytes(value.try_into().expect("validated u32 range")))
        .ok_or_else(|| invalid_catalog("u32 range is outside the carrier"))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LanguageCatalogError> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| invalid_catalog("u64 range overflows usize"))?;
    bytes
        .get(offset..end)
        .map(|value| u64::from_le_bytes(value.try_into().expect("validated u64 range")))
        .ok_or_else(|| invalid_catalog("u64 range is outside the carrier"))
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: usize) -> Result<(), LanguageCatalogError> {
    let value = u64::try_from(value).map_err(|_| invalid_catalog("offset overflows u64"))?;
    bytes.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn push_u32_at(bytes: &mut [u8], offset: usize, value: u32) -> Result<(), LanguageCatalogError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid_catalog("u32 range overflows usize"))?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| invalid_catalog("u32 range is outside the carrier"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn push_u64_at(bytes: &mut [u8], offset: usize, value: usize) -> Result<(), LanguageCatalogError> {
    let value = u64::try_from(value).map_err(|_| invalid_catalog("offset overflows u64"))?;
    let end = offset
        .checked_add(8)
        .ok_or_else(|| invalid_catalog("u64 range overflows usize"))?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| invalid_catalog("u64 range is outside the carrier"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}
