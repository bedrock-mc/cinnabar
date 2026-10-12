//! Optional carrier for the shipped original OreUI artwork, keyed by the bundle paths screens draw.
//!
//! Layout: magic, version, source-manifest SHA-256, image count, then per image `key length u16,
//! key, width u16, height u16, frame count u16, millis u32 per frame`, then one zlib stream of
//! every frame's RGBA8 in table order, then a SHA-256 of everything before it.

use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Write};
use thiserror::Error;

pub const OREUI_ICONS_MAGIC: [u8; 8] = *b"MCBEOUI1";
pub const OREUI_ICONS_VERSION: u32 = 1;
/// The reviewed source manifest a carrier must have been compiled from.
pub const OREUI_ICONS_SOURCE_MANIFEST: &[u8] =
    include_bytes!("../../../assets/oreui-icons-source.json");
pub const MAX_OREUI_ICONS_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_OREUI_ICON_IMAGES: usize = 512;
pub const MAX_OREUI_ICON_FRAMES: usize = 64;
pub const MAX_OREUI_ICON_SIDE: u32 = 1024;
pub const MAX_OREUI_ICON_KEY_BYTES: usize = 256;
/// Decoded RGBA8 bytes across every frame.
pub const MAX_OREUI_ICON_PIXEL_BYTES: usize = 16 * 1024 * 1024;

const HASH_BYTES: usize = 32;
const HEADER_BYTES: usize = 8 + 4 + HASH_BYTES + 4;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OreUiIconsError {
    #[error("invalid OreUI icon carrier: {0}")]
    Invalid(&'static str),
    #[error("OreUI icon carrier hash mismatch")]
    HashMismatch,
    #[error("OreUI icon carrier was compiled from a different source manifest")]
    SourceManifestMismatch,
}

/// One frame's straight RGBA8 pixels; `millis` is zero for still images.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OreUiIconFrame {
    pub millis: u32,
    pub rgba8: Box<[u8]>,
}

/// A still image or animation whose frames share its dimensions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OreUiIcon {
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<OreUiIconFrame>,
}

fn validate(icons: &[OreUiIcon]) -> Result<usize, OreUiIconsError> {
    let invalid = OreUiIconsError::Invalid;
    if icons.is_empty() || icons.len() > MAX_OREUI_ICON_IMAGES {
        return Err(invalid("image count exceeds bounds"));
    }
    let mut keys = HashSet::new();
    let mut pixels = 0usize;
    for icon in icons {
        if icon.key.is_empty() || icon.key.len() > MAX_OREUI_ICON_KEY_BYTES {
            return Err(invalid("key length exceeds bounds"));
        }
        if !keys.insert(icon.key.as_str()) {
            return Err(invalid("duplicate key"));
        }
        if !(1..=MAX_OREUI_ICON_SIDE).contains(&icon.width)
            || !(1..=MAX_OREUI_ICON_SIDE).contains(&icon.height)
        {
            return Err(invalid("image side exceeds bounds"));
        }
        if icon.frames.is_empty() || icon.frames.len() > MAX_OREUI_ICON_FRAMES {
            return Err(invalid("frame count exceeds bounds"));
        }
        let animated = icon.frames.len() > 1;
        let frame_bytes = (icon.width * icon.height * 4) as usize;
        for frame in &icon.frames {
            if animated != (frame.millis > 0) {
                return Err(invalid("animation frames need durations; stills have none"));
            }
            if frame.rgba8.len() != frame_bytes {
                return Err(invalid("frame pixels do not match the image dimensions"));
            }
            pixels += frame_bytes;
        }
        if pixels > MAX_OREUI_ICON_PIXEL_BYTES {
            return Err(invalid("decoded pixels exceed bounds"));
        }
    }
    Ok(pixels)
}

