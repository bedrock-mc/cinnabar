//! Compiled particle carrier: verbatim `particles/*.json` effect definitions keyed by
//! identifier, plus the RGBA8 textures they reference keyed by logical texture path.
//!
//! Effects stay unresolved JSON because a joined server pack adds or overrides effects
//! at runtime; the render crate parses them.

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::AssetError;

pub const PARTICLE_CARRIER_MAGIC: [u8; 8] = *b"MCBEPT01";
pub const PARTICLE_CARRIER_VERSION: u32 = 1;
pub const MAX_PARTICLE_TEXTURES: usize = 512;
pub const MAX_PARTICLE_EFFECTS: usize = 4096;
pub const MAX_PARTICLE_KEY_BYTES: usize = 256;
pub const MAX_PARTICLE_TEXTURE_SIDE: u32 = 1024;
pub const MAX_PARTICLE_EFFECT_BYTES: usize = 512 * 1024;
pub const MAX_PARTICLE_CARRIER_BYTES: usize = 32 * 1024 * 1024;
/// Native actor flame rendering samples this vertically stacked square-frame texture.
pub const ACTOR_FLAME_TEXTURE: &str = "textures/flame_atlas";

const HEADER_BYTES: usize = 72;
const HASH_BYTES: usize = 32;

/// One particle texture: raw RGBA8, `width * height * 4` bytes, row-major.
#[derive(Clone, Eq, PartialEq)]
pub struct ParticleTexture {
    pub path: Box<str>,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

impl std::fmt::Debug for ParticleTexture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParticleTexture")
            .field("path", &self.path)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// One raw `particle_effect` json document keyed by its `description.identifier`.
#[derive(Clone, Eq, PartialEq)]
pub struct ParticleEffectFile {
    pub identifier: Box<str>,
    pub bytes: Arc<[u8]>,
}

impl std::fmt::Debug for ParticleEffectFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParticleEffectFile")
            .field("identifier", &self.identifier)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Decoded, validated particle carrier; textures and effects are sorted by key.
#[derive(Clone)]
pub struct RuntimeParticleAssets {
    source_manifest_sha256: [u8; 32],
    textures: Arc<[ParticleTexture]>,
    effects: Arc<[ParticleEffectFile]>,
}

impl std::fmt::Debug for RuntimeParticleAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeParticleAssets")
            .field("textures", &self.textures.len())
            .field("effects", &self.effects.len())
            .finish_non_exhaustive()
    }
}

