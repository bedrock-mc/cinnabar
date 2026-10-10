use super::*;

/// Builds a one-glyph coverage page with enough empty space to exercise compression.
fn fixture() -> Box<[u8]> {
    let pixels = vec![73; 128 * 128].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/coverage.png".into(),
        source_bytes: 1,
        source_sha256: [9; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 128,
        height: 128,
        pixels: FontPixels::Coverage(pixels),
    };
    let glyph = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [1, 1, 17, 17],
        bearing: [0, -16],
        advance_64: 1024,
    };
    encode_font_catalog([1; 32], &[glyph], &[page]).unwrap()
}

/// Replaces the single page's stream while preserving canonical authenticated framing.
fn replace_stream(bytes: &[u8], stored: &[u8]) -> Vec<u8> {
    let envelope = validate_envelope(bytes, [1; 32]).unwrap();
    let mut result = bytes[..envelope.pixels_offset].to_vec();
    result[envelope.page_offset + 32..envelope.page_offset + 40]
        .copy_from_slice(&(stored.len() as u64).to_le_bytes());
    result.extend_from_slice(stored);
    let end = result.len();
    result[85..93].copy_from_slice(&(end as u64).to_le_bytes());
    result.extend_from_slice(&Sha256::digest(&result));
    result
}

#[test]
fn compressed_coverage_round_trips_metrics_hashes_and_exact_texels() {
    let bytes = fixture();
    assert!(bytes.len() < 1024);
    let decoded = CompiledFontCatalog::decode(&bytes, [1; 32]).unwrap();
    assert_eq!(decoded.identity().schema, FONT_CARRIER_SCHEMA);
    assert_eq!(decoded.glyph('A').unwrap().uv, [1, 1, 17, 17]);
    let page = &decoded.pages()[0];
    assert!(matches!(page.pixels, FontPixels::Coverage(_)));
    assert_eq!(page.pixels.bytes(), vec![73; 128 * 128]);
    assert_eq!(
        page.pixels_sha256.as_slice(),
        Sha256::digest(page.pixels.bytes()).as_slice()
    );
    assert_eq!(
        encode_font_catalog([1; 32], decoded.glyphs(), decoded.pages()).unwrap(),
        bytes
    );
}

#[test]
fn truncated_oversized_trailing_and_corrupted_streams_are_rejected_after_valid_envelope() {
    let bytes = fixture();
    let env = validate_envelope(&bytes, [1; 32]).unwrap();
    let stream = &bytes[env.pixels_offset..env.hash_offset];
    let mut corrupt = stream.to_vec();
    corrupt[stream.len() / 2] ^= 0xff;
    let mut trailing = stream.to_vec();
    trailing.push(0);
    for stored in [
        stream[..stream.len() - 1].to_vec(),
        compress(&vec![73; 128 * 128 + 1]).unwrap(),
        compress(&vec![73; 128 * 128 - 1]).unwrap(),
        compress(&vec![74; 128 * 128]).unwrap(),
        corrupt,
        trailing,
    ] {
        let bad = replace_stream(&bytes, &stored);
        assert!(CompiledFontCatalog::decode(&bad, [1; 32]).is_err());
    }
    for end in [0, HEADER_BYTES, bytes.len() - 1] {
        assert!(CompiledFontCatalog::decode(&bytes[..end], [1; 32]).is_err());
    }
}

#[test]
fn oversized_decoded_page_is_rejected_before_decompression() {
    let mut bytes = fixture().into_vec();
    let env = validate_envelope(&bytes, [1; 32]).unwrap();
    bytes[env.page_offset + 12..env.page_offset + 16]
        .copy_from_slice(&(MAX_FONT_PAGE_SIDE + 1).to_le_bytes());
    let digest = Sha256::digest(&bytes[..env.hash_offset]);
    bytes[env.hash_offset..].copy_from_slice(&digest);
    assert!(CompiledFontCatalog::decode(&bytes, [1; 32]).is_err());
}
