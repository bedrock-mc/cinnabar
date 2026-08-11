//! Bounded, provenance-pinned vanilla sound-definition catalog.
//!
//! This is only a direct named-definition lookup. Numeric/level sound events
//! require separate `sounds.json` routing and must not be resolved through it.

use sha2::{Digest, Sha256};
use std::{cmp::Ordering, io};
use thiserror::Error;

pub const AUDIO_CARRIER_MAGIC: &[u8; 8] = b"MCBEAUD1";
const AUDIO_CARRIER_SCHEMA: u32 = 1;
const HEADER_BYTES: usize = 88;
const HASH_BYTES: usize = 32;

pub const MAX_AUDIO_CARRIER_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_AUDIO_DEFINITIONS: usize = 4096;
pub const MAX_AUDIO_ALTERNATIVES: usize = 16_384;
pub const MAX_AUDIO_ALTERNATIVES_PER_DEFINITION: usize = 64;
pub const MAX_AUDIO_IDENTIFIER_BYTES: usize = 256;
pub const MAX_AUDIO_CATEGORY_BYTES: usize = 32;
pub const MAX_AUDIO_SUBTITLE_BYTES: usize = 256;
pub const MAX_AUDIO_PATH_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioAlternative {
    /// Whether the source used an object rather than the scalar-name shorthand.
    pub object_form: bool,
    pub name: Box<str>,
    pub weight: u16,
    pub volume: Option<f32>,
    pub pitch: Option<f32>,
    pub is_3d: Option<bool>,
    pub stream: Option<bool>,
    pub load_on_low_memory: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioDefinition {
    pub identifier: Box<str>,
    pub category: Option<Box<str>>,
    pub subtitle: Option<Box<str>>,
    pub min_distance: Option<f32>,
    pub max_distance: Option<f32>,
    pub volume: Option<f32>,
    pub pitch: Option<f32>,
    /// Exact source token; the pinned catalog spells this as a string.
    pub use_legacy_max_distance: Option<Box<str>>,
    pub alternatives: Box<[AudioAlternative]>,
}

#[derive(Clone, Debug)]
pub struct RuntimeAudioCatalog {
    source_manifest_sha256: [u8; 32],
    sound_definitions_sha256: [u8; 32],
    envelope_sha256: [u8; 32],
    definitions: Box<[AudioDefinition]>,
}

impl RuntimeAudioCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, AudioCatalogError> {
        if !(HEADER_BYTES + HASH_BYTES..=MAX_AUDIO_CARRIER_BYTES).contains(&bytes.len()) {
            return invalid("carrier byte length is outside its bound");
        }
        if bytes.get(..8) != Some(AUDIO_CARRIER_MAGIC) {
            return invalid("invalid MCBEAUD1 magic");
        }
        if u32_at(bytes, 8)? != AUDIO_CARRIER_SCHEMA {
            return invalid("unsupported MCBEAUD1 schema");
        }
        let definition_count = count_at(bytes, 12, MAX_AUDIO_DEFINITIONS, "definition")?;
        let alternative_count = count_at(bytes, 16, MAX_AUDIO_ALTERNATIVES, "alternative")?;
        let payload_len = count_at(bytes, 20, MAX_AUDIO_CARRIER_BYTES, "payload")?;
        let expected_len = HEADER_BYTES
            .checked_add(payload_len)
            .and_then(|length| length.checked_add(HASH_BYTES))
            .ok_or_else(|| error("carrier length overflow"))?;
        if expected_len != bytes.len() {
            return invalid("noncanonical carrier section lengths");
        }
        let hash_offset = bytes.len() - HASH_BYTES;
        let expected_hash: [u8; 32] = Sha256::digest(&bytes[..hash_offset]).into();
        if bytes[hash_offset..] != expected_hash {
            return invalid("carrier envelope hash mismatch");
        }

        let source_manifest_sha256 = array_at(bytes, 24)?;
        let sound_definitions_sha256 = array_at(bytes, 56)?;
        let mut cursor = Cursor::new(&bytes[HEADER_BYTES..hash_offset]);
        let mut definitions = Vec::with_capacity(definition_count);
        let mut decoded_alternatives = 0usize;
        for _ in 0..definition_count {
            let identifier = cursor.string(MAX_AUDIO_IDENTIFIER_BYTES)?;
            let category = cursor.optional_string(MAX_AUDIO_CATEGORY_BYTES)?;
            let subtitle = cursor.optional_string(MAX_AUDIO_SUBTITLE_BYTES)?;
            let min_distance = cursor.optional_f32()?;
            let max_distance = cursor.optional_f32()?;
            let volume = cursor.optional_f32()?;
            let pitch = cursor.optional_f32()?;
            let use_legacy_max_distance = cursor.optional_string(MAX_AUDIO_CATEGORY_BYTES)?;
            let count = usize::from(cursor.u16()?);
            if count > MAX_AUDIO_ALTERNATIVES_PER_DEFINITION
                || decoded_alternatives.saturating_add(count) > alternative_count
            {
                return invalid("per-definition alternative count is outside its bound");
            }
            let mut alternatives = Vec::with_capacity(count);
            for _ in 0..count {
                let object_form = cursor.bool()?;
                let name = cursor.string(MAX_AUDIO_PATH_BYTES)?;
                let weight = cursor.u16()?;
                if weight == 0 {
                    return invalid("alternative weight must be positive");
                }
                alternatives.push(AudioAlternative {
                    object_form,
                    name,
                    weight,
                    volume: cursor.optional_f32()?,
                    pitch: cursor.optional_f32()?,
                    is_3d: cursor.optional_bool()?,
                    stream: cursor.optional_bool()?,
                    load_on_low_memory: cursor.optional_bool()?,
                });
            }
            if !alternatives
                .windows(2)
                .all(|pair| alternative_cmp(&pair[0], &pair[1]).is_le())
            {
                return invalid("alternatives are not in canonical order");
            }
            decoded_alternatives += count;
            definitions.push(AudioDefinition {
                identifier,
                category,
                subtitle,
                min_distance,
                max_distance,
                volume,
                pitch,
                use_legacy_max_distance,
                alternatives: alternatives.into_boxed_slice(),
            });
        }
        if decoded_alternatives != alternative_count || !cursor.is_empty() {
            return invalid("carrier counts or trailing payload do not match the envelope");
        }
        if !definitions
            .windows(2)
            .all(|pair| pair[0].identifier < pair[1].identifier)
        {
            return invalid("definitions are not sorted and unique");
        }
        Ok(Self {
            source_manifest_sha256,
            sound_definitions_sha256,
            envelope_sha256: expected_hash,
            definitions: definitions.into_boxed_slice(),
        })
    }

    pub fn lookup(&self, identifier: &str) -> Option<&AudioDefinition> {
        self.definitions
            .binary_search_by(|definition| definition.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.definitions[index])
    }

    pub fn definitions(&self) -> &[AudioDefinition] {
        &self.definitions
    }

    pub fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    pub fn sound_definitions_sha256(&self) -> [u8; 32] {
        self.sound_definitions_sha256
    }

    pub fn envelope_sha256(&self) -> [u8; 32] {
        self.envelope_sha256
    }
}

