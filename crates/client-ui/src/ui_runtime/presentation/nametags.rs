//! Java-style actor labels, retaining server text and the shared font-pixel atlas.

use protocol_ids::{ActorDataKey, ActorFlag};
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
pub(super) mod bounds;
mod counter;
pub(super) mod style;

#[cfg(test)]
pub(super) mod tests;

/// Presentation resource bound, not a native visibility rule.
pub(super) const MAX_PRESENTED_NAMETAGS: usize = 128;
const DEFAULT_RENDER_DISTANCE: f32 = 64.0;
const HEAD_CLEARANCE: f32 = 0.5;
const DEFAULT_HEIGHT: f32 = 1.8;
/// Java's sneaking label moves down in camera space after its head translation.
const SNEAK_LABEL_DROP: f32 = 0.25;
const SNEAK_PLAYER_Y_OFFSET: f32 = 0.125;
const SLEEPING_LABEL_DROP: f32 = 1.5;
const METADATA_HEIGHT: u32 = ActorDataKey::Height.wire_value();
const METADATA_ALWAYS_SHOW_NAMETAG: u32 = ActorDataKey::AlwaysShowNameTag.wire_value();
const METADATA_SCORE: u32 = ActorDataKey::Score.wire_value();
const METADATA_RENDER_DISTANCE: u32 = ActorDataKey::NameplateRenderDistance.wire_value();
const SCORE_DISTANCE_SQUARED: f32 = 100.0;
pub(super) const EXTRA_LINE_LIFT: f32 = 0.125;
pub(super) const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];
/// Each line needs one background and one text record.
pub(super) const NAMETAG_RECORDS_PER_LINE: usize = 2;
/// Most lines one tag can consume in a scene, including its backing plates.
const MAX_NAMETAG_LINES: usize = MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE;

/// The camera controls visibility, score range and label draw order.
#[derive(Clone, Copy, Debug)]
pub struct NametagView {
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
    pub(super) sneaking: bool,
    pub(super) text_alpha: f32,
    /// Signed camera-space depth of the head anchor, independent of camera distance.
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

/// Retains bounded tag lines for queried actors, including frustum-rejected labels.
#[derive(Default)]
pub struct NametagTextCache {
    entries: HashMap<(u64, i64), CachedNametagText>,
    scratch: ProjectionScratch,
    anchors: Vec<NametagAnchor>,
    #[cfg(test)]
    retention_checks: usize,
}

struct CachedNametagText {
    name: Arc<str>,
    score: Option<Arc<str>>,
    scoreboard_revision: Option<u64>,
    lines: Arc<[Arc<str>]>,
    queried: bool,
}

#[derive(Default)]
struct ProjectionScratch {
    mounts: HashSet<i64>,
    candidates: Vec<(u64, Arc<str>, f32)>,
}

impl NametagTextCache {
    /// Reuses the retired presentation buffer after the new frame replaces it.
    pub fn recycle_anchors(&mut self, mut anchors: Vec<NametagAnchor>) {
        anchors.clear();
        if self.anchors.capacity() > anchors.capacity() {
            return;
        }
        anchors.reserve(MAX_PRESENTED_NAMETAGS);
        self.anchors = anchors;
    }

    /// Retires identities that no longer reach text extraction this frame.
    fn retain_queried(&mut self) {
        #[cfg(test)]
        {
            self.retention_checks = 0;
        }
        self.entries.retain(|_, cached| {
            #[cfg(test)]
            {
                self.retention_checks += 1;
            }
            cached.queried
        });
        if self.entries.capacity() > self.entries.len().saturating_mul(4) {
            self.entries.shrink_to(self.entries.len().saturating_mul(2));
        }
    }

