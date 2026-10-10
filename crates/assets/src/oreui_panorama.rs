//! Optional, hash-checked panorama crops prepared from the downloaded sample pack.

use std::io::Write;

use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use sha2::{Digest, Sha256};

pub const BANNER_COUNT: usize = 8;
pub const IMAGE_COUNT: usize = BANNER_COUNT + 1;
pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 540;
pub const IMAGE_BYTES: usize = (WIDTH * HEIGHT * 4) as usize;
const PIXEL_BYTES: usize = IMAGE_COUNT * IMAGE_BYTES;
const MAGIC: &[u8; 8] = b"MCBEOPA1";
const VERSION: u32 = 1;
const HEADER_BYTES: usize = MAGIC.len() + 4;
const HASH_BYTES: usize = 32;
pub const MAX_CARRIER_BYTES: usize = PIXEL_BYTES + 128 * 1024;

/// Eight profile banners followed by the world preview, each in straight RGBA8.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OreUiPanoramas {
    pub images: [Box<[u8]>; IMAGE_COUNT],
}

/// Compresses exactly sized crops and appends a digest of the encoded header and payload.
pub fn encode(panoramas: &OreUiPanoramas) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    for image in &panoramas.images {
        if image.len() != IMAGE_BYTES {
            return Err("invalid OreUI panorama dimensions".into());
        }
        encoder
            .write_all(image)
            .map_err(|error| error.to_string())?;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend(encoder.finish().map_err(|error| error.to_string())?);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    if bytes.len() > MAX_CARRIER_BYTES {
        return Err("OreUI panorama carrier exceeds its byte limit".into());
    }
    Ok(bytes)
}

/// Checks framing, digest and bounded decompression before exposing any crop pixels.
pub fn decode(bytes: &[u8]) -> Result<OreUiPanoramas, String> {
    if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_CARRIER_BYTES {
        return Err("invalid OreUI panorama carrier length".into());
    }
    let (body, digest) = bytes.split_at(bytes.len() - HASH_BYTES);
    if digest != Sha256::digest(body).as_slice() {
        return Err("OreUI panorama hash mismatch".into());
    }
    if &body[..MAGIC.len()] != MAGIC || body[MAGIC.len()..HEADER_BYTES] != VERSION.to_le_bytes() {
        return Err("unsupported OreUI panorama header".into());
    }
    let payload = &body[HEADER_BYTES..];
    let mut decoder = Decompress::new(true);
    let mut pixels = vec![0; PIXEL_BYTES + 1];
    let status = decoder
        .decompress(payload, &mut pixels, FlushDecompress::Finish)
        .map_err(|error| error.to_string())?;
    if status != Status::StreamEnd
        || decoder.total_out() as usize != PIXEL_BYTES
        || decoder.total_in() as usize != payload.len()
    {
        return Err("invalid OreUI panorama pixel payload".into());
    }
    Ok(OreUiPanoramas {
        images: std::array::from_fn(|index| {
            pixels[index * IMAGE_BYTES..(index + 1) * IMAGE_BYTES].into()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct solid crops make image ordering observable without a game fixture.
    fn fixture() -> OreUiPanoramas {
        OreUiPanoramas {
            images: std::array::from_fn(|index| vec![index as u8; IMAGE_BYTES].into()),
        }
    }

    /// Updates the digest so malformed framing reaches the decoder after integrity checks.
    fn rehash(bytes: &mut Vec<u8>) {
        bytes.truncate(bytes.len() - HASH_BYTES);
        let digest = Sha256::digest(&*bytes);
        bytes.extend_from_slice(&digest);
    }

    #[test]
    fn panorama_carrier_round_trips_every_crop() {
        let panoramas = fixture();
        assert_eq!(decode(&encode(&panoramas).unwrap()).unwrap(), panoramas);
    }

    #[test]
    fn panorama_carrier_rejects_corruption_and_malformed_framing() {
        let bytes = encode(&fixture()).unwrap();
        for length in [0, 8, HEADER_BYTES, bytes.len() - 1] {
            assert!(decode(&bytes[..length]).is_err());
        }
        let mut corrupt = bytes.clone();
        corrupt[HEADER_BYTES] ^= 1;
        assert!(decode(&corrupt).is_err());
        let mut version = bytes.clone();
        version[8] += 1;
        rehash(&mut version);
        assert!(decode(&version).is_err());
        let mut trailing = bytes.clone();
        trailing.insert(trailing.len() - HASH_BYTES, 0);
        rehash(&mut trailing);
        assert!(decode(&trailing).is_err());
        let mut incomplete = bytes.clone();
        incomplete.drain(incomplete.len() - HASH_BYTES - 4..incomplete.len() - HASH_BYTES);
        rehash(&mut incomplete);
        assert!(decode(&incomplete).is_err(), "incomplete zlib checksum");
        let mut short = fixture();
        short.images[0] = vec![0; IMAGE_BYTES - 4].into();
        assert!(encode(&short).is_err());
    }

    #[test]
    fn panorama_carrier_rejects_wrong_decompressed_size() {
        for length in [PIXEL_BYTES - 1, PIXEL_BYTES + 1] {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(&vec![0; length]).unwrap();
            let mut bytes = [MAGIC.as_slice(), &VERSION.to_le_bytes()].concat();
            bytes.extend(encoder.finish().unwrap());
            bytes.extend([0; HASH_BYTES]);
            rehash(&mut bytes);
            assert!(decode(&bytes).is_err());
        }
    }
}