pub fn encode_audio_catalog(
    source_manifest_sha256: [u8; 32],
    sound_definitions_sha256: [u8; 32],
    definitions: &[AudioDefinition],
) -> Result<Vec<u8>, AudioCatalogError> {
    if definitions.len() > MAX_AUDIO_DEFINITIONS {
        return invalid("definition count exceeds bound");
    }
    let mut definitions = definitions.to_vec();
    definitions.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    if definitions
        .windows(2)
        .any(|pair| pair[0].identifier == pair[1].identifier)
    {
        return invalid("definition identifiers must be unique");
    }
    let mut alternative_count = 0usize;
    let mut payload = Vec::new();
    for definition in &mut definitions {
        validate_string(
            &definition.identifier,
            MAX_AUDIO_IDENTIFIER_BYTES,
            "identifier",
        )?;
        validate_optional_string(&definition.category, MAX_AUDIO_CATEGORY_BYTES, "category")?;
        validate_optional_string(&definition.subtitle, MAX_AUDIO_SUBTITLE_BYTES, "subtitle")?;
        validate_optional_string(
            &definition.use_legacy_max_distance,
            MAX_AUDIO_CATEGORY_BYTES,
            "legacy-distance token",
        )?;
        validate_finite_fields(definition)?;
        if definition.alternatives.len() > MAX_AUDIO_ALTERNATIVES_PER_DEFINITION {
            return invalid("per-definition alternative count exceeds bound");
        }
        alternative_count = alternative_count
            .checked_add(definition.alternatives.len())
            .ok_or_else(|| error("alternative count overflow"))?;
        if alternative_count > MAX_AUDIO_ALTERNATIVES {
            return invalid("alternative count exceeds bound");
        }
        definition.alternatives.sort_by(alternative_cmp);
        push_string(&mut payload, &definition.identifier)?;
        push_optional_string(&mut payload, definition.category.as_deref())?;
        push_optional_string(&mut payload, definition.subtitle.as_deref())?;
        push_optional_f32(&mut payload, definition.min_distance);
        push_optional_f32(&mut payload, definition.max_distance);
        push_optional_f32(&mut payload, definition.volume);
        push_optional_f32(&mut payload, definition.pitch);
        push_optional_string(&mut payload, definition.use_legacy_max_distance.as_deref())?;
        push_u16(&mut payload, definition.alternatives.len())?;
        for alternative in &definition.alternatives {
            validate_string(&alternative.name, MAX_AUDIO_PATH_BYTES, "sound path")?;
            if alternative.weight == 0 {
                return invalid("alternative weight must be positive");
            }
            for value in [alternative.volume, alternative.pitch]
                .into_iter()
                .flatten()
            {
                if !value.is_finite() {
                    return invalid("alternative numeric modifier must be finite");
                }
            }
            payload.push(u8::from(alternative.object_form));
            push_string(&mut payload, &alternative.name)?;
            payload.extend_from_slice(&alternative.weight.to_le_bytes());
            push_optional_f32(&mut payload, alternative.volume);
            push_optional_f32(&mut payload, alternative.pitch);
            push_optional_bool(&mut payload, alternative.is_3d);
            push_optional_bool(&mut payload, alternative.stream);
            push_optional_bool(&mut payload, alternative.load_on_low_memory);
        }
    }
    let length = HEADER_BYTES
        .checked_add(payload.len())
        .and_then(|length| length.checked_add(HASH_BYTES))
        .ok_or_else(|| error("carrier length overflow"))?;
    if length > MAX_AUDIO_CARRIER_BYTES {
        return invalid("carrier byte length exceeds bound");
    }
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(AUDIO_CARRIER_MAGIC);
    bytes.extend_from_slice(&AUDIO_CARRIER_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&(definitions.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(alternative_count as u32).to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&source_manifest_sha256);
    bytes.extend_from_slice(&sound_definitions_sha256);
    bytes.extend_from_slice(&payload);
    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    Ok(bytes)
}

fn validate_finite_fields(definition: &AudioDefinition) -> Result<(), AudioCatalogError> {
    for value in [
        definition.min_distance,
        definition.max_distance,
        definition.volume,
        definition.pitch,
    ]
    .into_iter()
    .flatten()
    {
        if !value.is_finite() {
            return invalid("definition numeric field must be finite");
        }
    }
    Ok(())
}

fn alternative_cmp(left: &AudioAlternative, right: &AudioAlternative) -> Ordering {
    left.name
        .cmp(&right.name)
        .then(left.object_form.cmp(&right.object_form))
        .then(left.weight.cmp(&right.weight))
        .then(option_f32_cmp(left.volume, right.volume))
        .then(option_f32_cmp(left.pitch, right.pitch))
        .then(left.is_3d.cmp(&right.is_3d))
        .then(left.stream.cmp(&right.stream))
        .then(left.load_on_low_memory.cmp(&right.load_on_low_memory))
}

fn option_f32_cmp(left: Option<f32>, right: Option<f32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn validate_optional_string(
    value: &Option<Box<str>>,
    maximum: usize,
    field: &str,
) -> Result<(), AudioCatalogError> {
    if let Some(value) = value {
        validate_string(value, maximum, field)?;
    }
    Ok(())
}

fn validate_string(value: &str, maximum: usize, field: &str) -> Result<(), AudioCatalogError> {
    if value.is_empty() || value.len() > maximum {
        return invalid(format!("{field} byte length is outside its bound"));
    }
    Ok(())
}

fn push_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), AudioCatalogError> {
    push_u16(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_optional_string(bytes: &mut Vec<u8>, value: Option<&str>) -> Result<(), AudioCatalogError> {
    bytes.push(u8::from(value.is_some()));
    if let Some(value) = value {
        push_string(bytes, value)?;
    }
    Ok(())
}

fn push_optional_f32(bytes: &mut Vec<u8>, value: Option<f32>) {
    bytes.push(u8::from(value.is_some()));
    if let Some(value) = value {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

fn push_optional_bool(bytes: &mut Vec<u8>, value: Option<bool>) {
    bytes.push(match value {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    });
}

fn push_u16(bytes: &mut Vec<u8>, value: usize) -> Result<(), AudioCatalogError> {
    let value = u16::try_from(value).map_err(|_| error("field length exceeds u16"))?;
    bytes.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn count_at(
    bytes: &[u8],
    offset: usize,
    maximum: usize,
    field: &str,
) -> Result<usize, AudioCatalogError> {
    let value =
        usize::try_from(u32_at(bytes, offset)?).map_err(|_| error("count exceeds platform"))?;
    if value > maximum {
        return invalid(format!("{field} count exceeds bound"));
    }
    Ok(value)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AudioCatalogError> {
    Ok(u32::from_le_bytes(array_at(bytes, offset)?))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AudioCatalogError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or_else(|| error("field offset overflow"))?,
        )
        .ok_or_else(|| error("truncated carrier field"))?
        .try_into()
        .map_err(|_| error("invalid carrier field"))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], AudioCatalogError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| error("payload offset overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| error("truncated carrier payload"))?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, AudioCatalogError> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Result<bool, AudioCatalogError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => invalid("noncanonical boolean"),
        }
    }
    fn optional_bool(&mut self) -> Result<Option<bool>, AudioCatalogError> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(false)),
            2 => Ok(Some(true)),
            _ => invalid("noncanonical optional boolean"),
        }
    }
    fn u16(&mut self) -> Result<u16, AudioCatalogError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn string(&mut self, maximum: usize) -> Result<Box<str>, AudioCatalogError> {
        let length = usize::from(self.u16()?);
        if length == 0 || length > maximum {
            return invalid("string byte length is outside its bound");
        }
        std::str::from_utf8(self.take(length)?)
            .map(Box::<str>::from)
            .map_err(|_| error("carrier string is not UTF-8"))
    }
    fn optional_string(&mut self, maximum: usize) -> Result<Option<Box<str>>, AudioCatalogError> {
        if self.bool()? {
            Ok(Some(self.string(maximum)?))
        } else {
            Ok(None)
        }
    }
    fn optional_f32(&mut self) -> Result<Option<f32>, AudioCatalogError> {
        if !self.bool()? {
            return Ok(None);
        }
        let value = f32::from_bits(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ));
        if !value.is_finite() {
            return invalid("numeric field is not finite");
        }
        Ok(Some(value))
    }
}

#[derive(Debug, Error)]
pub enum AudioCatalogError {
    #[error("invalid audio catalog: {0}")]
    Invalid(Box<str>),
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn error(detail: impl Into<Box<str>>) -> AudioCatalogError {
    AudioCatalogError::Invalid(detail.into())
}
fn invalid<T>(detail: impl Into<Box<str>>) -> Result<T, AudioCatalogError> {
    Err(error(detail))
}
