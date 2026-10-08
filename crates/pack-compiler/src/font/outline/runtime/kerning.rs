use std::collections::{BTreeMap, BTreeSet};

use assets::{GlyphMetrics, MAX_FONT_KERNING_PAIRS};
use ttf_parser::{
    Face, GlyphId, Tag,
    gpos::{PairAdjustment, PositioningSubtable},
};

use super::super::*;

pub(super) fn pairs(
    bytes: &[u8],
    glyphs: &[GlyphMetrics],
    em: u32,
) -> Result<BTreeMap<(char, char), i32>, FontCompileError> {
    let face =
        Face::parse(bytes, 0).map_err(|_| invalid("outline positioning tables are invalid"))?;
    let Some(table) = face.tables().gpos else {
        return Ok(BTreeMap::new());
    };
    let mut lookups = BTreeSet::new();
    for feature in table.features {
        if feature.tag == Tag::from_bytes(b"kern") {
            lookups.extend(feature.lookup_indices);
        }
    }
    let glyphs: Vec<_> = glyphs
        .iter()
        .filter_map(|glyph| {
            face.glyph_index(glyph.codepoint)
                .map(|id| (glyph.codepoint, id))
        })
        .collect();
    if glyphs.len() > 2048 {
        return Err(invalid("runtime pair enumeration exceeds bounds"));
    }
    let mut pairs = BTreeMap::new();
    for &(left, first) in &glyphs {
        for &(right, second) in &glyphs {
            let mut adjustment = 0i32;
            for &index in &lookups {
                let Some(lookup) = table.lookups.get(index) else {
                    continue;
                };
                for subtable in lookup.subtables.into_iter::<PositioningSubtable<'_>>() {
                    if let PositioningSubtable::Pair(pair) = subtable
                        && let Some(value) = advance(pair, first, second)?
                    {
                        adjustment += i32::from(value);
                        break;
                    }
                }
            }
            if adjustment != 0 {
                let fixed = (adjustment as f64 * f64::from(em) * 64.0
                    / f64::from(face.units_per_em()))
                .round() as i32;
                pairs.insert((left, right), fixed);
                if pairs.len() > MAX_FONT_KERNING_PAIRS {
                    return Err(invalid("runtime kerning exceeds bounds"));
                }
            }
        }
    }
    Ok(pairs)
}

fn advance(
    pair: PairAdjustment<'_>,
    left: GlyphId,
    right: GlyphId,
) -> Result<Option<i16>, FontCompileError> {
    let index = match pair.coverage().get(left) {
        Some(index) => index,
        None => return Ok(None),
    };
    let values = match pair {
        PairAdjustment::Format1 { sets, .. } => sets.get(index).and_then(|set| set.get(right)),
        PairAdjustment::Format2 {
            classes, matrix, ..
        } => matrix.get((classes.0.get(left), classes.1.get(right))),
    };
    let Some((first, second)) = values else {
        return Ok(None);
    };
    if first.x_placement != 0
        || first.y_placement != 0
        || first.y_advance != 0
        || second.x_placement != 0
        || second.y_placement != 0
        || second.x_advance != 0
        || second.y_advance != 0
        || first.x_advance_device.is_some()
        || first.x_placement_device.is_some()
        || first.y_advance_device.is_some()
        || first.y_placement_device.is_some()
        || second.x_advance_device.is_some()
        || second.x_placement_device.is_some()
        || second.y_advance_device.is_some()
        || second.y_placement_device.is_some()
    {
        return Err(invalid("outline kern requires unsupported pair placement"));
    }
    Ok(Some(first.x_advance))
}
