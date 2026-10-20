//! Java-style actor labels, retaining server text and the shared font-pixel atlas.

use protocol_ids::ActorFlag;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use assets::RuntimeFontCatalog;
use client_world::ActorSnapshot;
use glam::Vec3;
use protocol::{ActorKind, ActorMetadataValue};
use render_model::{
    MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NAMETAG_DIM_TEXT_ALPHA as DIM_TEXT_ALPHA,
    NAMETAG_LINE_PITCH_PX, NametagRecord, NametagScene, java_nametag_line_backgrounds,
    nametag_line_left,
};
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};
use ui_screens::text::bounded_text;

use super::nametag_atlas::{GlyphPage, NametagAtlas};
mod counter;

#[cfg(test)]
pub(super) mod tests;

/// Presentation resource bound, not a native visibility rule.
pub(super) const MAX_PRESENTED_NAMETAGS: usize = 128;
const DEFAULT_RENDER_DISTANCE: f32 = 64.0;
const SNEAK_RENDER_DISTANCE: f32 = 32.0;
const HEAD_CLEARANCE: f32 = 0.5;
const DEFAULT_HEIGHT: f32 = 1.8;
/// Java's sneaking label moves down in camera space after its head translation.
const SNEAK_LABEL_DROP: f32 = 0.25;
const SNEAK_PLAYER_Y_OFFSET: f32 = 0.125;
const SLEEPING_LABEL_DROP: f32 = 1.5;
const METADATA_HEIGHT: u32 = 54;
const METADATA_ALWAYS_SHOW_NAMETAG: u32 = 81;
const METADATA_SCORE: u32 = 84;
const METADATA_RENDER_DISTANCE: u32 = 140;
const ACTOR_FLAG_ALWAYS_SHOW_NAME: u32 = 15;
const SCORE_DISTANCE_SQUARED: f32 = 100.0;
pub(super) const EXTRA_LINE_LIFT: f32 = 0.125;
pub(super) const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];
/// Each line needs one background and one text record.
pub(super) const NAMETAG_RECORDS_PER_LINE: usize = 2;
/// Most lines one tag can consume in a scene, including its backing plates.
const MAX_NAMETAG_LINES: usize = MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE;

/// Viewer feet control visibility and score range; the camera controls label draw order.
#[derive(Clone, Copy, Debug)]
pub struct NametagView {
    pub viewer_position: Vec3,
    pub camera_position: Vec3,
    /// Unit forward vector from the camera rotation, pointing into the scene.
    pub camera_forward: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NametagAnchor {
    pub(super) runtime_id: u64,
    /// Head anchor, before the world-Y multiline lift.
    pub(super) position: Vec3,
    pub(super) lines: Arc<[Arc<str>]>,
    pub(super) depth_tested: bool,
    pub(super) text_alpha: f32,
    /// Signed camera-space depth of the head anchor, independent of viewer distance.
    pub(super) view_depth: f32,
    /// Camera depth gained by one block of world-Y lift.
    pub(super) camera_forward_y: f32,
    pub(super) cinnabar_badge: bool,
    pub(super) counter: Option<Arc<ui::mod_hud::CounterAnnotation>>,
}

impl NametagAnchor {
    /// Attaches native literal counter runs only after the actor passed ordinary nametag eligibility.
    pub fn set_counter_annotation(&mut self, counter: Option<Arc<ui::mod_hud::CounterAnnotation>>) {
        self.counter = counter;
    }
    /// Native actor identity for presentation filters against retained world facts.
    pub const fn runtime_id(&self) -> u64 {
        self.runtime_id
    }

    /// Applies the shared roster's display decision after existing tag visibility selection.
    pub fn set_cinnabar_badge(&mut self, visible: bool) {
        self.cinnabar_badge = visible;
    }
}

/// Retains bounded tag lines for the actors selected for presentation.
#[derive(Default)]
pub struct NametagTextCache {
    entries: HashMap<(u64, i64), CachedNametagText>,
    #[cfg(test)]
    retention_checks: usize,
}

struct CachedNametagText {
    name: Arc<str>,
    score: Option<Arc<str>>,
    scoreboard_revision: Option<u64>,
    lines: Arc<[Arc<str>]>,
}

impl NametagTextCache {
    /// Checks each cached identity once against the selected actor identities.
    fn retain_selected(&mut self, selected: &HashSet<(u64, i64)>) {
        #[cfg(test)]
        {
            self.retention_checks = 0;
        }
        self.entries.retain(|identity, _| {
            #[cfg(test)]
            {
                self.retention_checks += 1;
            }
            selected.contains(identity)
        });
    }

