//! Preserves high glyph indices that fontdue's format-4 reader treats as signed.

use std::collections::BTreeMap;

use super::*;

/// Parses source artwork, supplementing lost format-4 mappings without changing source files.
pub(super) fn parse(path: &Path, bytes: &[u8]) -> Result<Font, FontCompileError> {
    let parse = |source: &[u8]| {
        Font::from_bytes(source, FontSettings::default()).map_err(|detail| {
            FontCompileError::OutlineFont {
                path: path.into(),
                detail: detail.into(),
            }
        })
    };
    let font = parse(bytes)?;
    let Some((record, cmap)) = cmap_table(bytes)? else {
        return Ok(font);
    };
    let count = usize::from(be16(cmap, 2)?);
    cmap.get(..4 + count * 8)
        .ok_or_else(|| invalid("truncated cmap records"))?;
    let mut missing = BTreeMap::new();
    for index in 0..count {
        let entry = 4 + index * 8;
        let platform = be16(cmap, entry)?;
        let encoding = be16(cmap, entry + 2)?;
        if platform != 0 && !(platform == 3 && matches!(encoding, 1 | 10)) {
            continue;
        }
        let offset = be32(cmap, entry + 4)? as usize;
        let subtable = cmap
            .get(offset..)
            .ok_or_else(|| invalid("cmap offset exceeds source"))?;
        if be16(subtable, 0)? == 4 {
            high_indices(subtable, &font, &mut missing)?;
        }
    }
    if missing.is_empty() {
        return Ok(font);
    }
    // Append a small format-12 supplement only for mappings the reader lost.
    let new_count = u16::try_from(count + 1).map_err(|_| invalid("too many cmap subtables"))?;
    let mut table = Vec::with_capacity(cmap.len() + 24 + missing.len() * 12);
    table.extend_from_slice(&[0, 0]);
    table.extend_from_slice(&new_count.to_be_bytes());
    for index in 0..count {
        let entry = 4 + index * 8;
        table.extend_from_slice(&cmap[entry..entry + 4]);
        let offset = be32(cmap, entry + 4)?
            .checked_add(8)
            .ok_or_else(|| invalid("cmap offset overflows"))?;
        table.extend_from_slice(&offset.to_be_bytes());
    }
    // An internal custom encoding leaves declared Unicode encoding records intact.
    // fontdue reads every mapping subtable, including custom encodings.
    table.extend_from_slice(&[0, 4, 0, 255]);
    table.extend_from_slice(&((cmap.len() + 8) as u32).to_be_bytes());
    table.extend_from_slice(&cmap[4 + count * 8..]);
    table.extend_from_slice(&[0, 12, 0, 0]);
    table.extend_from_slice(&((16 + missing.len() * 12) as u32).to_be_bytes());
    table.extend_from_slice(&0_u32.to_be_bytes());
    table.extend_from_slice(&(missing.len() as u32).to_be_bytes());
    for (codepoint, glyph) in missing {
        table.extend_from_slice(&codepoint.to_be_bytes());
        table.extend_from_slice(&codepoint.to_be_bytes());
        table.extend_from_slice(&u32::from(glyph).to_be_bytes());
    }
    let mut normalized = bytes.to_vec();
    normalized.resize(normalized.len().next_multiple_of(4), 0);
    let offset = normalized.len() as u32;
    normalized[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
    normalized[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
    normalized.extend_from_slice(&table);
    parse(&normalized)
}

/// Finds the first face's cmap table in a TrueType/OpenType file or collection.
fn cmap_table(bytes: &[u8]) -> Result<Option<(usize, &[u8])>, FontCompileError> {
    let face = if bytes.starts_with(b"ttcf") {
        be32(bytes, 12)? as usize
    } else {
        0
    };
    let sfnt = bytes
        .get(face..)
        .ok_or_else(|| invalid("font face offset exceeds source"))?;
    for index in 0..usize::from(be16(sfnt, 4)?) {
        let record = 12 + index * 16;
        if read::<4>(sfnt, record)? != *b"cmap" {
            continue;
        }
        let offset = be32(sfnt, record + 8)? as usize;
        let len = be32(sfnt, record + 12)? as usize;
        let table = bytes
            .get(offset..)
            .and_then(|tail| tail.get(..len))
            .ok_or_else(|| invalid("cmap table exceeds source"))?;
        return Ok(Some((face + record, table)));
    }
    Ok(None)
}

/// Applies unsigned, modulo-65536 glyph arithmetic to bounded, ordered format-4 segments.
fn high_indices(
    data: &[u8],
    font: &Font,
    missing: &mut BTreeMap<u32, u16>,
) -> Result<(), FontCompileError> {
    let data = data
        .get(..usize::from(be16(data, 2)?))
        .ok_or_else(|| invalid("cmap subtable exceeds source"))?;
    let segments = usize::from(be16(data, 6)? / 2);
    let mut previous = None;
    for index in 0..segments {
        let end = be16(data, 14 + index * 2)?;
        let start = be16(data, 16 + segments * 2 + index * 2)?;
        if start > end || previous.is_some_and(|last| start <= last) {
            return Err(invalid("cmap segments overlap or are unordered"));
        }
        previous = Some(end);
        let delta = be16(data, 16 + segments * 4 + index * 2)?;
        let address = 16 + segments * 6 + index * 2;
        let range = usize::from(be16(data, address)?);
        if range == 0 || range == usize::from(u16::MAX) {
            continue;
        }
        for codepoint in start..=end {
            let Some(character) = char::from_u32(u32::from(codepoint)) else {
                continue;
            };
            if font.lookup_glyph_index(character) != 0 {
                continue;
            }
            let glyph = be16(data, address + range + usize::from(codepoint - start) * 2)?;
            if glyph == 0 {
                continue;
            }
            let glyph = glyph.wrapping_add(delta);
            if glyph > i16::MAX as u16 {
                missing.insert(u32::from(codepoint), glyph);
            }
        }
    }
    Ok(())
}

/// Reads one bounded big-endian table field.
fn read<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], FontCompileError> {
    bytes
        .get(offset..)
        .and_then(|tail| tail.get(..N))
        .and_then(|field| field.try_into().ok())
        .ok_or_else(|| invalid("truncated font mapping table"))
}

/// Reads an unsigned short from a font table.
fn be16(bytes: &[u8], offset: usize) -> Result<u16, FontCompileError> {
    Ok(u16::from_be_bytes(read(bytes, offset)?))
}

/// Reads an unsigned long from a font table.
fn be32(bytes: &[u8], offset: usize) -> Result<u32, FontCompileError> {
    Ok(u32::from_be_bytes(read(bytes, offset)?))
}
