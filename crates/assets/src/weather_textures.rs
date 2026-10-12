//! Optional carrier for the precipitation sheet and End sky textures.
//!
//! Layout: magic, version, then the weather and End-sky images as `width, height, rgba8`,
//! then a SHA-256 of everything before it.

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const WEATHER_TEXTURES_MAGIC: [u8; 8] = *b"MCBEWTH1";
pub const WEATHER_TEXTURES_VERSION: u32 = 1;
pub const WEATHER_SHEET_SIDE: u32 = 32;
pub const END_SKY_SIDE: u32 = 128;
pub const END_SKY_TEXTURE_PATH: &str = "textures/environment/end_sky";
pub const MAX_WEATHER_TEXTURES_BYTES: usize = 128 * 1024;

const HASH_BYTES: usize = 32;
const HEADER_BYTES: usize = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WeatherTexturesError {
    #[error("invalid weather texture carrier: {0}")]
    Invalid(&'static str),
}

/// One decoded RGBA8 image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeatherImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// The precipitation sheet and End sky images.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeatherTextures {
    pub weather: WeatherImage,
    pub end_sky: WeatherImage,
}

impl WeatherTextures {
    fn validate(&self) -> Result<(), WeatherTexturesError> {
        for (image, side) in [
            (&self.weather, WEATHER_SHEET_SIDE),
            (&self.end_sky, END_SKY_SIDE),
        ] {
            if image.width != side
                || image.height != side
                || image.rgba8.len() != (side * side * 4) as usize
            {
                return Err(WeatherTexturesError::Invalid("unexpected image dimensions"));
            }
        }
        Ok(())
    }
}

pub fn encode_weather_textures(
    textures: &WeatherTextures,
) -> Result<Vec<u8>, WeatherTexturesError> {
    textures.validate()?;
    let mut bytes = Vec::with_capacity(MAX_WEATHER_TEXTURES_BYTES / 2);
    bytes.extend_from_slice(&WEATHER_TEXTURES_MAGIC);
    bytes.extend_from_slice(&WEATHER_TEXTURES_VERSION.to_le_bytes());
    for image in [&textures.weather, &textures.end_sky] {
        bytes.extend_from_slice(&image.width.to_le_bytes());
        bytes.extend_from_slice(&image.height.to_le_bytes());
        bytes.extend_from_slice(&image.rgba8);
    }
    let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    Ok(bytes)
}

/// Decodes and hash-checks a carrier; the second value is its SHA-256 identity.
pub fn decode_weather_textures(
    bytes: &[u8],
) -> Result<(WeatherTextures, [u8; HASH_BYTES]), WeatherTexturesError> {
    let invalid = WeatherTexturesError::Invalid;
    if bytes.len() > MAX_WEATHER_TEXTURES_BYTES || bytes.len() < HEADER_BYTES + HASH_BYTES {
        return Err(invalid("bad length"));
    }
    let (body, hash) = bytes.split_at(bytes.len() - HASH_BYTES);
    let expected: [u8; HASH_BYTES] = Sha256::digest(body).into();
    if hash != expected {
        return Err(invalid("hash mismatch"));
    }
    if body[..8] != WEATHER_TEXTURES_MAGIC || body[8..12] != WEATHER_TEXTURES_VERSION.to_le_bytes()
    {
        return Err(invalid("unsupported header"));
    }
    let mut cursor = HEADER_BYTES;
    let mut read_image = |side: u32| -> Result<WeatherImage, WeatherTexturesError> {
        let width = read_u32(body, cursor).ok_or(invalid("truncated"))?;
        let height = read_u32(body, cursor + 4).ok_or(invalid("truncated"))?;
        if width != side || height != side {
            return Err(invalid("unexpected image dimensions"));
        }
        let start = cursor + 8;
        let end = start + (side * side * 4) as usize;
        let rgba8 = body.get(start..end).ok_or(invalid("truncated"))?;
        cursor = end;
        Ok(WeatherImage {
            width,
            height,
            rgba8: rgba8.into(),
        })
    };
    let weather = read_image(WEATHER_SHEET_SIDE)?;
    let end_sky = read_image(END_SKY_SIDE)?;
    if cursor != body.len() {
        return Err(invalid("trailing bytes"));
    }
    Ok((WeatherTextures { weather, end_sky }, expected))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WeatherTextures {
        let image = |side: u32, seed: u8| WeatherImage {
            width: side,
            height: side,
            rgba8: vec![seed; (side * side * 4) as usize].into(),
        };
        WeatherTextures {
            weather: image(WEATHER_SHEET_SIDE, 7),
            end_sky: image(END_SKY_SIDE, 9),
        }
    }

    #[test]
    fn round_trips_and_reports_a_stable_identity() {
        let bytes = encode_weather_textures(&sample()).unwrap();
        let (decoded, identity) = decode_weather_textures(&bytes).unwrap();
        assert_eq!(decoded, sample());
        assert_eq!(identity, decode_weather_textures(&bytes).unwrap().1);
    }

    #[test]
    fn rejects_corruption_truncation_and_wrong_sizes() {
        let mut bytes = encode_weather_textures(&sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(decode_weather_textures(&bytes).is_err());
        assert!(decode_weather_textures(&bytes[..20]).is_err());
        let mut wrong = sample();
        wrong.weather.width = 16;
        assert!(encode_weather_textures(&wrong).is_err());
    }
}