    /// Reuses unchanged text while recomputing visibility, pose and camera distance.
    fn extract(
        &mut self,
        actor: &ActorSnapshot,
        view: NametagView,
        picked_actor: Option<u64>,
        name: Arc<str>,
        scoreboards: Option<&ui::ScoreboardStore>,
        partial_tick: f32,
    ) -> Option<NametagAnchor> {
        if name.is_empty() {
            return None;
        }
        let (position, distance_squared) =
            nametag_geometry(actor, view.camera_position, picked_actor, partial_tick)?;
        let near_score = scoreboards.is_some() && distance_squared < SCORE_DISTANCE_SQUARED;
        let score = match actor.metadata.get(&METADATA_SCORE) {
            Some(ActorMetadataValue::String(score)) if near_score => Some(Arc::clone(score)),
            _ => None,
        };
        let scoreboard_revision = scoreboards
            .filter(|_| near_score && score.is_none())
            .map(ui::ScoreboardStore::revision);
        let key = (actor.runtime_id, actor.unique_id);
        if let Some(cached) = self.entries.get_mut(&key) {
            let same_score = match (&cached.score, &score) {
                (Some(cached), Some(score)) => Arc::ptr_eq(cached, score),
                (None, None) => true,
                _ => false,
            };
            if Arc::ptr_eq(&cached.name, &name)
                && same_score
                && cached.scoreboard_revision == scoreboard_revision
            {
                cached.queried = true;
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
                queried: true,
            },
        );
        nametag_with_lines(actor, position, view, lines)
    }
}

/// Reads the rendered pose, including client-owned local animation flags.
fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    actor_animation::AnimatedActor::flag(actor, bit)
}

/// Follows the published box height, as vanilla anchors every actor's label to its box, except
/// that crouched and sleeping players keep the standing height before their Java-style drop.
/// Raw metadata height and clearance each receive the render scale once.
fn tag_height(actor: &ActorSnapshot) -> f32 {
    let player = matches!(actor.kind, ActorKind::Player { .. });
    let prone = player
        && [ActorFlag::Swimming, ActorFlag::Crawling, ActorFlag::Gliding]
            .into_iter()
            .any(|flag| actor_flag(actor, flag.wire_value()));
    let offset = if !player || prone {
        0.0
    } else if actor_flag(actor, ActorFlag::Sneaking.wire_value()) {
        SNEAK_PLAYER_Y_OFFSET
    } else if actor.is_sleeping() {
        SLEEPING_LABEL_DROP
    } else {
        0.0
    };
    let height = match actor.metadata.get(&METADATA_HEIGHT) {
        // The posed box has already shrunk; the Java drop applies to the standing height.
        _ if offset != 0.0 => DEFAULT_HEIGHT,
        Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height >= 0.0 => *height,
        _ => DEFAULT_HEIGHT,
    };
    (height + HEAD_CLEARANCE) * actor.render_scale() - offset
}

/// Adds the actor's current label height to the selected render feet.
fn tag_world_position(actor: &ActorSnapshot, feet: Vec3) -> Vec3 {
    feet + Vec3::Y * tag_height(actor)
}

/// Uses the server's distance or the constructor default, without a pose-dependent cap.
fn render_distance(actor: &ActorSnapshot) -> f32 {
    match actor.metadata.get(&METADATA_RENDER_DISTANCE) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        _ => DEFAULT_RENDER_DISTANCE,
    }
}

/// Shows named actors requested by the server or selected by the current hit result.
fn visible(actor: &ActorSnapshot, picked_actor: Option<u64>) -> bool {
    !actor_flag(actor, ActorFlag::Invisible.wire_value())
        && (matches!(actor.metadata.get(&METADATA_ALWAYS_SHOW_NAMETAG),
            Some(ActorMetadataValue::Byte(value)) if *value != 0 && *value != -1)
            || picked_actor == Some(actor.runtime_id))
}

/// Appends nearby score text when the caller admits a score line.
fn tag_text(
    actor: &ActorSnapshot,
    name: Arc<str>,
    distance_squared: f32,
    scoreboards: Option<&ui::ScoreboardStore>,
) -> Arc<str> {
    let Some(scoreboards) = scoreboards.filter(|_| distance_squared < SCORE_DISTANCE_SQUARED)
    else {
        return name;
    };
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

/// Checks an actor's visibility and returns its unlifted anchor and squared camera distance.
fn nametag_geometry(
    actor: &ActorSnapshot,
    camera_position: Vec3,
    picked_actor: Option<u64>,
    partial_tick: f32,
) -> Option<(Vec3, f32)> {
    let feet = Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?);
    nametag_geometry_at_feet(actor, camera_position, picked_actor, feet)
}

