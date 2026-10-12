//! Several font carriers of one outline face, each at its own raster em and rendering mode.

use super::*;

pub const FONT_FACE_BUNDLE_MAGIC: [u8; 9] = *b"MCBEFACE1";
pub const FONT_FACE_BUNDLE_SCHEMA: u32 = 1;
const MAX_FACE_RASTERS: usize = 32;
const ENTRY_BYTES: usize = 13;

/// One encoded carrier and the raster em and rendering it was produced for.
pub struct FontFaceRaster {
    pub em_pixels: u32,
    pub rendering: FontRendering,
    pub carrier: Box<[u8]>,
}

/// Returns the authenticated bundle; every carrier must name the same source manifest.
pub fn encode_font_face_bundle(
    source_manifest_sha256: [u8; 32],
    rasters: &[FontFaceRaster],
) -> Result<Box<[u8]>, FontCatalogError> {
    if rasters.is_empty() || rasters.len() > MAX_FACE_RASTERS {
        return Err(invalid_catalog("face bundle raster count exceeds bounds"));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&FONT_FACE_BUNDLE_MAGIC);
    bytes.extend_from_slice(&FONT_FACE_BUNDLE_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&source_manifest_sha256);
    bytes.extend_from_slice(&(rasters.len() as u32).to_le_bytes());
    for raster in rasters {
        bytes.extend_from_slice(&raster.em_pixels.to_le_bytes());
        bytes.push(rendering_code(raster.rendering));
        bytes.extend_from_slice(&(raster.carrier.len() as u64).to_le_bytes());
    }
    for raster in rasters {
        bytes.extend_from_slice(&raster.carrier);
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    if bytes.len() > MAX_FONT_CARRIER_BYTES {
        return Err(invalid_catalog("face bundle exceeds its byte bound"));
    }
    Ok(bytes.into_boxed_slice())
}

/// Decodes every carrier in bundle order once their pages fit one catalog's page and decoded-byte
/// budget, rejecting any byte the digest does not cover.
pub fn decode_font_face_bundle(
    bytes: &[u8],
    source_manifest_sha256: [u8; 32],
) -> Result<Vec<(u32, FontRendering, CompiledFontCatalog)>, FontCatalogError> {
    let invalid = |detail: &str| FontCatalogError::InvalidCarrier {
        detail: detail.into(),
    };
    let header = FONT_FACE_BUNDLE_MAGIC.len() + 4 + 32 + 4;
    if bytes.len() < header + HASH_BYTES || bytes.len() > MAX_FONT_CARRIER_BYTES {
        return Err(invalid("face bundle length is outside its bound"));
    }
    let (body, digest) = bytes.split_at(bytes.len() - HASH_BYTES);
    if Sha256::digest(body).as_slice() != digest {
        return Err(FontCatalogError::CarrierHashMismatch);
    }
    if body[..9] != FONT_FACE_BUNDLE_MAGIC
        || u32::from_le_bytes(body[9..13].try_into().unwrap()) != FONT_FACE_BUNDLE_SCHEMA
    {
        return Err(invalid("unsupported MCBEFACE1 header"));
    }
    if body[13..45] != source_manifest_sha256 {
        return Err(FontCatalogError::SourceManifestMismatch);
    }
    let count = u32::from_le_bytes(body[45..49].try_into().unwrap()) as usize;
    if count == 0 || count > MAX_FACE_RASTERS || body.len() < header + count * ENTRY_BYTES {
        return Err(invalid("face bundle raster count exceeds bounds"));
    }
    let mut offset = header + count * ENTRY_BYTES;
    let mut entries = Vec::with_capacity(count);
    let (mut pages, mut decoded) = (0usize, 0usize);
    for entry in body[header..header + count * ENTRY_BYTES]
        .as_chunks::<ENTRY_BYTES>()
        .0
    {
        let em = u32::from_le_bytes(entry[..4].try_into().unwrap());
        let rendering = match entry[4] {
            0 => FontRendering::Coverage,
            1 => FontRendering::NativeCoverage,
            2 => FontRendering::NativeSdf,
            _ => return Err(invalid("unknown face bundle rendering")),
        };
        let length = usize::try_from(u64::from_le_bytes(entry[5..].try_into().unwrap()))
            .map_err(|_| invalid("face bundle raster length exceeds platform"))?;
        let end = offset
            .checked_add(length)
            .filter(|&end| end <= body.len())
            .ok_or_else(|| invalid("face bundle raster exceeds its payload"))?;
        if em == 0 || em > MAX_FONT_PAGE_SIDE {
            return Err(invalid("face bundle raster em exceeds bounds"));
        }
        let (entry_pages, entry_bytes) =
            decoded_extent(&body[offset..end], source_manifest_sha256)?;
        pages += entry_pages;
        decoded += entry_bytes;
        if pages > MAX_FONT_PAGES || decoded > MAX_FONT_DECODED_BYTES {
            return Err(invalid(
                "face bundle pages exceed the shared decoded budget",
            ));
        }
        entries.push((em, rendering, offset..end));
        offset = end;
    }
    if offset != body.len() {
        return Err(invalid("face bundle has trailing bytes"));
    }
    entries
        .into_iter()
        .map(|(em, rendering, range)| {
            CompiledFontCatalog::decode(&body[range], source_manifest_sha256)
                .map(|font| (em, rendering, font))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A carrier of `pages` one-texel pages, each within every per-carrier bound.
    fn carrier(manifest: [u8; 32], pages: usize) -> Box<[u8]> {
        let pages: Vec<_> = (0..pages)
            .map(|index| FontTexturePage {
                source_path: format!("font/page-{index:03}.png").into(),
                source_bytes: 1,
                source_sha256: [1; 32],
                pixels_sha256: Sha256::digest([255]).into(),
                width: 1,
                height: 1,
                pixels: FontPixels::Coverage([255u8].into()),
            })
            .collect();
        let glyph = GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        };
        encode_font_catalog(manifest, &[glyph], &pages).unwrap()
    }

    #[test]
    fn nested_carriers_share_one_page_and_decoded_budget() {
        let manifest = [3; 32];
        let raster = |pages| FontFaceRaster {
            em_pixels: 52,
            rendering: FontRendering::NativeSdf,
            carrier: carrier(manifest, pages),
        };
        let fits = encode_font_face_bundle(manifest, &[raster(128), raster(128)]).unwrap();
        assert_eq!(decode_font_face_bundle(&fits, manifest).unwrap().len(), 2);
        let over = encode_font_face_bundle(manifest, &[raster(200), raster(200)]).unwrap();
        assert!(decode_font_face_bundle(&over, manifest).is_err());
    }
}

const fn rendering_code(rendering: FontRendering) -> u8 {
    match rendering {
        FontRendering::Coverage => 0,
        FontRendering::NativeCoverage => 1,
        FontRendering::NativeSdf => 2,
    }
}
