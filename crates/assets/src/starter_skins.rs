//! Optional carrier for the Dressing Room's starter skins and the humanoid skin geometry.
//!
//! Layout: magic, version, geometry JSON as `len, utf8`, a skin count, then per skin
//! `name len, name, slim, rgba8`, then a SHA-256 of everything before it.

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const STARTER_SKINS_MAGIC: [u8; 8] = *b"MCBESKN1";
pub const STARTER_SKINS_VERSION: u32 = 1;
/// Starter skins are classic 64x64 RGBA8 images.
pub const STARTER_SKIN_SIDE: u32 = 64;
pub const MAX_STARTER_SKIN_GEOMETRY_BYTES: usize = 64 * 1024;
pub const MAX_STARTER_SKINS_BYTES: usize = 128 * 1024;

/// Skin geometry identifiers for the classic and slim player models.
pub const CLASSIC_SKIN_GEOMETRY: &str = "geometry.humanoid.custom";
pub const SLIM_SKIN_GEOMETRY: &str = "geometry.humanoid.customSlim";

const HASH_BYTES: usize = 32;
const HEADER_BYTES: usize = 12;
const MAX_NAME_BYTES: usize = 64;
const RGBA_BYTES: usize = (STARTER_SKIN_SIDE * STARTER_SKIN_SIDE * 4) as usize;

/// A starter skin the carrier holds, with its texture below the vanilla resource pack.
#[derive(Clone, Copy, Debug)]
pub struct StarterSkinSource {
    pub name: &'static str,
    pub texture: &'static str,
    pub slim: bool,
}

/// Starter skins in Dressing Room order; the first is the default.
pub const STARTER_SKIN_SOURCES: [StarterSkinSource; 2] = [
    StarterSkinSource {
        name: "Steve",
        texture: "textures/entity/steve.png",
        slim: false,
    },
    StarterSkinSource {
        name: "Alex",
        texture: "textures/entity/alex.png",
        slim: true,
    },
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StarterSkinsError {
    #[error("invalid starter skin carrier: {0}")]
    Invalid(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StarterSkin {
    pub name: Box<str>,
    pub slim: bool,
    /// `STARTER_SKIN_SIDE` square RGBA8 pixels.
    pub rgba8: Box<[u8]>,
}

/// Skin geometry JSON naming the classic and slim humanoid models, and the starter skins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StarterSkins {
    pub geometry: Box<str>,
    pub skins: Vec<StarterSkin>,
}

impl StarterSkins {
    fn validate(&self) -> Result<(), StarterSkinsError> {
        let invalid = StarterSkinsError::Invalid;
        if self.geometry.is_empty() || self.geometry.len() > MAX_STARTER_SKIN_GEOMETRY_BYTES {
            return Err(invalid("geometry length"));
        }
        if self.skins.is_empty() || self.skins.len() > STARTER_SKIN_SOURCES.len() {
            return Err(invalid("skin count"));
        }
        for skin in &self.skins {
            if skin.name.is_empty() || skin.name.len() > MAX_NAME_BYTES {
                return Err(invalid("skin name length"));
            }
            if skin.rgba8.len() != RGBA_BYTES {
                return Err(invalid("unexpected image dimensions"));
            }
        }
        Ok(())
    }
}

pub fn encode_starter_skins(skins: &StarterSkins) -> Result<Vec<u8>, StarterSkinsError> {
    skins.validate()?;
    let mut bytes = Vec::with_capacity(MAX_STARTER_SKINS_BYTES / 2);
    bytes.extend_from_slice(&STARTER_SKINS_MAGIC);
    bytes.extend_from_slice(&STARTER_SKINS_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(skins.geometry.len() as u32).to_le_bytes());
    bytes.extend_from_slice(skins.geometry.as_bytes());
    bytes.push(skins.skins.len() as u8);
    for skin in &skins.skins {
        bytes.push(skin.name.len() as u8);
        bytes.extend_from_slice(skin.name.as_bytes());
        bytes.push(u8::from(skin.slim));
        bytes.extend_from_slice(&skin.rgba8);
    }
    let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    Ok(bytes)
}

/// Decodes and hash-checks a carrier.
pub fn decode_starter_skins(bytes: &[u8]) -> Result<StarterSkins, StarterSkinsError> {
    let invalid = StarterSkinsError::Invalid;
    if bytes.len() > MAX_STARTER_SKINS_BYTES || bytes.len() < HEADER_BYTES + HASH_BYTES {
        return Err(invalid("bad length"));
    }
    let (body, hash) = bytes.split_at(bytes.len() - HASH_BYTES);
    if hash != Sha256::digest(body).as_slice() {
        return Err(invalid("hash mismatch"));
    }
    if body[..8] != STARTER_SKINS_MAGIC || body[8..12] != STARTER_SKINS_VERSION.to_le_bytes() {
        return Err(invalid("unsupported header"));
    }
    let mut reader = Reader {
        bytes: body,
        at: HEADER_BYTES,
    };
    let geometry_len = u32::from_le_bytes(reader.take(4)?.try_into().expect("four bytes"));
    let geometry = std::str::from_utf8(reader.take(geometry_len as usize)?)
        .map_err(|_| invalid("geometry is not UTF-8"))?;
    let count = reader.byte()?;
    let mut skins = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name_len = reader.byte()?;
        let name = std::str::from_utf8(reader.take(usize::from(name_len))?)
            .map_err(|_| invalid("skin name is not UTF-8"))?;
        let slim = match reader.byte()? {
            0 => false,
            1 => true,
            _ => return Err(invalid("skin model")),
        };
        skins.push(StarterSkin {
            name: name.into(),
            slim,
            rgba8: reader.take(RGBA_BYTES)?.into(),
        });
    }
    if reader.at != body.len() {
        return Err(invalid("trailing bytes"));
    }
    let decoded = StarterSkins {
        geometry: geometry.into(),
        skins,
    };
    decoded.validate()?;
    Ok(decoded)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], StarterSkinsError> {
        let end = self
            .at
            .checked_add(len)
            .ok_or(StarterSkinsError::Invalid("truncated"))?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(StarterSkinsError::Invalid("truncated"))?;
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, StarterSkinsError> {
        Ok(self.take(1)?[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StarterSkins {
        StarterSkins {
            geometry: r#"{"format_version":"1.8.0"}"#.into(),
            skins: STARTER_SKIN_SOURCES
                .iter()
                .enumerate()
                .map(|(index, source)| StarterSkin {
                    name: source.name.into(),
                    slim: source.slim,
                    rgba8: vec![index as u8; RGBA_BYTES].into(),
                })
                .collect(),
        }
    }

    #[test]
    fn round_trips() {
        let bytes = encode_starter_skins(&sample()).unwrap();
        assert!(bytes.len() <= MAX_STARTER_SKINS_BYTES);
        assert_eq!(decode_starter_skins(&bytes).unwrap(), sample());
    }

    #[test]
    fn rejects_corruption_truncation_and_wrong_sizes() {
        let mut bytes = encode_starter_skins(&sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(decode_starter_skins(&bytes).is_err());
        let bytes = encode_starter_skins(&sample()).unwrap();
        assert!(decode_starter_skins(&bytes[..bytes.len() - 1]).is_err());
        let mut wrong = sample();
        wrong.skins[0].rgba8 = vec![0; RGBA_BYTES - 4].into();
        assert!(encode_starter_skins(&wrong).is_err());
        wrong = sample();
        wrong.geometry = "".into();
        assert!(encode_starter_skins(&wrong).is_err());
    }
}