/// Applies actor visibility and distance rules to the caller's current render feet.
fn nametag_geometry_at_feet(
    actor: &ActorSnapshot,
    camera_position: Vec3,
    picked_actor: Option<u64>,
    feet: Vec3,
) -> Option<(Vec3, f32)> {
    if !visible(actor, picked_actor) {
        return None;
    }
    let distance_squared = camera_position.distance_squared(feet);
    let maximum = render_distance(actor);
    if !distance_squared.is_finite() || distance_squared > maximum * maximum {
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
        sneaking,
        depth_tested: sneaking || actor_flag(actor, ActorFlag::NameplateDepthTested.wire_value()),
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
    extract_nametag_at_feet(actor, view, picked_actor, name, Some(scoreboards), feet)
}

/// Extracts a tag from the same render feet as its body, without interpolating the
/// actor again. Absent score authority suppresses the appended score line.
pub fn extract_nametag_at_feet(
    actor: &ActorSnapshot,
    view: NametagView,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: Option<&ui::ScoreboardStore>,
    feet: Vec3,
) -> Option<NametagAnchor> {
    if name.is_empty() {
        return None;
    }
    let (position, distance_squared) =
        nametag_geometry_at_feet(actor, view.camera_position, picked_actor, feet)?;
    let text = tag_text(actor, name, distance_squared, scoreboards);
    nametag_with_lines(actor, position, view, nametag_lines(&text))
}

/// Rejects newline-only names before selection, while allowing a visible appended score.
fn has_nametag_lines(
    actor: &ActorSnapshot,
    name: &Arc<str>,
    distance_squared: f32,
    scoreboards: Option<&ui::ScoreboardStore>,
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
    let Some(scoreboards) = scoreboards.filter(|_| distance_squared < SCORE_DISTANCE_SQUARED)
    else {
        return false;
    };
    let remaining = maximum.saturating_sub(name.len() + 1);
    match actor.metadata.get(&METADATA_SCORE) {
        Some(ActorMetadataValue::String(score)) => bounded_text(score, remaining)
            .bytes()
            .any(|byte| byte != b'\n'),
        _ => {
            remaining != 0
                && (scoreboards
                    .below_name_for_owner(&ui::ScoreOwner::Player(actor.unique_id))
                    .or_else(|| {
                        scoreboards.below_name_for_owner(&ui::ScoreOwner::Entity(actor.unique_id))
                    })
                    .is_some())
        }
    }
}

/// Selects the nearest on-screen labels after native eligibility, before applying the tag cap.
#[allow(
    clippy::too_many_arguments,
    reason = "Presentation bounds are supplied independently of actor eligibility."
)]
pub fn project_nametags(
    cache: &mut NametagTextCache,
    scoreboards: &ui::ScoreboardStore,
    stream: &chunk_pipeline::WorldStream,
    view: NametagView,
    picked_actor: Option<u64>,
    partial_tick: f32,
    show_players: bool,
    mut admit: impl FnMut(&mut NametagAnchor) -> bool,
) -> Vec<NametagAnchor> {
    let authority = stream.authority();
    for cached in cache.entries.values_mut() {
        cached.queried = false;
    }
    let mut scratch = std::mem::take(&mut cache.scratch);
    let ProjectionScratch { mounts, candidates } = &mut scratch;
    // Passengers suppress the appended score, while the mount keeps its name.
    mounts.clear();
    mounts.extend(
        authority
            .remote_actors()
            .filter_map(|actor| authority.ridden_unique_id(actor.unique_id))
            .chain(authority.ridden_unique_id(authority.local_player_unique_id())),
    );
    candidates.clear();
    candidates.extend(
        authority
            .remote_actors()
            .filter(|actor| show_players || !matches!(actor.kind, ActorKind::Player { .. }))
            .filter_map(|actor| {
                let name = stream.authority().actor_name_tag(actor.unique_id)?;
                let (_, distance_squared) =
                    nametag_geometry(actor, view.camera_position, picked_actor, partial_tick)?;
                has_nametag_lines(
                    actor,
                    &name,
                    distance_squared,
                    (!mounts.contains(&actor.unique_id)).then_some(scoreboards),
                )
                .then_some((actor.runtime_id, name, distance_squared.sqrt()))
            }),
    );
    candidates.sort_unstable_by(|(a, _, a_distance), (b, _, b_distance)| {
        a_distance.total_cmp(b_distance).then(a.cmp(b))
    });
    let mut anchors = std::mem::take(&mut cache.anchors);
    anchors.reserve(MAX_PRESENTED_NAMETAGS);
    for (runtime_id, name, _) in candidates.drain(..) {
        let Some(actor) = authority.actor(runtime_id) else {
            continue;
        };
        let Some(mut anchor) = cache.extract(
            actor,
            view,
            picked_actor,
            name,
            (!mounts.contains(&actor.unique_id)).then_some(scoreboards),
            partial_tick,
        ) else {
            continue;
        };
        if !admit(&mut anchor) {
            continue;
        }
        anchors.push(anchor);
        if anchors.len() == MAX_PRESENTED_NAMETAGS {
            break;
        }
    }
    cache.retain_queried();
    cache.scratch = scratch;
    anchors
}

