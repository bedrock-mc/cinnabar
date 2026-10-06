//! Native name-tag extraction and font-pixel records for the retained GPU atlas.
//! See docs/reference/nametag-rendering.md for the vanilla rules.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use bevy::{camera::Camera, math::Vec3, prelude::GlobalTransform};
use client_world::ActorSnapshot;
use protocol::{ActorKind, ActorMetadataValue};
use render_model::{MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NametagRecord, NametagScene};
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};

use super::nametag_atlas::{GlyphPage, NametagAtlas};

#[cfg(test)]
pub(super) mod tests;

/// Presentation resource bound, not a native visibility rule.
pub(super) const MAX_PRESENTED_NAMETAGS: usize = 128;
const DEFAULT_RENDER_DISTANCE: f32 = 64.0;
const HEAD_CLEARANCE: f32 = 0.7;
const DEFAULT_HEIGHT: f32 = 1.8;
const SNEAKING_HEIGHT: f32 = 1.5;
const METADATA_HEIGHT: u32 = 54;
const METADATA_ALWAYS_SHOW_NAMETAG: u32 = 81;
const METADATA_SCORE: u32 = 84;
const METADATA_RENDER_DISTANCE: u32 = 140;
const ACTOR_FLAG_SNEAKING: u32 = 1;
const ACTOR_FLAG_INVISIBLE: u32 = 5;
const ACTOR_FLAG_SHOW_NAME: u32 = 14;
const ACTOR_FLAG_ALWAYS_SHOW_NAME: u32 = 15;
const SCORE_DISTANCE_SQUARED: f32 = 100.0;
pub(super) const LINE_PITCH_PX: f32 = 10.0;
pub(super) const EXTRA_LINE_LIFT: f32 = 0.125;
pub(super) const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];
const SNEAK_TEXT_ALPHA: f32 = 0.125;

#[derive(Clone, Debug, PartialEq)]
pub struct NametagAnchor {
    pub(super) runtime_id: u64,
    /// Unlifted anchor: multiline lift must not change the eye-facing rotation.
    pub(super) position: Vec3,
    pub(super) lines: Vec<Arc<str>>,
    pub(super) depth_tested: bool,
    pub(super) text_alpha: f32,
    pub(super) distance: f32,
}

fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    matches!(actor.metadata.get(&0),
        Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags))
            if flags & (1_u64 << bit) != 0)
}

fn tag_height(actor: &ActorSnapshot) -> f32 {
    let height = match actor.metadata.get(&METADATA_HEIGHT) {
        Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height > 0.0 => *height,
        _ if actor_flag(actor, ACTOR_FLAG_SNEAKING) => SNEAKING_HEIGHT * actor.render_scale(),
        _ => DEFAULT_HEIGHT * actor.render_scale(),
    };
    height + HEAD_CLEARANCE
}

fn tag_world_position(actor: &ActorSnapshot, partial_tick: f32) -> Option<Vec3> {
    Some(
        Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?)
            + Vec3::Y * tag_height(actor),
    )
}

fn render_distance(actor: &ActorSnapshot) -> f32 {
    match actor.metadata.get(&METADATA_RENDER_DISTANCE) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() && *value >= 0.0 => *value,
        _ => DEFAULT_RENDER_DISTANCE,
    }
}

fn visible(actor: &ActorSnapshot, picked_actor: Option<u64>) -> bool {
    if actor_flag(actor, ACTOR_FLAG_INVISIBLE) {
        return false;
    }
    let always = matches!(actor.kind, ActorKind::Player { .. })
        || actor_flag(actor, ACTOR_FLAG_ALWAYS_SHOW_NAME)
        || matches!(actor.metadata.get(&METADATA_ALWAYS_SHOW_NAMETAG),
            Some(ActorMetadataValue::Byte(value)) if *value != 0);
    always || (actor_flag(actor, ACTOR_FLAG_SHOW_NAME) && picked_actor == Some(actor.runtime_id))
}

fn tag_text(
    actor: &ActorSnapshot,
    name: Arc<str>,
    distance_squared: f32,
    scoreboards: &ui::ScoreboardStore,
) -> Arc<str> {
    if distance_squared >= SCORE_DISTANCE_SQUARED {
        return name;
    }
    // Empty synced score text is authoritative; retained objectives are only a fallback.
    match actor.metadata.get(&METADATA_SCORE) {
        Some(ActorMetadataValue::String(score)) if !score.is_empty() => {
            format!("{name}\n{score}").into()
        }
        Some(ActorMetadataValue::String(_)) => name,
        _ => scoreboards
            .below_name_for_owner(&ui::ScoreOwner::Player(actor.unique_id))
            .or_else(|| scoreboards.below_name_for_owner(&ui::ScoreOwner::Entity(actor.unique_id)))
            .map_or_else(
                || Arc::clone(&name),
                |(score, objective)| format!("{name}\n{score} {objective}").into(),
            ),
    }
}

pub fn extract_nametag(
    actor: &ActorSnapshot,
    eye: Vec3,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: &ui::ScoreboardStore,
    partial_tick: f32,
) -> Option<NametagAnchor> {
    if name.is_empty() || !visible(actor, picked_actor) {
        return None;
    }
    let feet = Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?);
    let distance_squared = eye.distance_squared(feet);
    let maximum = render_distance(actor);
    if !distance_squared.is_finite() || distance_squared > maximum * maximum {
        return None;
    }
    let position = tag_world_position(actor, partial_tick)?;
    if !position.is_finite() || eye == position || !(eye - position).length_squared().is_finite() {
        return None;
    }
    let text = tag_text(actor, name, distance_squared, scoreboards);
    let lines: Vec<Arc<str>> = super::bounded_visible_text(&text)
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(Arc::from)
        .collect();
    if lines.is_empty() {
        return None;
    }
    let sneaking = actor_flag(actor, ACTOR_FLAG_SNEAKING);
    Some(NametagAnchor {
        runtime_id: actor.runtime_id,
        position,
        lines,
        depth_tested: sneaking,
        text_alpha: if sneaking { SNEAK_TEXT_ALPHA } else { 1.0 },
        distance: distance_squared.sqrt(),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn project_nametags(
    scoreboards: &ui::ScoreboardStore,
    stream: &chunk_pipeline::WorldStream,
    _camera: &Camera,
    camera_transform: &GlobalTransform,
    _logical_size: [f32; 2],
    picked_actor: Option<u64>,
    partial_tick: f32,
    show_players: bool,
) -> Vec<NametagAnchor> {
    let eye = camera_transform.translation();
    let mut anchors: Vec<_> = stream
        .authority()
        .remote_actors()
        .filter(|actor| show_players || !matches!(actor.kind, ActorKind::Player { .. }))
        .filter_map(|actor| {
            extract_nametag(
                actor,
                eye,
                picked_actor,
                stream.authority().actor_name_tag(actor.unique_id)?,
                scoreboards,
                partial_tick,
            )
        })
        .collect();
    anchors.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    anchors.truncate(MAX_PRESENTED_NAMETAGS);
    anchors
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