    /// Reuses unchanged text while recomputing visibility, pose and viewer distance.
    fn extract(
        &mut self,
        actor: &ActorSnapshot,
        view: NametagView,
        picked_actor: Option<u64>,
        name: Arc<str>,
        scoreboards: &ui::ScoreboardStore,
        partial_tick: f32,
    ) -> Option<NametagAnchor> {
        if name.is_empty() {
            return None;
        }
        let (position, distance_squared) =
            nametag_geometry(actor, view.viewer_position, picked_actor, partial_tick)?;
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
                return nametag_with_lines(actor, position, view, Arc::clone(&cached.lines));
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
        nametag_with_lines(actor, position, view, lines)
    }
}

/// Reads the rendered pose, including client-owned local animation flags.
fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    actor_animation::AnimatedActor::flag(actor, bit)
}

/// The tag's height above the feet: the box height plus the head clearance, both following the
/// actor's size as the Scale metadata sets it. A published zero height counts, so a text host
/// with an empty box shows its tag one clearance up, and one at near-zero scale at its feet.
/// Java player pose offsets are applied after that translation.
fn tag_height(actor: &ActorSnapshot) -> f32 {
    let height = match actor.metadata.get(&METADATA_HEIGHT) {
        Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height >= 0.0 => *height,
        _ => DEFAULT_HEIGHT,
    };
    let offset = if !matches!(actor.kind, ActorKind::Player { .. }) {
        0.0
    } else if actor_flag(actor, ActorFlag::Sneaking.wire_value()) {
        SNEAK_PLAYER_Y_OFFSET
    } else if actor.is_sleeping() {
        SLEEPING_LABEL_DROP
    } else {
        0.0
    };
    (height + HEAD_CLEARANCE) * actor.render_scale() - offset
}

/// Adds the actor's current label height to the selected render feet.
fn tag_world_position(actor: &ActorSnapshot, feet: Vec3) -> Vec3 {
    feet + Vec3::Y * tag_height(actor)
}

fn render_distance(actor: &ActorSnapshot) -> f32 {
    let java_distance = if actor_flag(actor, ActorFlag::Sneaking.wire_value()) {
        SNEAK_RENDER_DISTANCE
    } else {
        DEFAULT_RENDER_DISTANCE
    };
    match actor.metadata.get(&METADATA_RENDER_DISTANCE) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() && *value >= 0.0 => *value,
        _ => java_distance,
    }
    .min(java_distance)
}

/// Applies explicit always-show overrides before the ordinary show-name gate.
fn visible(actor: &ActorSnapshot, picked_actor: Option<u64>) -> bool {
    if actor_flag(actor, ActorFlag::Invisible.wire_value()) {
        return false;
    }
    let always = actor_flag(actor, ACTOR_FLAG_ALWAYS_SHOW_NAME)
        || matches!(actor.metadata.get(&METADATA_ALWAYS_SHOW_NAMETAG),
            Some(ActorMetadataValue::Byte(value)) if *value != 0 && *value != -1);
    if always {
        return true;
    }
    // Missing flags retain the player default; an explicit server hide stays authoritative.
    if matches!(
        actor.metadata.get(&0),
        Some(ActorMetadataValue::Flags(_) | ActorMetadataValue::FlagsExtended(_))
    ) && !actor_flag(actor, ActorFlag::ShowName.wire_value())
    {
        return false;
    }
    matches!(actor.kind, ActorKind::Player { .. })
        || (actor_flag(actor, ActorFlag::ShowName.wire_value())
            && picked_actor == Some(actor.runtime_id))
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

/// Checks an actor's visibility and returns its unlifted anchor and squared viewer distance.
fn nametag_geometry(
    actor: &ActorSnapshot,
    viewer_position: Vec3,
    picked_actor: Option<u64>,
    partial_tick: f32,
) -> Option<(Vec3, f32)> {
    let feet = Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?);
    nametag_geometry_at_feet(actor, viewer_position, picked_actor, feet)
}

