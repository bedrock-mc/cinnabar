use super::*;

#[test]
fn missing_replacement_at_one_pixel_uses_the_faces_own_missing_glyph() {
    let mut source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/CinnanglesSans.ttf"
    ))
    .to_vec();
    let glyph_id = ttf_parser::Face::parse(&source, 0)
        .unwrap()
        .glyph_index('A')
        .unwrap()
        .0;
    let read16 = |bytes: &[u8], at| u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap());
    let read32 = |bytes: &[u8], at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let record = (0..usize::from(read16(&source, 4)))
        .map(|index| 12 + index * 16)
        .find(|&at| &source[at..at + 4] == b"cmap")
        .unwrap();
    let start = read32(&source, record + 8) as usize;
    let mut cmap = vec![0; 44];
    cmap[2..4].copy_from_slice(&1u16.to_be_bytes());
    cmap[4..6].copy_from_slice(&3u16.to_be_bytes());
    cmap[6..8].copy_from_slice(&1u16.to_be_bytes());
    cmap[8..12].copy_from_slice(&12u32.to_be_bytes());
    let words = [
        4,
        32,
        0,
        4,
        4,
        1,
        0,
        u16::from(b'A'),
        u16::MAX,
        0,
        u16::from(b'A'),
        u16::MAX,
        glyph_id.wrapping_sub(u16::from(b'A')),
        1,
        0,
        0,
    ];
    for (index, word) in words.into_iter().enumerate() {
        cmap[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let checksum = cmap.chunks_exact(4).fold(0u32, |sum, bytes| {
        sum.wrapping_add(u32::from_be_bytes(bytes.try_into().unwrap()))
    });
    source[record + 4..record + 8].copy_from_slice(&checksum.to_be_bytes());
    source[record + 12..record + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
    source[start..start + cmap.len()].copy_from_slice(&cmap);
    let mut face = freetype::Face::new(&source, 1).unwrap();
    assert!(!face.has(REQUIRED_REPLACEMENT));
    let missing = face.rasterize(REQUIRED_REPLACEMENT).unwrap();
    let catalog = compile_native_outline_font(
        Path::new("font/missing-face.ttf"),
        &source,
        Sha256::digest(&source).into(),
        OutlineFontConfig {
            pixel_height: 1,
            atlas_side: 256,
            ..OutlineFontConfig::default()
        },
        FontRendering::NativeCoverage,
    )
    .unwrap();
    let replacement = catalog.glyph(REQUIRED_REPLACEMENT).unwrap();
    assert_eq!(replacement.advance_64, missing.advance_64);
    assert_eq!(replacement.bearing, missing.bearing);
    assert_eq!(catalog.line_metrics().unwrap().em_64, 64);
    assert_eq!(catalog.rendering(), FontRendering::NativeCoverage);
    assert!(catalog.glyph('A').unwrap().bearing[1] < 0);
}