/// Draws complete labels back to front, keeping each plate and its text passes together.
/// Reserves cosmetic expansion before atlas requests or line-prefix admission.
pub fn build_nametag_scene<'p>(
    anchors: &[NametagAnchor],
    style: Option<&ui::mod_hud::NametagStyle>,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    atlas: &mut NametagAtlas,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
) -> NametagScene {
    let lift_scale = style.map_or(1.0, |style| style.scale);
    let text_cost = style::record_cost(style, false);
    let line_cost = text_cost + style::record_cost(style, true);
    // Admit nearer labels first; final draw order follows placed geometry.
    let mut ordered: Vec<&NametagAnchor> = anchors.iter().collect();
    ordered.sort_by(|a, b| {
        a.view_depth
            .total_cmp(&b.view_depth)
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    let mut available_records = MAX_NAMETAG_RECORDS;
    // Any deferred atlas rebuild must finish before this frame captures its first cell UVs.
    let requested: Vec<_> = ordered
        .iter()
        .flat_map(|anchor| {
            let badge_records = usize::from(anchor.cinnabar_badge) * text_cost;
            let counter_records = anchor
                .counter
                .as_ref()
                .map_or(0, |counter| counter::record_budget(counter, style));
            let count = anchor
                .lines
                .len()
                .min(available_records.saturating_sub(badge_records + counter_records) / line_cost);
            available_records -=
                count * line_cost + usize::from(count != 0) * (badge_records + counter_records);
            anchor.lines.iter().take(count).chain(
                anchor
                    .counter
                    .as_ref()
                    .filter(|_| count != 0)
                    .into_iter()
                    .flat_map(|counter| counter::texts(counter)),
            )
        })
        .cloned()
        .collect();
    atlas.prepare_frame(
        &requested,
        ordered.iter().any(|anchor| anchor.cinnabar_badge),
        font,
        layouts,
        pages,
    );
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
    available_records = MAX_NAMETAG_RECORDS;
    for anchor in ordered {
        let badge = badge.filter(|_| anchor.cinnabar_badge);
        let badge_records = usize::from(badge.is_some()) * text_cost;
        let counter_records = anchor
            .counter
            .as_ref()
            .map_or(0, |counter| counter::record_budget(counter, style));
        let remaining_lines =
            available_records.saturating_sub(badge_records + counter_records) / line_cost;
        placed.clear();
        placed.extend(
            anchor
                .lines
                .iter()
                .take(remaining_lines)
                .map(|line| atlas.line(line, font, layouts, pages)),
        );
        while placed.last().is_some_and(Option::is_none) {
            placed.pop();
        }
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
        let name_metrics = placed[0]
            .map(|line| (line.width_px, line.ink_y_px))
            .or_else(|| {
                super::nametag_atlas::line_metrics(&anchor.lines[0], font, layouts)
                    .map(|metrics| (metrics.width_px, metrics.ink_y_px))
            });
        let (name_width, [ink_top, ink_bottom]) = name_metrics.unwrap_or_default();
        let badge_size = ink_bottom - ink_top;
        let badge = badge.filter(|_| badge_size > 0.0);
        let extra_width = if badge.is_some() {
            badge_size + 1.0
        } else {
            0.0
        };
        // Ordinary names draw at full opacity over geometry, including server cosmetics
        // that extend above the published actor bounds. Sneaking names still test depth.
        let common = NametagRecord {
            anchor: anchor.position.to_array(),
            runtime_id: anchor.runtime_id,
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
                    placed[index].map_or(0.0, |line| line.width_px)
                        + if index == 0 {
                            extra_width + inline_width
                        } else {
                            0.
                        }
                }
            }))
            .enumerate()
            .filter(|(index, _)| {
                (separate && *index == if before { 0 } else { placed.len() })
                    || placed[index - name_line_start].is_some()
            })
            .map(|(_, rect)| NametagRecord {
                rect,
                uv: [0.0, 0.0, -1.0, -1.0],
                color: PLATE_COLOR,
                ..common
            }),
        );
        for (index, line) in placed.iter().enumerate() {
            let Some(line) = line else {
                continue;
            };
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
                let left = nametag_line_left(name_width + extra_width + inline_width);
                (
                    if before {
                        left
                    } else {
                        left + extra_width + name_width + counter.gap
                    },
                    0,
                )
            };
            counter.append(&mut records, common, left, line, anchor.text_alpha);
        }
        if let Some(badge) = badge {
            let left = nametag_line_left(name_width + extra_width + inline_width)
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
        if anchor.sneaking {
            let drop = SNEAK_LABEL_DROP / render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;
            for record in &mut records[first..] {
                record.rect[1] += drop;
                record.rect[3] += drop;
            }
        }
        let plane_depth =
            anchor.view_depth + anchor.camera_forward_y * (common.line_lift * lift_scale);
        available_records -= records[first..]
            .iter()
            .map(|record| style::record_cost(style, record.uv[2] < 0.0))
            .sum::<usize>();
        labels.push((
            plane_depth,
            anchor.runtime_id,
            anchor.view_depth,
            first..records.len(),
        ));
    }
    labels.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ordered_records = Vec::with_capacity(records.len());
    let mut ordered_labels = Vec::with_capacity(labels.len());
    for (_, _, view_depth, range) in labels {
        let first = ordered_records.len();
        ordered_records.extend_from_slice(&records[range]);
        ordered_labels.push(render_model::NametagLabel {
            records: first..ordered_records.len(),
            view_depth,
        });
    }
    let (atlas, atlas_revision) = atlas.publish();
    NametagScene {
        records: ordered_records,
        labels: ordered_labels,
        atlas,
        atlas_revision,
    }
}

