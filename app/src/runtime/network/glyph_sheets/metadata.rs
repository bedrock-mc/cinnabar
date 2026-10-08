//! Pack font declarations and ordered, locale-filtered aliases.
use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

/// Applies the winning version-one metadata without allowing unbounded font inventories.
pub(super) fn apply(
    view: &LayeredPackView,
    default: &[CellGlyph],
    fonts: &mut BTreeMap<String, Vec<CellGlyph>>,
) {
    let Some(root) = view
        .read("font/font_metadata.json")
        .and_then(|bytes| super::super::resource_packs::parse_pack_json(&bytes))
    else {
        return;
    };
    if root["version"].as_u64() != Some(1) {
        return;
    }
    let mut budget = assets::MAX_FONT_SOURCE_BYTES as usize;
    for entry in root["fonts"].as_array().into_iter().flatten().take(32) {
        let Some(name) = entry["font_name"].as_str().filter(|name| name.len() <= 256) else {
            continue;
        };
        let cells = match entry["font_format"].as_str() {
            Some("bitmap") => bitmap(
                view,
                entry["ascii_font_file"].as_str(),
                entry["unicode_file_prefix"].as_str(),
            ),
            Some("ttf") | Some("ttfmsdf") => outline(view, entry).unwrap_or_default(),
            _ => continue,
        };
        if admit(&cells, &mut budget) {
            fonts.insert(name.into(), cells);
        }
    }
    let sources = fonts.clone();
    for alias in root["font_aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .take(32)
    {
        let Some(name) = alias["alias"].as_str().filter(|name| name.len() <= 256) else {
            continue;
        };
        let mut cells = BTreeMap::new();
        for selector in alias["fonts"].as_array().into_iter().flatten() {
            if selector["font_language_code"].as_str().is_some_and(|code| {
                !code.is_empty() && code != super::super::resource_packs::active_language_code()
            }) {
                continue;
            }
            let Some(reference) = selector["font_reference"].as_str() else {
                continue;
            };
            let source = sources
                .get(reference)
                .map(Vec::as_slice)
                .or_else(|| (reference == "default").then_some(default));
            for cell in source.into_iter().flatten() {
                let ranges = selector["font_ranges"].as_array();
                if ranges.is_none_or(|ranges| {
                    ranges.is_empty()
                        || ranges.iter().any(|range| {
                            let point = u64::from(u32::from(cell.codepoint));
                            range["first"]
                                .as_u64()
                                .zip(range["last"].as_u64())
                                .is_some_and(|(first, last)| first <= point && point <= last)
                        })
                }) {
                    cells.entry(cell.codepoint).or_insert_with(|| cell.clone());
                }
            }
        }
        let cells: Vec<_> = cells.into_values().collect();
        if admit(&cells, &mut budget) {
            fonts.insert(name.into(), cells);
        }
    }
}

/// Bounds the combined pixel storage of declarations and aliases.
fn admit(cells: &[CellGlyph], remaining: &mut usize) -> bool {
    let bytes = cells.iter().map(|cell| cell.rgba8.len()).sum::<usize>();
    if cells.is_empty() || bytes > *remaining {
        return false;
    }
    *remaining -= bytes;
    true
}

/// Rasterizes shipped outline fonts with bounded dimensions and the declared render size.
fn outline(view: &LayeredPackView, entry: &Value) -> Option<Vec<CellGlyph>> {
    let path = entry["font_file"].as_str()?;
    let bytes = [
        path.to_owned(),
        format!("{path}.ttf"),
        format!("{path}.otf"),
    ]
    .into_iter()
    .find_map(|path| view.read_capped(&path, assets::MAX_FONT_SOURCE_BYTES))?;
    let font = fontdue::Font::from_bytes(bytes.as_ref(), fontdue::FontSettings::default()).ok()?;
    let size = entry["target_font_render_size"].as_f64().unwrap_or(64.0) as f32;
    if !size.is_finite() || !(8.0..=128.0).contains(&size) {
        return None;
    }
    let scale = assets::texel_size_64(0, 1) as f32 / size;
    let mut chars: Vec<_> = font.chars().keys().copied().collect();
    chars.sort_unstable();
    let mut bytes = 0usize;
    let mut cells = Vec::new();
    for codepoint in chars.into_iter().take(assets::MAX_FONT_GLYPHS) {
        let metrics = font.metrics(codepoint, size);
        let charge = metrics.width.checked_mul(metrics.height)?.checked_mul(4)?;
        bytes = bytes.checked_add(charge)?;
        if bytes > assets::MAX_FONT_SOURCE_BYTES as usize {
            return None;
        }
        let (metrics, pixels) = font.rasterize(codepoint, size);
        cells.push(CellGlyph {
            codepoint,
            size: [metrics.width as u32, metrics.height as u32],
            rgba8: pixels
                .into_iter()
                .flat_map(|alpha| [255, 255, 255, alpha])
                .collect(),
            bearing: [
                (metrics.xmin as f32 * scale / 64.0).round() as i16,
                (-(metrics.ymin as f32 + metrics.height as f32) * scale / 64.0).round() as i16,
            ],
            advance_64: (metrics.advance_width * scale)
                .round()
                .clamp(0.0, i16::MAX as f32) as i16,
            draw_size_64: [
                (metrics.width as f32 * scale).round() as u32,
                (metrics.height as f32 * scale).round() as u32,
            ],
        });
    }
    Some(cells)
}