impl RuntimeParticleAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > MAX_PARTICLE_CARRIER_BYTES {
            return Err(invalid("particle carrier exceeds bound"));
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != PARTICLE_CARRIER_MAGIC
            || read_u32(bytes, 8)? != PARTICLE_CARRIER_VERSION
        {
            return Err(invalid("unsupported particle carrier header"));
        }
        let texture_count = read_u32(bytes, 12)? as usize;
        let effect_count = read_u32(bytes, 16)? as usize;
        if read_u32(bytes, 20)? != 0 || read_u32(bytes, 24)? != 0 || read_u32(bytes, 28)? != 0 {
            return Err(invalid("noncanonical particle carrier padding"));
        }
        let source_manifest_sha256 = read_array::<32>(bytes, 32)?;
        let payload_end = usize::try_from(u64::from_le_bytes(read_array(bytes, 64)?))
            .map_err(|_| invalid("particle carrier offset exceeds platform"))?;
        if texture_count > MAX_PARTICLE_TEXTURES
            || effect_count > MAX_PARTICLE_EFFECTS
            || source_manifest_sha256 == [0; 32]
            || payload_end < HEADER_BYTES
            || bytes.len()
                != payload_end
                    .checked_add(HASH_BYTES)
                    .ok_or_else(|| invalid("particle carrier length overflow"))?
        {
            return Err(invalid("noncanonical particle carrier layout"));
        }
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(invalid("particle carrier envelope hash mismatch"));
        }

        let mut cursor = HEADER_BYTES;
        let mut textures: Vec<ParticleTexture> = Vec::with_capacity(texture_count);
        for _ in 0..texture_count {
            let path = read_key(bytes, &mut cursor, payload_end)?;
            let width = u32::from(read_u16(bytes, cursor)?);
            let height = u32::from(read_u16(bytes, cursor + 2)?);
            cursor += 4;
            if width == 0
                || height == 0
                || width > MAX_PARTICLE_TEXTURE_SIDE
                || height > MAX_PARTICLE_TEXTURE_SIDE
            {
                return Err(invalid("particle texture dimensions exceed bounds"));
            }
            let length = pixel_length(width, height)?;
            let end = cursor
                .checked_add(length)
                .filter(|end| *end <= payload_end)
                .ok_or_else(|| invalid("particle texture runs past the payload"))?;
            if textures
                .last()
                .is_some_and(|previous| previous.path.as_ref() >= path.as_str())
            {
                return Err(invalid("particle textures are not strictly sorted"));
            }
            textures.push(ParticleTexture {
                path: path.into(),
                width,
                height,
                rgba8: Arc::from(&bytes[cursor..end]),
            });
            cursor = end;
        }
        let mut effects: Vec<ParticleEffectFile> = Vec::with_capacity(effect_count);
        for _ in 0..effect_count {
            let identifier = read_key(bytes, &mut cursor, payload_end)?;
            let length = read_u32(bytes, cursor)? as usize;
            cursor += 4;
            let end = cursor
                .checked_add(length)
                .filter(|end| *end <= payload_end && length <= MAX_PARTICLE_EFFECT_BYTES)
                .ok_or_else(|| invalid("particle effect runs past the payload"))?;
            if effects
                .last()
                .is_some_and(|previous| previous.identifier.as_ref() >= identifier.as_str())
            {
                return Err(invalid("particle effects are not strictly sorted"));
            }
            effects.push(ParticleEffectFile {
                identifier: identifier.into(),
                bytes: Arc::from(&bytes[cursor..end]),
            });
            cursor = end;
        }
        if cursor != payload_end {
            return Err(invalid("trailing particle carrier payload"));
        }
        Ok(Self {
            source_manifest_sha256,
            textures: textures.into(),
            effects: effects.into(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn textures(&self) -> &[ParticleTexture] {
        &self.textures
    }

    #[must_use]
    pub fn texture(&self, path: &str) -> Option<&ParticleTexture> {
        self.textures
            .binary_search_by(|entry| entry.path.as_ref().cmp(path))
            .ok()
            .map(|index| &self.textures[index])
    }

    #[must_use]
    pub fn effects(&self) -> &[ParticleEffectFile] {
        &self.effects
    }

    /// Raw json of the effect with this `description.identifier`.
    #[must_use]
    pub fn effect(&self, identifier: &str) -> Option<&[u8]> {
        self.effects
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| self.effects[index].bytes.as_ref())
    }
}

/// Encodes a carrier from strictly path-sorted textures and identifier-sorted effects.
pub fn encode_particle_catalog(
    source_manifest_sha256: [u8; 32],
    textures: &[ParticleTexture],
    effects: &[ParticleEffectFile],
) -> Result<Vec<u8>, AssetError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid("particle carrier provenance is unset"));
    }
    if textures.len() > MAX_PARTICLE_TEXTURES || effects.len() > MAX_PARTICLE_EFFECTS {
        return Err(invalid("particle carrier record count exceeds bound"));
    }
    let mut payload = Vec::new();
    let mut previous: Option<&str> = None;
    for texture in textures {
        if texture.width == 0
            || texture.height == 0
            || texture.width > MAX_PARTICLE_TEXTURE_SIDE
            || texture.height > MAX_PARTICLE_TEXTURE_SIDE
            || texture.rgba8.len() != pixel_length(texture.width, texture.height)?
        {
            return Err(invalid(
                "particle texture dimensions or pixels exceed bounds",
            ));
        }
        if previous.is_some_and(|previous| previous >= texture.path.as_ref()) {
            return Err(invalid("particle textures are not strictly sorted"));
        }
        write_key(&mut payload, &texture.path)?;
        crate::encoding::append_bounded(
            &mut payload,
            &(texture.width as u16).to_le_bytes(),
            MAX_PARTICLE_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &(texture.height as u16).to_le_bytes(),
            MAX_PARTICLE_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &texture.rgba8,
            MAX_PARTICLE_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
        previous = Some(&texture.path);
    }
    let mut previous: Option<&str> = None;
    for effect in effects {
        if effect.bytes.len() > MAX_PARTICLE_EFFECT_BYTES {
            return Err(invalid("particle effect exceeds byte bound"));
        }
        if previous.is_some_and(|previous| previous >= effect.identifier.as_ref()) {
            return Err(invalid("particle effects are not strictly sorted"));
        }
        write_key(&mut payload, &effect.identifier)?;
        crate::encoding::append_bounded(
            &mut payload,
            &(effect.bytes.len() as u32).to_le_bytes(),
            MAX_PARTICLE_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &effect.bytes,
            MAX_PARTICLE_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
        previous = Some(&effect.identifier);
    }
    let payload_end = HEADER_BYTES
        .checked_add(payload.len())
        .filter(|end| end + HASH_BYTES <= MAX_PARTICLE_CARRIER_BYTES)
        .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
    let mut bytes = vec![0u8; HEADER_BYTES];
    bytes[..8].copy_from_slice(&PARTICLE_CARRIER_MAGIC);
    bytes[8..12].copy_from_slice(&PARTICLE_CARRIER_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(textures.len() as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&(effects.len() as u32).to_le_bytes());
    bytes[32..64].copy_from_slice(&source_manifest_sha256);
    bytes[64..72].copy_from_slice(&(payload_end as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

/// Removes `//` line and `/* */` block comments outside string literals, which vanilla
/// particle json permits.
#[must_use]
pub fn strip_json_comments(source: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;
    while index < source.len() {
        let byte = source[index];
        if in_string {
            out.push(byte);
            if byte == b'\\' && index + 1 < source.len() {
                out.push(source[index + 1]);
                index += 1;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
            out.push(byte);
        } else if byte == b'/' && source.get(index + 1) == Some(&b'/') {
            while index < source.len() && source[index] != b'\n' {
                index += 1;
            }
            continue;
        } else if byte == b'/' && source.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < source.len() && !(source[index] == b'*' && source[index + 1] == b'/')
            {
                index += 1;
            }
            index += 2;
            continue;
        } else {
            out.push(byte);
        }
        index += 1;
    }
    out
}

fn read_key(bytes: &[u8], cursor: &mut usize, payload_end: usize) -> Result<String, AssetError> {
    let length = usize::from(read_u16(bytes, *cursor)?);
    *cursor += 2;
    if length == 0 || length > MAX_PARTICLE_KEY_BYTES {
        return Err(invalid("particle carrier key length is out of bounds"));
    }
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= payload_end)
        .ok_or_else(|| invalid("particle carrier key runs past the payload"))?;
    let key = std::str::from_utf8(&bytes[*cursor..end])
        .map_err(|_| invalid("particle carrier key is not UTF-8"))?
        .to_owned();
    *cursor = end;
    Ok(key)
}

fn write_key(payload: &mut Vec<u8>, key: &str) -> Result<(), AssetError> {
    if key.is_empty() || key.len() > MAX_PARTICLE_KEY_BYTES {
        return Err(invalid("particle carrier key length is out of bounds"));
    }
    crate::encoding::append_bounded(
        payload,
        &(key.len() as u16).to_le_bytes(),
        MAX_PARTICLE_CARRIER_BYTES,
        HEADER_BYTES + HASH_BYTES,
    )
    .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
    crate::encoding::append_bounded(
        payload,
        key.as_bytes(),
        MAX_PARTICLE_CARRIER_BYTES,
        HEADER_BYTES + HASH_BYTES,
    )
    .ok_or_else(|| invalid("particle carrier exceeds bound"))?;
    Ok(())
}

fn pixel_length(width: u32, height: u32) -> Result<usize, AssetError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("particle texture pixel length overflow"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AssetError> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or_else(|| invalid("particle carrier field overflow"))?,
        )
        .ok_or_else(|| invalid("truncated particle carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid particle carrier field"))
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<ParticleTexture>, Vec<ParticleEffectFile>) {
        let textures = vec![
            ParticleTexture {
                path: "textures/particle/a".into(),
                width: 2,
                height: 1,
                rgba8: Arc::from(vec![1u8; 8]),
            },
            ParticleTexture {
                path: "textures/particle/b".into(),
                width: 1,
                height: 1,
                rgba8: Arc::from(vec![2u8; 4]),
            },
        ];
        let effects = vec![ParticleEffectFile {
            identifier: "minecraft:test".into(),
            bytes: Arc::from(br#"{"particle_effect":{}}"#.to_vec()),
        }];
        (textures, effects)
    }

    #[test]
    fn strips_comments_outside_strings_only() {
        let stripped = strip_json_comments(b"{\"a\": \"x//y\", // note\n \"b\": 1 /* c */}");
        assert_eq!(stripped, b"{\"a\": \"x//y\", \n \"b\": 1 }".to_vec());
    }

    #[test]
    fn round_trips_textures_and_effects() {
        let (textures, effects) = sample();
        let bytes = encode_particle_catalog([7; 32], &textures, &effects).unwrap();
        let assets = RuntimeParticleAssets::decode(&bytes).unwrap();
        assert_eq!(assets.texture("textures/particle/a").unwrap().width, 2);
        assert!(assets.texture("textures/particle/zzz").is_none());
        assert!(assets.effect("minecraft:test").is_some());
        assert_eq!(assets.source_manifest_sha256(), [7; 32]);
    }

    #[test]
    fn rejects_unsorted_records_zero_provenance_and_tampering() {
        let (mut textures, effects) = sample();
        assert!(encode_particle_catalog([0; 32], &textures, &effects).is_err());
        let mut bytes = encode_particle_catalog([7; 32], &textures, &effects).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(RuntimeParticleAssets::decode(&bytes).is_err());
        textures.reverse();
        assert!(encode_particle_catalog([7; 32], &textures, &effects).is_err());
    }
}