/// Returns the authenticated carrier for `icons`, compiled from the given source manifest.
pub fn encode_oreui_icons(
    source_manifest_sha256: [u8; 32],
    icons: &[OreUiIcon],
) -> Result<Vec<u8>, OreUiIconsError> {
    validate(icons)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&OREUI_ICONS_MAGIC);
    bytes.extend_from_slice(&OREUI_ICONS_VERSION.to_le_bytes());
    bytes.extend_from_slice(&source_manifest_sha256);
    bytes.extend_from_slice(&(icons.len() as u32).to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    for icon in icons {
        bytes.extend_from_slice(&(icon.key.len() as u16).to_le_bytes());
        bytes.extend_from_slice(icon.key.as_bytes());
        bytes.extend_from_slice(&(icon.width as u16).to_le_bytes());
        bytes.extend_from_slice(&(icon.height as u16).to_le_bytes());
        bytes.extend_from_slice(&(icon.frames.len() as u16).to_le_bytes());
        for frame in &icon.frames {
            bytes.extend_from_slice(&frame.millis.to_le_bytes());
            encoder
                .write_all(&frame.rgba8)
                .map_err(|_| OreUiIconsError::Invalid("compression failed"))?;
        }
    }
    let compressed = encoder
        .finish()
        .map_err(|_| OreUiIconsError::Invalid("compression failed"))?;
    bytes.extend_from_slice(&compressed);
    let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    if bytes.len() > MAX_OREUI_ICONS_BYTES {
        return Err(OreUiIconsError::Invalid("carrier exceeds its byte bound"));
    }
    Ok(bytes)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], OreUiIconsError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(OreUiIconsError::Invalid("truncated"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u16(&mut self) -> Result<u16, OreUiIconsError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, OreUiIconsError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Decodes a carrier compiled from `source_manifest_sha256`, rejecting any byte the digest does not
/// cover and any table or stream outside its bounds.
pub fn decode_oreui_icons(
    bytes: &[u8],
    source_manifest_sha256: [u8; 32],
) -> Result<Vec<OreUiIcon>, OreUiIconsError> {
    let invalid = OreUiIconsError::Invalid;
    if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_OREUI_ICONS_BYTES {
        return Err(invalid("length is outside its bound"));
    }
    let (body, hash) = bytes.split_at(bytes.len() - HASH_BYTES);
    if Sha256::digest(body).as_slice() != hash {
        return Err(OreUiIconsError::HashMismatch);
    }
    let mut reader = Reader { bytes: body, at: 0 };
    if reader.take(8)? != OREUI_ICONS_MAGIC || reader.u32()? != OREUI_ICONS_VERSION {
        return Err(invalid("unsupported header"));
    }
    if reader.take(HASH_BYTES)? != source_manifest_sha256 {
        return Err(OreUiIconsError::SourceManifestMismatch);
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > MAX_OREUI_ICON_IMAGES {
        return Err(invalid("image count exceeds bounds"));
    }
    let mut table = Vec::with_capacity(count);
    let mut pixels = 0usize;
    for _ in 0..count {
        let key_len = reader.u16()? as usize;
        let key = std::str::from_utf8(reader.take(key_len)?)
            .map_err(|_| invalid("key is not UTF-8"))?
            .to_owned();
        let width = u32::from(reader.u16()?);
        let height = u32::from(reader.u16()?);
        let frames = reader.u16()? as usize;
        if !(1..=MAX_OREUI_ICON_SIDE).contains(&width)
            || !(1..=MAX_OREUI_ICON_SIDE).contains(&height)
        {
            return Err(invalid("image side exceeds bounds"));
        }
        if frames == 0 || frames > MAX_OREUI_ICON_FRAMES {
            return Err(invalid("frame count exceeds bounds"));
        }
        let millis = (0..frames)
            .map(|_| reader.u32())
            .collect::<Result<Vec<_>, _>>()?;
        pixels = pixels
            .checked_add(width as usize * height as usize * 4 * frames)
            .filter(|total| *total <= MAX_OREUI_ICON_PIXEL_BYTES)
            .ok_or(invalid("decoded pixels exceed bounds"))?;
        table.push((key, width, height, millis));
    }
    let stored = &body[reader.at..];
    let mut payload = vec![0; pixels + 1];
    let mut decoder = Decompress::new(true);
    let status = decoder
        .decompress(stored, &mut payload, FlushDecompress::Finish)
        .map_err(|_| invalid("invalid compressed pixels"))?;
    if status != Status::StreamEnd
        || decoder.total_out() != pixels as u64
        || decoder.total_in() != stored.len() as u64
    {
        return Err(invalid("compressed pixel length mismatch"));
    }
    let mut at = 0;
    let icons: Vec<_> = table
        .into_iter()
        .map(|(key, width, height, millis)| {
            let frame_bytes = (width * height * 4) as usize;
            let frames = millis
                .into_iter()
                .map(|millis| {
                    let rgba8 = payload[at..at + frame_bytes].into();
                    at += frame_bytes;
                    OreUiIconFrame { millis, rgba8 }
                })
                .collect();
            OreUiIcon {
                key,
                width,
                height,
                frames,
            }
        })
        .collect();
    validate(&icons)?;
    Ok(icons)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<OreUiIcon> {
        let frame = |millis, seed: u8| OreUiIconFrame {
            millis,
            rgba8: vec![seed; 2 * 3 * 4].into(),
        };
        vec![
            OreUiIcon {
                key: "assets/still.png".into(),
                width: 2,
                height: 3,
                frames: vec![frame(0, 7)],
            },
            OreUiIcon {
                key: "assets/spin.gif".into(),
                width: 3,
                height: 2,
                frames: vec![frame(100, 1), frame(120, 2)],
            },
        ]
    }

    #[test]
    fn round_trips_against_its_source_manifest() {
        let bytes = encode_oreui_icons([5; 32], &sample()).unwrap();
        assert_eq!(decode_oreui_icons(&bytes, [5; 32]).unwrap(), sample());
        assert_eq!(
            decode_oreui_icons(&bytes, [6; 32]),
            Err(OreUiIconsError::SourceManifestMismatch)
        );
    }

    #[test]
    fn rejects_corruption_truncation_and_malformed_tables() {
        let mut bytes = encode_oreui_icons([5; 32], &sample()).unwrap();
        assert!(decode_oreui_icons(&bytes[..40], [5; 32]).is_err());
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert_eq!(
            decode_oreui_icons(&bytes, [5; 32]),
            Err(OreUiIconsError::HashMismatch)
        );
        let mut still_with_duration = sample();
        still_with_duration[0].frames[0].millis = 10;
        assert!(encode_oreui_icons([5; 32], &still_with_duration).is_err());
        let mut duplicate = sample();
        duplicate[1].key = duplicate[0].key.clone();
        assert!(encode_oreui_icons([5; 32], &duplicate).is_err());
        let mut short = sample();
        short[0].frames[0].rgba8 = vec![0; 4].into();
        assert!(encode_oreui_icons([5; 32], &short).is_err());
        assert!(encode_oreui_icons([5; 32], &[]).is_err());
    }

    #[test]
    fn a_resealed_oversized_table_is_rejected_before_inflating() {
        let mut bytes = encode_oreui_icons([5; 32], &sample()).unwrap();
        bytes.truncate(bytes.len() - HASH_BYTES);
        let width_offset = HEADER_BYTES + 2 + "assets/still.png".len();
        bytes[width_offset..width_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes[width_offset + 2..width_offset + 4].copy_from_slice(&u16::MAX.to_le_bytes());
        let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
        bytes.extend_from_slice(&hash);
        assert_eq!(
            decode_oreui_icons(&bytes, [5; 32]),
            Err(OreUiIconsError::Invalid("image side exceeds bounds"))
        );
    }
}