#[cfg(test)]
mod selection_work_tests {
    use super::*;

    #[test]
    fn frames_without_projection_keep_the_warm_spare_buffer() {
        let mut cache = NametagTextCache::default();
        cache.recycle_anchors(Vec::new());
        let (_, allocations) = test_alloc::count(|| {
            cache.recycle_anchors(Vec::new());
            cache.recycle_anchors(Vec::new());
        });
        assert_eq!(allocations, 0);
        assert!(cache.anchors.capacity() >= MAX_PRESENTED_NAMETAGS);
    }

    #[test]
    fn retention_checks_each_identity_once_and_preserves_queried_text() {
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
                    queried: true,
                },
            );
        }
        cache.retain_queried();
        assert_eq!(cache.retention_checks, MAX_PRESENTED_NAMETAGS);
        assert_eq!(cache.entries.len(), MAX_PRESENTED_NAMETAGS);
        assert!(
            cache
                .entries
                .values()
                .all(|entry| Arc::ptr_eq(&entry.lines, &lines))
        );
        // Reusing a runtime ID with a different unique ID must retire the old identity.
        for (identity, cached) in &mut cache.entries {
            cached.queried = *identity == (1, 1);
        }
        cache.retain_queried();
        assert_eq!(cache.retention_checks, MAX_PRESENTED_NAMETAGS);
        assert_eq!(cache.entries.keys().copied().collect::<Vec<_>>(), [(1, 1)]);
        cache.entries.get_mut(&(1, 1)).unwrap().queried = false;
        cache.retain_queried();
        assert_eq!(cache.retention_checks, 1);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.entries.capacity(), 0);
    }
}
