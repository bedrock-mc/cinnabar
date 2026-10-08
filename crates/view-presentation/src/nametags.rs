//! Native billboard text geometry from admitted name-tag anchors.

use crate::nametag_atlas::{GlyphPage, NametagAtlas};
use assets::RuntimeFontCatalog;
use bevy::math::Vec3;
use render_model::{MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NametagRecord, NametagScene};
use std::sync::Arc;
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};

pub const DEFAULT_RENDER_DISTANCE: f32 = 64.0;
pub const HEAD_CLEARANCE: f32 = 0.7;
pub const DEFAULT_HEIGHT: f32 = 1.8;
pub const SNEAKING_HEIGHT: f32 = 1.5;
pub const SNEAK_TEXT_ALPHA: f32 = 0.125;
pub const LINE_PITCH_PX: f32 = 10.0;
pub const EXTRA_LINE_LIFT: f32 = 0.125;
pub const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];

#[derive(Clone, Debug, PartialEq)]
pub struct NametagAnchor {
    pub runtime_id: u64,
    /// Unlifted anchor: multiline lift must not change the eye-facing rotation.
    pub position: Vec3,
    pub lines: Vec<Arc<str>>,
    pub depth_tested: bool,
    pub text_alpha: f32,
    pub distance: f32,
}

/// Retained line atlas, one continuous plate per actor, and individually centred glyph lines.
pub fn build_nametag_scene<'p>(
    anchors: &[NametagAnchor],
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    atlas: &mut NametagAtlas,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
) -> NametagScene {
    let mut ordered: Vec<&NametagAnchor> = anchors.iter().collect();
    ordered.sort_by(|a, b| {
        a.depth_tested
            .cmp(&b.depth_tested)
            .then(b.distance.total_cmp(&a.distance))
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    if !atlas.has_room_for(ordered.iter().map(|anchor| anchor.lines.len()).sum()) {
        atlas.reset();
    }
    let side = NAMETAG_ATLAS_SIDE as f32;
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut records = Vec::new();
    let mut see_through = 0;
    for anchor in ordered {
        let placed: Vec<_> = anchor
            .lines
            .iter()
            .filter_map(|line| atlas.line(line, font, layouts, pages))
            .collect();
        if placed.is_empty() || records.len() + placed.len() + 1 > MAX_NAMETAG_RECORDS {
            continue;
        }
        let half = placed
            .iter()
            .map(|line| line.width_px as u32 / 2)
            .max()
            .unwrap_or(0) as f32;
        let common = NametagRecord {
            anchor: anchor.position.to_array(),
            line_lift: EXTRA_LINE_LIFT * placed.len().saturating_sub(1) as f32,
            ..NametagRecord::default()
        };
        if placed.len() > 1 || half > 0.0 {
            records.push(NametagRecord {
                rect: [
                    -(half + 1.0),
                    -1.0,
                    half + 1.0,
                    LINE_PITCH_PX * placed.len() as f32 - 1.0,
                ],
                uv: [0.0, 0.0, -1.0, -1.0],
                color: PLATE_COLOR,
                ..common
            });
        }
        for (index, line) in placed.iter().enumerate() {
            let left = -((line.width_px as u32 / 2) as f32);
            let [x, y, width, height] = line.cell.map(|value| value as f32);
            let top = LINE_PITCH_PX * index as f32 + line.top_px;
            records.push(NametagRecord {
                text: 1,
                rect: [left, top, left + width / texels, top + height / texels],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color: [1.0, 1.0, 1.0, anchor.text_alpha],
                ..common
            });
        }
        if !anchor.depth_tested {
            see_through = records.len();
        }
    }
    let (atlas, atlas_revision) = atlas.publish();
    NametagScene {
        records,
        see_through,
        atlas,
        atlas_revision,
    }
}

/// Admitted nameplate text and position through the native distance and sneak policy.
#[allow(clippy::too_many_arguments)]
pub fn nametag_anchor(
    runtime_id: u64,
    text: &str,
    feet: Vec3,
    position: Vec3,
    eye: Vec3,
    sneaking: bool,
    maximum_distance: f32,
) -> Option<NametagAnchor> {
    let distance_squared = eye.distance_squared(feet);
    if !feet.is_finite()
        || !position.is_finite()
        || !distance_squared.is_finite()
        || distance_squared > maximum_distance * maximum_distance
        || eye == position
        || !(eye - position).length_squared().is_finite()
    {
        return None;
    }
    let lines: Vec<Arc<str>> = crate::text::bounded_visible_text(text)
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(Arc::from)
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(NametagAnchor {
        runtime_id,
        position,
        lines,
        depth_tested: sneaking,
        text_alpha: if sneaking { SNEAK_TEXT_ALPHA } else { 1.0 },
        distance: distance_squared.sqrt(),
    })
}

/// Public player name using native player defaults when no metadata is streamed.
pub fn player_nametag_anchor(
    name: &str,
    feet: Vec3,
    eye: Vec3,
    sneaking: bool,
) -> Option<NametagAnchor> {
    let height = if sneaking {
        SNEAKING_HEIGHT
    } else {
        DEFAULT_HEIGHT
    };
    nametag_anchor(
        0,
        name,
        feet,
        feet + Vec3::Y * (height + HEAD_CLEARANCE),
        eye,
        sneaking,
        DEFAULT_RENDER_DISTANCE,
    )
}
