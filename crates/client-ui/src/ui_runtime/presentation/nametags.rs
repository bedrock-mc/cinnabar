//! Native name-tag extraction and font-pixel records for the retained GPU atlas.
//! See docs/reference/nametag-rendering.md for the vanilla rules.

use std::{collections::HashMap, sync::Arc};

use assets::RuntimeFontCatalog;
use client_world::ActorSnapshot;
use glam::Vec3;
use protocol::{ActorKind, ActorMetadataValue};
use render_model::{
    MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NAMETAG_LINE_PITCH_PX, NametagRecord, NametagScene,
    nametag_line_backgrounds, nametag_line_left,
};
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};
use ui_screens::text::bounded_text;

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
pub(super) const EXTRA_LINE_LIFT: f32 = 0.125;
pub(super) const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];
const SNEAK_TEXT_ALPHA: f32 = 0.125;
/// Each rendered line uses at most one background record and one text record.
const NAMETAG_RECORDS_PER_LINE: usize = 2;
/// Most lines one tag can consume in a scene, including its backing plates.
const MAX_NAMETAG_LINES: usize = MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE;

#[derive(Clone, Debug, PartialEq)]
pub struct NametagAnchor {
    pub(super) runtime_id: u64,
    /// Unlifted anchor: multiline lift must not change the eye-facing rotation.
    pub(super) position: Vec3,
    pub(super) lines: Arc<[Arc<str>]>,
    pub(super) depth_tested: bool,
    pub(super) text_alpha: f32,
    pub(super) distance: f32,
}

impl NametagAnchor {
    /// Native actor identity for presentation filters against retained world facts.
    pub const fn runtime_id(&self) -> u64 {
        self.runtime_id
    }
}

/// Retains bounded tag lines for the actors selected for presentation.
#[derive(Default)]
pub struct NametagTextCache {
    entries: HashMap<(u64, i64), CachedNametagText>,
}

struct CachedNametagText {
    name: Arc<str>,
    score: Option<Arc<str>>,
    scoreboard_revision: Option<u64>,
    lines: Arc<[Arc<str>]>,
}

impl NametagTextCache {
    /// Reuses unchanged text while recomputing visibility, pose and score distance.
    fn extract(
        &mut self,
        actor: &ActorSnapshot,
        eye: Vec3,
        picked_actor: Option<u64>,
        name: Arc<str>,
        scoreboards: &ui::ScoreboardStore,
        partial_tick: f32,
    ) -> Option<NametagAnchor> {
        if name.is_empty() {
            return None;
        }
        let (position, distance_squared) =
            nametag_geometry(actor, eye, picked_actor, partial_tick)?;
        let near_score = distance_squared < SCORE_DISTANCE_SQUARED;
        let score = match actor.metadata.get(&METADATA_SCORE) {
            Some(ActorMetadataValue::String(score)) if near_score => Some(Arc::clone(score)),
            _ => None,
        };
        let scoreboard_revision = (near_score && score.is_none()).then(|| scoreboards.revision());
        let key = (actor.runtime_id, actor.unique_id);
        if let Some(cached) = self.entries.get(&key) {
            let same_score = match (&cached.score, &score) {
                (Some(cached), Some(score)) => Arc::ptr_eq(cached, score),
                (None, None) => true,
                _ => false,
            };
            if Arc::ptr_eq(&cached.name, &name)
                && same_score
                && cached.scoreboard_revision == scoreboard_revision
            {
                return nametag_with_lines(
                    actor,
                    position,
                    distance_squared,
                    Arc::clone(&cached.lines),
                );
            }
        }
        let text = tag_text(actor, Arc::clone(&name), distance_squared, scoreboards);
        let lines = nametag_lines(&text);
        self.entries.insert(
            key,
            CachedNametagText {
                name,
                score,
                scoreboard_revision,
                lines: Arc::clone(&lines),
            },
        );
        nametag_with_lines(actor, position, distance_squared, lines)
    }
}

fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    matches!(actor.metadata.get(&0),
        Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags))
            if flags & (1_u64 << bit) != 0)
}