/// Applies actor visibility and distance rules to the caller's current render feet.
fn nametag_geometry_at_feet(
    actor: &ActorSnapshot,
    viewer_position: Vec3,
    picked_actor: Option<u64>,
    feet: Vec3,
) -> Option<(Vec3, f32)> {
    if !visible(actor, picked_actor) {
        return None;
    }
    let distance_squared = viewer_position.distance_squared(feet);
    let maximum = render_distance(actor);
    if !distance_squared.is_finite() || distance_squared >= maximum * maximum {
        return None;
    }
    let position = tag_world_position(actor, feet);
    if !position.is_finite() {
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
    view: NametagView,
    lines: Arc<[Arc<str>]>,
) -> Option<NametagAnchor> {
    if lines.is_empty() {
        return None;
    }
    let view_depth = view.camera_forward.dot(position - view.camera_position);
    if !view_depth.is_finite() {
        return None;
    }
    let sneaking = actor_flag(actor, ActorFlag::Sneaking.wire_value());
    Some(NametagAnchor {
        runtime_id: actor.runtime_id,
        position,
        lines,
        depth_tested: sneaking,
        text_alpha: if sneaking { DIM_TEXT_ALPHA } else { 1.0 },
        view_depth,
        camera_forward_y: view.camera_forward.y,
        cinnabar_badge: false,
        counter: None,
    })
}

/// Extracts a visible tag using the scene's bounded line prefix.
pub fn extract_nametag(
    actor: &ActorSnapshot,
    view: NametagView,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: &ui::ScoreboardStore,
    partial_tick: f32,
) -> Option<NametagAnchor> {
    let feet = Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?);
    extract_nametag_at_feet(actor, view, picked_actor, name, scoreboards, feet)
}