/// The tag's height above the feet: the box height plus the head clearance, both following the
/// actor's size as the Scale metadata sets it. A published zero height counts, so a text host
/// with an empty box shows its tag one clearance up, and one at near-zero scale at its feet.
fn tag_height(actor: &ActorSnapshot) -> f32 {
    let height = match actor.metadata.get(&METADATA_HEIGHT) {
        Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height >= 0.0 => *height,
        _ if actor_flag(actor, ACTOR_FLAG_SNEAKING) => SNEAKING_HEIGHT,
        _ => DEFAULT_HEIGHT,
    };
    (height + HEAD_CLEARANCE) * actor.render_scale()
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

/// Checks an actor's visibility and returns its unlifted anchor and squared eye distance.
fn nametag_geometry(
    actor: &ActorSnapshot,
    eye: Vec3,
    picked_actor: Option<u64>,
    partial_tick: f32,
) -> Option<(Vec3, f32)> {
    if !visible(actor, picked_actor) {
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
    Some((position, distance_squared))
}

/// Keeps only the nonempty line prefix that the scene and text layout can consume.
fn nametag_lines(text: &str) -> Arc<[Arc<str>]> {
    // A glyph needs at least one byte; each additional styled span needs a code and a glyph.
    let line_bytes = ui::UiLimits::MAX_TEXT_BYTES
        .min(ui::MAX_GLYPHS_PER_LAYOUT)
        .min(ui::MAX_TEXT_SPANS.saturating_mul("§aA".len()));
    // A tag holds several formatted lines, so it is bounded as the metadata string it came from
    // rather than as one line of presented text.
    bounded_text(text, protocol::MAX_ACTOR_METADATA_STRING_BYTES)
        .split('\n')
        .filter(|line| !line.is_empty())
        .take(MAX_NAMETAG_LINES)
        .map(|line| Arc::from(bounded_text(line, line_bytes)))
        .collect::<Vec<_>>()
        .into()
}

/// Adds current actor presentation facts to a retained set of nonempty tag lines.
fn nametag_with_lines(
    actor: &ActorSnapshot,
    position: Vec3,
    distance_squared: f32,
    lines: Arc<[Arc<str>]>,
) -> Option<NametagAnchor> {
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

/// Extracts a visible tag using the scene's bounded line prefix.
pub fn extract_nametag(
    actor: &ActorSnapshot,
    eye: Vec3,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: &ui::ScoreboardStore,
    partial_tick: f32,
) -> Option<NametagAnchor> {
    if name.is_empty() {
        return None;
    }
    let (position, distance_squared) = nametag_geometry(actor, eye, picked_actor, partial_tick)?;
    let text = tag_text(actor, name, distance_squared, scoreboards);
    nametag_with_lines(actor, position, distance_squared, nametag_lines(&text))
}

/// Rejects newline-only names before selection, while allowing a visible appended score.
fn has_nametag_lines(
    actor: &ActorSnapshot,
    name: &Arc<str>,
    distance_squared: f32,
    scoreboards: &ui::ScoreboardStore,
) -> bool {
    let maximum = protocol::MAX_ACTOR_METADATA_STRING_BYTES;
    if bounded_text(name, maximum)
        .bytes()
        .any(|byte| byte != b'\n')
    {
        return true;
    }
    if name.len() >= maximum {
        return false;
    }
    let text = tag_text(actor, Arc::clone(name), distance_squared, scoreboards);
    bounded_text(&text, maximum)
        .bytes()
        .any(|byte| byte != b'\n')
}

/// Selects the nearest eligible actors before extracting their bounded, cached tag text.
pub fn project_nametags(
    cache: &mut NametagTextCache,
    scoreboards: &ui::ScoreboardStore,
    stream: &chunk_pipeline::WorldStream,
    eye: Vec3,
    picked_actor: Option<u64>,
    partial_tick: f32,
    show_players: bool,
) -> Vec<NametagAnchor> {
    let mut candidates: Vec<_> = stream
        .authority()
        .remote_actors()
        .filter(|actor| show_players || !matches!(actor.kind, ActorKind::Player { .. }))
        .filter_map(|actor| {
            let name = stream.authority().actor_name_tag(actor.unique_id)?;
            let (_, distance_squared) = nametag_geometry(actor, eye, picked_actor, partial_tick)?;
            has_nametag_lines(actor, &name, distance_squared, scoreboards).then_some((
                actor,
                name,
                distance_squared.sqrt(),
            ))
        })
        .collect();
    candidates.sort_by(|(a, _, a_distance), (b, _, b_distance)| {
        a_distance
            .total_cmp(b_distance)
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    candidates.truncate(MAX_PRESENTED_NAMETAGS);
    cache.entries.retain(|&(runtime_id, unique_id), _| {
        candidates
            .iter()
            .any(|(actor, _, _)| actor.runtime_id == runtime_id && actor.unique_id == unique_id)
    });
    candidates
        .into_iter()
        .filter_map(|(actor, name, _)| {
            cache.extract(actor, eye, picked_actor, name, scoreboards, partial_tick)
        })
        .collect()
}

/// Retained line atlas with a background and centred glyphs per line; backgrounds precede glyphs.
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
    let requested_lines = ordered
        .iter()
        .map(|anchor| anchor.lines.len())
        .sum::<usize>()
        .min(MAX_NAMETAG_LINES);
    if !atlas.has_room_for(requested_lines) {
        atlas.reset();
    }
    let side = NAMETAG_ATLAS_SIDE as f32;
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut records = Vec::new();
    let mut see_through = 0;
    for anchor in ordered {
        let remaining_lines = (MAX_NAMETAG_RECORDS - records.len()) / NAMETAG_RECORDS_PER_LINE;
        let placed: Vec<_> = anchor
            .lines
            .iter()
            .take(remaining_lines)
            .filter_map(|line| atlas.line(line, font, layouts, pages))
            .collect();
        if placed.is_empty() {
            continue;
        }
        let common = NametagRecord {
            anchor: anchor.position.to_array(),
            line_lift: EXTRA_LINE_LIFT * placed.len().saturating_sub(1) as f32,
            ..NametagRecord::default()
        };
        records.extend(
            nametag_line_backgrounds(placed.iter().map(|line| line.width_px)).map(|rect| {
                NametagRecord {
                    rect,
                    uv: [0.0, 0.0, -1.0, -1.0],
                    color: PLATE_COLOR,
                    ..common
                }
            }),
        );
        for (index, line) in placed.iter().enumerate() {
            let left = nametag_line_left(line.width_px);
            let [x, y, width, height] = line.cell.map(|value| value as f32);
            let top = NAMETAG_LINE_PITCH_PX * index as f32 + line.top_px;
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

/// Applies bounded cosmetic overrides while preserving native depth pass membership.
pub(super) fn style_scene(
    scene: &mut NametagScene,
    style: &ui::mod_hud::NametagStyle,
    own_anchor: Option<[f32; 3]>,
) {
    if style.hide {
        scene.records.clear();
        scene.see_through = 0;
        return;
    }
    let original = std::mem::take(&mut scene.records);
    let old_boundary = scene.see_through;
    let mut boundary = 0;
    for (index, mut record) in original.into_iter().enumerate() {
        for coordinate in &mut record.rect {
            *coordinate *= style.scale;
        }
        record.line_lift *= style.scale;
        let mut extra = Vec::new();
        if record.uv[2] < 0.0 {
            if let Some(color) = style.background_color {
                record.color = color;
            }
            record.color[3] *= style.opacity;
            if let Some(mut color) = style.border_color {
                color[3] *= style.opacity;
                let [left, top, right, bottom] = record.rect;
                let width = style.border_width * style.scale;
                for rect in [
                    [left - width, top - width, right + width, top],
                    [left - width, bottom, right + width, bottom + width],
                    [left - width, top, left, bottom],
                    [right, top, right + width, bottom],
                ] {
                    extra.push(NametagRecord {
                        rect,
                        color,
                        ..record
                    });
                }
            }
        } else {
            let own = own_anchor == Some(record.anchor);
            let text_color = if own {
                style.own_text_color.or(style.text_color)
            } else {
                style.text_color
            };
            let replace = if own && style.own_text_color.is_some() {
                style.replace_own_text_color
            } else {
                style.replace_text_color
            };
            if let Some(color) = text_color {
                for (channel, tint) in record.color[..3].iter_mut().zip(color) {
                    *channel = if replace { tint } else { *channel * tint };
                }
                record.color[3] *= color[3];
            }
            record.color[3] *= style.opacity;
            if style.shadow {
                let mut shadow = record;
                shadow.rect = [
                    record.rect[0] + style.scale,
                    record.rect[1] + style.scale,
                    record.rect[2] + style.scale,
                    record.rect[3] + style.scale,
                ];
                shadow.color = [0., 0., 0., record.color[3] * 0.8];
                extra.push(shadow);
            }
        }
        if scene.records.len() + extra.len() < MAX_NAMETAG_RECORDS {
            scene.records.extend(extra);
            scene.records.push(record);
        }
        if index + 1 == old_boundary {
            boundary = scene.records.len();
        }
    }
    scene.see_through = boundary;
}