/// Extracts a tag from the same render feet as its body, without interpolating the
/// actor again. Metadata, visibility, score range and text limits remain authoritative.
pub fn extract_nametag_at_feet(
    actor: &ActorSnapshot,
    view: NametagView,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: &ui::ScoreboardStore,
    feet: Vec3,
) -> Option<NametagAnchor> {
    if name.is_empty() {
        return None;
    }
    let (position, distance_squared) =
        nametag_geometry_at_feet(actor, view.viewer_position, picked_actor, feet)?;
    let text = tag_text(actor, name, distance_squared, scoreboards);
    nametag_with_lines(actor, position, view, nametag_lines(&text))
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
    view: NametagView,
    picked_actor: Option<u64>,
    partial_tick: f32,
    show_players: bool,
) -> Vec<NametagAnchor> {
    let authority = stream.authority();
    // Java hides the label of a mount carrying a passenger, including the local player.
    let mounts: HashSet<_> = authority
        .remote_actors()
        .filter_map(|actor| authority.ridden_unique_id(actor.unique_id))
        .chain(authority.ridden_unique_id(authority.local_player_unique_id()))
        .collect();
    let mut candidates: Vec<_> = authority
        .remote_actors()
        .filter(|actor| !mounts.contains(&actor.unique_id))
        .filter(|actor| show_players || !matches!(actor.kind, ActorKind::Player { .. }))
        .filter_map(|actor| {
            let name = stream.authority().actor_name_tag(actor.unique_id)?;
            let (_, distance_squared) =
                nametag_geometry(actor, view.viewer_position, picked_actor, partial_tick)?;
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
    let selected: HashSet<_> = candidates
        .iter()
        .map(|(actor, _, _)| (actor.runtime_id, actor.unique_id))
        .collect();
    cache.retain_selected(&selected);
    candidates
        .into_iter()
        .filter_map(|(actor, name, _)| {
            cache.extract(actor, view, picked_actor, name, scoreboards, partial_tick)
        })
        .collect()
}

/// Draws complete labels back to front, keeping each plate and its text passes together.
/// `lift_scale` is the cosmetic scale applied to line lift after building the scene.
pub fn build_nametag_scene<'p>(
    anchors: &[NametagAnchor],
    lift_scale: f32,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    atlas: &mut NametagAtlas,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
) -> NametagScene {
    // Retain budget and atlas allocation order; final draw order follows placed geometry.
    let mut ordered: Vec<&NametagAnchor> = anchors.iter().collect();
    ordered.sort_by(|a, b| {
        b.view_depth
            .total_cmp(&a.view_depth)
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    let mut available_records = MAX_NAMETAG_RECORDS;
    // Any deferred atlas rebuild must finish before this frame captures its first cell UVs.
    atlas.begin_frame(ordered.iter().flat_map(|anchor| {
        let badge_records = usize::from(anchor.cinnabar_badge);
        let counter_records = anchor
            .counter
            .as_ref()
            .map_or(0, |counter| counter::record_budget(counter));
        let count = anchor.lines.len().min(
            available_records.saturating_sub(badge_records + counter_records)
                / NAMETAG_RECORDS_PER_LINE,
        );
        available_records -= count * NAMETAG_RECORDS_PER_LINE
            + usize::from(count != 0) * (badge_records + counter_records);
        anchor.lines.iter().take(count).chain(
            anchor
                .counter
                .as_ref()
                .into_iter()
                .flat_map(|counter| counter::texts(counter)),
        )
    }));
    let badge = ordered
        .iter()
        .any(|anchor| anchor.cinnabar_badge)
        .then(|| atlas.badge())
        .flatten();
    let side = NAMETAG_ATLAS_SIDE as f32;
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut records = Vec::with_capacity(MAX_NAMETAG_RECORDS - available_records);
    let mut labels = Vec::with_capacity(ordered.len());
    // One line buffer serves every label.
    let mut placed = Vec::new();
    for anchor in ordered {
        let badge = badge.filter(|_| anchor.cinnabar_badge);
        let badge_records = usize::from(badge.is_some());
        let counter_records = anchor
            .counter
            .as_ref()
            .map_or(0, |counter| counter::record_budget(counter));
        let remaining_lines = (MAX_NAMETAG_RECORDS - records.len())
            .saturating_sub(badge_records + counter_records)
            / NAMETAG_RECORDS_PER_LINE;
        placed.clear();
        let mut name_placed = false;
        placed.extend(
            anchor
                .lines
                .iter()
                .take(remaining_lines)
                .enumerate()
                .filter_map(|(index, line)| {
                    let line = atlas.line(line, font, layouts, pages)?;
                    name_placed |= index == 0;
                    Some(line)
                }),
        );
        if placed.is_empty() {
            continue;
        }
        let counter = anchor
            .counter
            .as_ref()
            .and_then(|policy| counter::Layout::measure(policy, atlas, font, layouts, pages));
        let separate = counter.as_ref().is_some_and(|counter| counter.separate());
        let before = counter.as_ref().is_some_and(|counter| counter.before());
        let counter_width = counter.as_ref().map_or(0., |counter| counter.width());
        let inline_width = counter
            .as_ref()
            .filter(|_| !separate)
            .map_or(0., |counter| counter.width() + counter.gap);
        let name_line_start = usize::from(separate && before);
        let [ink_top, ink_bottom] = placed[0].ink_y_px;
        let badge_size = ink_bottom - ink_top;
        let badge = badge.filter(|_| name_placed && badge_size > 0.0);
        let extra_width = if badge.is_some() {
            badge_size + 1.0
        } else {
            0.0
        };
        // Ordinary names draw at full opacity over geometry, including server cosmetics
        // that extend above the published actor bounds. Sneaking names still test depth.
        let common = NametagRecord {
            anchor: anchor.position.to_array(),
            line_lift: EXTRA_LINE_LIFT
                * (placed.len() + usize::from(separate)).saturating_sub(1) as f32,
            depth_tested: u32::from(anchor.depth_tested),
            ..NametagRecord::default()
        };
        let first = records.len();
        records.extend(
            java_nametag_line_backgrounds((0..placed.len() + usize::from(separate)).map(|line| {
                if separate && line == if before { 0 } else { placed.len() } {
                    counter_width
                } else {
                    let index = line - name_line_start;
                    placed[index].width_px
                        + if index == 0 {
                            extra_width + inline_width
                        } else {
                            0.
                        }
                }
            }))
            .map(|rect| NametagRecord {
                rect,
                uv: [0.0, 0.0, -1.0, -1.0],
                color: PLATE_COLOR,
                ..common
            }),
        );
        for (index, line) in placed.iter().enumerate() {
            let left = if index == 0 {
                nametag_line_left(line.width_px + extra_width + inline_width)
                    + extra_width
                    + if before { inline_width } else { 0. }
            } else {
                nametag_line_left(line.width_px)
            };
            let [x, y, width, height] = line.cell.map(|value| value as f32);
            let top = NAMETAG_LINE_PITCH_PX * (index + name_line_start) as f32 + line.top_px;
            records.push(NametagRecord {
                text: 1,
                rect: [left, top, left + width / texels, top + height / texels],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color: [1.0, 1.0, 1.0, anchor.text_alpha],
                ..common
            });
        }
        if let Some(counter) = &counter {
            let (left, line) = if separate {
                (
                    nametag_line_left(counter.width()),
                    if before { 0 } else { placed.len() },
                )
            } else {
                let left = nametag_line_left(placed[0].width_px + extra_width + inline_width);
                (
                    if before {
                        left
                    } else {
                        left + extra_width + placed[0].width_px + counter.gap
                    },
                    0,
                )
            };
            counter.append(&mut records, common, left, line, anchor.text_alpha);
        }
        if let Some(badge) = badge {
            let left = nametag_line_left(placed[0].width_px + extra_width + inline_width)
                + if before && !separate {
                    inline_width
                } else {
                    0.
                };
            let [x, y, width, height] = badge.cell.map(|value| value as f32);
            records.push(NametagRecord {
                text: 1,
                rect: [
                    left,
                    ink_top + NAMETAG_LINE_PITCH_PX * name_line_start as f32,
                    left + badge_size,
                    ink_bottom + NAMETAG_LINE_PITCH_PX * name_line_start as f32,
                ],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color: [1.0, 1.0, 1.0, anchor.text_alpha],
                ..common
            });
        }
        if anchor.depth_tested {
            let drop = SNEAK_LABEL_DROP / render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;
            for record in &mut records[first..] {
                record.rect[1] += drop;
                record.rect[3] += drop;
            }
        }
        let plane_depth =
            anchor.view_depth + anchor.camera_forward_y * (common.line_lift * lift_scale);
        labels.push((plane_depth, anchor.runtime_id, first..records.len()));
    }
    labels.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ordered_records = Vec::with_capacity(records.len());
    for (_, _, range) in labels {
        ordered_records.extend_from_slice(&records[range]);
    }
    let (atlas, atlas_revision) = atlas.publish();
    NametagScene {
        records: ordered_records,
        atlas,
        atlas_revision,
    }
}

/// Applies bounded cosmetic overrides without changing record order or depth selection.
pub(super) fn style_scene(
    scene: &mut NametagScene,
    style: &ui::mod_hud::NametagStyle,
    own_anchor: Option<[f32; 3]>,
) {
    if style.hide {
        scene.records.clear();
        return;
    }
    let original = std::mem::take(&mut scene.records);
    for mut record in original {
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
    }
}

#[cfg(test)]
mod selection_work_tests {
    use super::*;

    #[test]
    fn retention_checks_each_identity_once_and_preserves_selected_text() {
        let mut cache = NametagTextCache::default();
        let lines: Arc<[Arc<str>]> = vec![Arc::from("cached")].into();
        for runtime_id in 0..MAX_PRESENTED_NAMETAGS as u64 {
            cache.entries.insert(
                (runtime_id, runtime_id as i64),
                CachedNametagText {
                    name: Arc::from("cached"),
                    score: None,
                    scoreboard_revision: None,
                    lines: Arc::clone(&lines),
                },
            );
        }
        let selected: HashSet<_> = cache.entries.keys().copied().collect();
        let mut previous_checks = 0;
        for identity in cache.entries.keys() {
            assert!(selected.iter().any(|candidate| {
                previous_checks += 1;
                candidate == identity
            }));
        }
        assert_eq!(
            previous_checks,
            MAX_PRESENTED_NAMETAGS * (MAX_PRESENTED_NAMETAGS + 1) / 2
        );
        cache.retain_selected(&selected);
        assert_eq!(cache.retention_checks, MAX_PRESENTED_NAMETAGS);
        assert_eq!(cache.entries.len(), selected.len());
        assert!(
            cache
                .entries
                .values()
                .all(|entry| Arc::ptr_eq(&entry.lines, &lines))
        );
        // Reusing a runtime ID with a different unique ID must retire the old identity.
        cache.retain_selected(&HashSet::from([(0, -1), (1, 1)]));
        assert_eq!(cache.retention_checks, MAX_PRESENTED_NAMETAGS);
        assert_eq!(cache.entries.keys().copied().collect::<Vec<_>>(), [(1, 1)]);
        cache.retain_selected(&HashSet::new());
        assert_eq!(cache.retention_checks, 1);
        assert!(cache.entries.is_empty());
    }
}
