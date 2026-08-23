use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{HudTextureRole, RuntimeFontCatalog};
use ui::{
    DisplaySlot, ScoreOwner, ScoreRenderType, ScoreboardStore, TextLayoutCache, TextShadow, UiNode,
    UiNodeId, UiVisual,
};

use super::{
    HudSprite, HudTexturePages, TextMetrics, UiPresentationError, UiPresentationRuntime,
    bounded_visible_text, rect,
};

// Exact classic-profile contracts from the hash-pinned official sample ui/scoreboards.json.
pub(super) const SCOREBOARD_MAIN_HORIZONTAL_EXPANSION: f32 = 4.0;
pub(super) const SCOREBOARD_TEXT_HEIGHT: f32 = 10.0;
pub(super) const SCOREBOARD_TITLE_BACKGROUND_HEIGHT: f32 = 9.0;
pub(super) const SCOREBOARD_TITLE_WIDTH: f32 = 170.0;
pub(super) const SCOREBOARD_NAME_WIDTH: f32 = 100.0;
pub(super) const SCOREBOARD_LIST_OFFSET: f32 = 10.0;
pub(super) const PLAYER_LIST_TOP_OFFSET: f32 = 10.0;
pub(super) const SCOREBOARD_HORIZONTAL_PADDING: f32 = 10.0;
pub(super) const MAX_PRESENTED_SCOREBOARD_ROWS: usize = 15;
pub(super) const MAX_PRESENTED_PLAYER_LIST_ROWS: usize = protocol::MAX_PLAYER_LIST_RECORDS;
pub(super) const MAX_PRESENTED_BELOW_NAME_ROWS: usize = ui::MAX_SCORES;
/// Provisional hearts-row placeholder cap: one ten-heart row, the Java
/// sidebar's single-row capacity, pending a version-matched native witness
/// for hearts-style criteria. Overflowing scores present only this bound.
pub(super) const MAX_PRESENTED_SCOREBOARD_HEARTS: u8 = 10;
/// Provisional hearts-row spacing: eight GUI pixels per heart with one-pixel
/// overlap, pending independent version-matched native measurement like the
/// row cap above.
const SCOREBOARD_HEART_ADVANCE: f32 = 8.0;
const NAMEPLATE_LINE_HEIGHT: f32 = 9.0;
const NAMEPLATE_VERTICAL_GAP: f32 = 1.0;
const NAMEPLATE_HORIZONTAL_PADDING: f32 = 2.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScoreboardPresentationScope {
    HudSidebar,
    #[allow(
        dead_code,
        reason = "the player-list projection must not render on the always-on HUD surface"
    )]
    PlayerList,
    ActorNameplate,
}

impl ScoreboardPresentationScope {
    const fn slot(self) -> DisplaySlot {
        match self {
            Self::HudSidebar => DisplaySlot::Sidebar,
            Self::PlayerList => DisplaySlot::List,
            Self::ActorNameplate => DisplaySlot::BelowName,
        }
    }

    const fn maximum_rows(self) -> usize {
        match self {
            Self::HudSidebar => MAX_PRESENTED_SCOREBOARD_ROWS,
            Self::PlayerList => MAX_PRESENTED_PLAYER_LIST_ROWS,
            Self::ActorNameplate => MAX_PRESENTED_BELOW_NAME_ROWS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PresentedScoreValue {
    Text(Arc<str>),
    Hearts { full_hearts: u8, half_heart: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboardRow {
    pub(super) label: Arc<str>,
    pub(super) value: PresentedScoreValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboard {
    pub(super) scope: ScoreboardPresentationScope,
    pub(super) title: Arc<str>,
    pub(super) rows: Vec<PresentedScoreboardRow>,
}

#[allow(
    dead_code,
    reason = "the actor-nameplate surface consumes this after native geometry is measured"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedBelowNameRow {
    pub(super) owner: ScoreOwner,
    pub(super) score: i32,
}

#[allow(
    dead_code,
    reason = "the actor-nameplate surface consumes this after native geometry is measured"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedBelowNameScores {
    pub(super) scope: ScoreboardPresentationScope,
    pub(super) objective_display_name: Arc<str>,
    pub(super) rows: Vec<PresentedBelowNameRow>,
}

/// A projected world-space actor nameplate, prepared on the main thread from
/// authoritative actor and scoreboard state before the retained UI tree is
/// built. Coordinates are in the safe content viewport's logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BelowNameAnchor {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) name: Arc<str>,
    pub(super) score: i32,
    pub(super) objective: Arc<str>,
}

#[derive(Debug, Default)]
pub(super) struct PresentedScoreboardCache {
    revision: Option<u64>,
    owner_names_revision: u64,
    projection: Option<PresentedScoreboard>,
}

#[derive(Debug, Default)]
pub(super) struct ScoreboardOwnerNameAuthority {
    revision: u64,
    names: BTreeMap<i64, Arc<str>>,
}

// Java Edition scoreboard sidebar background opacities, adopted for the Hybrid HUD.
//
// Bedrock exposes `#objective_background_opacity` / `#scoreboard_objective_background_opacity` as
// runtime engine bindings with no static value in the hash-pinned pack, so there is no Bedrock
// authority to bind here. Java Edition draws the sidebar body with `getBackgroundColor(0.3)` and
// the title with `getBackgroundColor(0.4)`; converting those normalized channels to byte alpha
// gives 77 and 102. Recorded as a Hybrid HUD deviation in plan.md.
const JAVA_SCOREBOARD_BODY_ALPHA: u8 = 77;
const JAVA_SCOREBOARD_TITLE_ALPHA: u8 = 102;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScoreboardOpacityAuthority {
    body: u8,
    title: u8,
}

impl ScoreboardOpacityAuthority {
    #[must_use]
    const fn from_alpha_bytes(body: u8, title: u8) -> Self {
        Self { body, title }
    }

    #[must_use]
    const fn java_edition_style() -> Self {
        Self::from_alpha_bytes(JAVA_SCOREBOARD_BODY_ALPHA, JAVA_SCOREBOARD_TITLE_ALPHA)
    }
}

impl UiPresentationRuntime {
    /// Enables the scoreboard sidebar using the Java Edition background opacities.
    ///
    /// The sidebar still renders only when the server publishes a sidebar objective; this just
    /// binds the background alpha the fail-closed gate requires.
    pub(crate) fn enable_scoreboard_background(&mut self) {
        self.scoreboard_opacity = Some(ScoreboardOpacityAuthority::java_edition_style());
    }

    #[cfg(test)]
    pub(crate) fn set_native_scoreboard_opacity(&mut self, body: u8, title: u8) {
        self.scoreboard_opacity = Some(ScoreboardOpacityAuthority::from_alpha_bytes(body, title));
    }

    pub(crate) fn set_scoreboard_owner_names(
        &mut self,
        names: impl IntoIterator<Item = (i64, Arc<str>)>,
    ) {
        self.scoreboard_owner_names.replace(names);
    }

    pub(crate) fn refresh_scoreboard_owner_names(
        &mut self,
        store: &ScoreboardStore,
        stream: Option<&client_world::WorldStream>,
    ) {
        self.set_scoreboard_owner_names(required_sidebar_owner_ids(store).into_iter().filter_map(
            |unique_id| {
                stream?
                    .actor_display_name(unique_id)
                    .map(|name| (unique_id, name))
            },
        ));
    }
}

impl PresentedScoreboardCache {
    pub(super) fn refresh(
        &mut self,
        store: &ScoreboardStore,
        owner_names: &ScoreboardOwnerNameAuthority,
    ) -> Option<&PresentedScoreboard> {
        let revision = store.revision();
        if self.revision != Some(revision) || self.owner_names_revision != owner_names.revision {
            self.projection = project_scoreboard_for_scope(
                store,
                ScoreboardPresentationScope::HudSidebar,
                |owner| owner_names.resolve(owner),
            );
            self.revision = Some(revision);
            self.owner_names_revision = owner_names.revision;
        }
        self.projection.as_ref()
    }
}

impl ScoreboardOwnerNameAuthority {
    pub(super) fn replace(&mut self, names: impl IntoIterator<Item = (i64, Arc<str>)>) {
        let next = names
            .into_iter()
            .filter(|(_, name)| !name.is_empty())
            .collect::<BTreeMap<_, _>>();
        if self.names != next {
            self.names = next;
            self.revision = self.revision.saturating_add(1);
        }
    }

    fn resolve(&self, owner: &ScoreOwner) -> Option<Arc<str>> {
        match owner {
            ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
                self.names.get(unique_id).cloned()
            }
            ScoreOwner::FakePlayer(name) => Some(Arc::clone(name)),
            ScoreOwner::None => None,
        }
    }
}

/// Converts one authoritative score into its presented value under the
/// objective's render type. Hearts values are a bounded provisional
/// placeholder — the score reads as half-hearts capped to one ten-heart row —
/// because no in-repo authority fixes the native hearts-style presentation.
fn presented_score_value(render_type: ScoreRenderType, score: i32) -> PresentedScoreValue {
    match render_type {
        ScoreRenderType::Integer => PresentedScoreValue::Text(Arc::from(score.to_string())),
        ScoreRenderType::Hearts => {
            let halves = score.clamp(0, i32::from(MAX_PRESENTED_SCOREBOARD_HEARTS) * 2);
            PresentedScoreValue::Hearts {
                full_hearts: (halves / 2) as u8,
                half_heart: halves % 2 == 1,
            }
        }
    }
}

/// Bounded fallback for protocol owners no name authority can answer
/// (unloaded players, XUID-keyed scores): their raw retained numeric identity
/// stays visible instead of the row disappearing. Provisional until an exact
/// owner-name authority covers those owners.
fn fallback_owner_label(owner: &ScoreOwner) -> Arc<str> {
    match owner {
        ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
            Arc::from(unique_id.to_string())
        }
        ScoreOwner::FakePlayer(_) | ScoreOwner::None => Arc::from(""),
    }
}

pub(super) fn project_scoreboard_for_scope(
    store: &ScoreboardStore,
    scope: ScoreboardPresentationScope,
    mut resolve_protocol_owner: impl FnMut(&ScoreOwner) -> Option<Arc<str>>,
) -> Option<PresentedScoreboard> {
    if scope == ScoreboardPresentationScope::ActorNameplate {
        return None;
    }
    let projection = store.projection_bounded(scope.slot(), scope.maximum_rows(), |owner| {
        !matches!(owner, ScoreOwner::None)
    })?;
    let rows = projection
        .rows
        .into_iter()
        .filter_map(|row| {
            let label = match &row.owner {
                ScoreOwner::FakePlayer(label) => Arc::clone(label),
                ScoreOwner::Player(_) | ScoreOwner::Entity(_) => resolve_protocol_owner(&row.owner)
                    .unwrap_or_else(|| fallback_owner_label(&row.owner)),
                ScoreOwner::None => return None,
            };
            Some(PresentedScoreboardRow {
                label,
                value: presented_score_value(projection.render_type, row.score),
            })
        })
        .collect();
    Some(PresentedScoreboard {
        scope,
        title: projection.display_name,
        rows,
    })
}

pub(super) fn required_sidebar_owner_ids(store: &ScoreboardStore) -> Vec<i64> {
    store
        .projection_bounded(
            DisplaySlot::Sidebar,
            MAX_PRESENTED_SCOREBOARD_ROWS,
            |owner| matches!(owner, ScoreOwner::Player(_) | ScoreOwner::Entity(_)),
        )
        .map(|projection| {
            projection
                .rows
                .into_iter()
                .filter_map(|row| match row.owner {
                    ScoreOwner::Player(unique_id) | ScoreOwner::Entity(unique_id) => {
                        Some(unique_id)
                    }
                    ScoreOwner::FakePlayer(_) | ScoreOwner::None => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

#[allow(
    dead_code,
    reason = "below-name objectives fail closed until the actor-nameplate surface has measured native geometry"
)]
pub(super) fn project_below_name_scores(
    store: &ScoreboardStore,
) -> Option<PresentedBelowNameScores> {
    let projection = store.projection_bounded(
        DisplaySlot::BelowName,
        MAX_PRESENTED_BELOW_NAME_ROWS,
        |owner| matches!(owner, ScoreOwner::Player(_) | ScoreOwner::Entity(_)),
    )?;
    Some(PresentedBelowNameScores {
        scope: ScoreboardPresentationScope::ActorNameplate,
        objective_display_name: projection.display_name,
        rows: projection
            .rows
            .into_iter()
            .map(|row| PresentedBelowNameRow {
                owner: row.owner,
                score: row.score,
            })
            .collect(),
    })
}

struct PreparedScoreboardRow {
    label: Arc<ui::TextLayout>,
    label_width: f32,
    cell: PreparedScoreCell,
}

enum PreparedScoreCell {
    Text {
        layout: Arc<ui::TextLayout>,
        width: f32,
    },
    Hearts {
        texture_page: u16,
        sprites: Vec<HudSprite>,
        width: f32,
    },
}

impl PreparedScoreCell {
    fn width(&self) -> f32 {
        match self {
            Self::Text { width, .. } | Self::Hearts { width, .. } => *width,
        }
    }
}

fn prepare_score_cell(
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    value: &PresentedScoreValue,
    hud_textures: Option<&HudTexturePages>,
) -> Result<PreparedScoreCell, UiPresentationError> {
    match value {
        PresentedScoreValue::Text(text) => {
            let layout = layouts
                .layout(metrics.request(
                    bounded_visible_text(text),
                    (SCOREBOARD_TITLE_WIDTH * 64.0) as u32,
                    font,
                ))
                .map_err(UiPresentationError::Text)?;
            let width = layout.size_64()[0] as f32 / 64.0;
            Ok(PreparedScoreCell::Text { layout, width })
        }
        PresentedScoreValue::Hearts {
            full_hearts,
            half_heart,
        } => {
            // Without the required HUD carrier a hearts cell degrades honestly
            // to an empty zero-width row (no fabricated sprites); production
            // startup fails closed before this path can render.
            let Some(textures) = hud_textures else {
                return Ok(PreparedScoreCell::Hearts {
                    texture_page: 0,
                    sprites: Vec::new(),
                    width: 0.0,
                });
            };
            let full = textures.sprite(HudTextureRole::HeartFull);
            let half = textures.sprite(HudTextureRole::HeartHalf);
            let mut sprites =
                Vec::with_capacity(usize::from(*full_hearts) + usize::from(*half_heart));
            sprites.extend((0..*full_hearts).map(|_| full));
            if *half_heart {
                sprites.push(half);
            }
            let width = if sprites.is_empty() {
                0.0
            } else {
                SCOREBOARD_HEART_ADVANCE * (sprites.len() - 1) as f32
                    + f32::from(sprites[0].size[0])
            };
            Ok(PreparedScoreCell::Hearts {
                texture_page: textures.page,
                sprites,
                width,
            })
        }
    }
}

/// Tab player-list overlay: every known player-list username on its own
/// row, centered under the top edge over a translucent backdrop, with the
/// list-objective score right-aligned in yellow. Shown only while the
/// player-list action is held.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_player_list_nodes(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_texture_page: u16,
    viewport_width: f32,
    viewport_height: f32,
    players: &[(Arc<str>, Option<i32>)],
) -> Result<(), UiPresentationError> {
    if players.is_empty() {
        return Ok(());
    }
    struct PreparedPlayerRow {
        name: Arc<ui::TextLayout>,
        name_width: f32,
        score: Option<(Arc<ui::TextLayout>, f32)>,
    }
    let mut content_width = 0.0f32;
    let mut rows = Vec::with_capacity(players.len().min(MAX_PRESENTED_PLAYER_LIST_ROWS));
    for (name, score) in players.iter().take(MAX_PRESENTED_PLAYER_LIST_ROWS) {
        let name_layout = layouts
            .layout(metrics.request(
                bounded_visible_text(name),
                (SCOREBOARD_NAME_WIDTH * 64.0) as u32,
                font,
            ))
            .map_err(UiPresentationError::Text)?;
        let name_width = name_layout.size_64()[0] as f32 / 64.0;
        let score = score
            .map(|score| {
                layouts
                    .layout(metrics.request(
                        &score.to_string(),
                        (SCOREBOARD_TITLE_WIDTH * 64.0) as u32,
                        font,
                    ))
                    .map(|layout| {
                        let width = layout.size_64()[0] as f32 / 64.0;
                        (layout, width)
                    })
            })
            .transpose()
            .map_err(UiPresentationError::Text)?;
        let score_width = score.as_ref().map_or(0.0, |(_, width)| *width);
        content_width = content_width.max(name_width + SCOREBOARD_HORIZONTAL_PADDING + score_width);
        rows.push(PreparedPlayerRow {
            name: name_layout,
            name_width,
            score,
        });
    }
    let width = content_width + SCOREBOARD_HORIZONTAL_PADDING;
    let height = SCOREBOARD_TEXT_HEIGHT * rows.len() as f32 + 4.0;
    if width <= 0.0 || viewport_width < width || viewport_height < height {
        return Ok(());
    }
    let left = (viewport_width - width) * 0.5;
    let top = PLAYER_LIST_TOP_OFFSET;
    let right = left + width;
    nodes.push(solid_node(
        take_node_id(next_id),
        [left, top, right, top + height],
        solid_texture_page,
        [0, 0, 0, 120],
    )?);
    for (index, row) in rows.into_iter().enumerate() {
        let row_top = top + 2.0 + SCOREBOARD_TEXT_HEIGHT * index as f32;
        let row_bottom = row_top + SCOREBOARD_TEXT_HEIGHT;
        append_clipped_text_node(
            nodes,
            next_id,
            [left + 2.0, row_top, right - 2.0, row_bottom],
            [left + 2.0, row_top, left + 2.0 + row.name_width, row_bottom],
            row.name,
            [255; 4],
            metrics.shadow(),
        )?;
        if let Some((score, score_width)) = row.score {
            append_clipped_text_node(
                nodes,
                next_id,
                [left + 2.0, row_top, right - 2.0, row_bottom],
                [right - 2.0 - score_width, row_top, right - 2.0, row_bottom],
                score,
                [255, 255, 85, 255],
                metrics.shadow(),
            )?;
        }
    }
    Ok(())
}

/// Presents the Java-style two-line actor nameplate: the actor name above the
/// head and the below-name objective value immediately beneath it. The world
/// projection is performed by the app because only it owns the live camera and
/// actor stream; this function remains a pure retained-tree append.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_below_name_nodes(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_texture_page: u16,
    viewport_width: f32,
    viewport_height: f32,
    anchors: &[BelowNameAnchor],
) -> Result<(), UiPresentationError> {
    for anchor in anchors.iter().take(MAX_PRESENTED_BELOW_NAME_ROWS) {
        let name = layouts
            .layout(metrics.request(bounded_visible_text(&anchor.name), 160 * 64, font))
            .map_err(UiPresentationError::Text)?;
        let score_text = format!(
            "{} {}",
            anchor.score,
            bounded_visible_text(&anchor.objective)
        );
        let score = layouts
            .layout(metrics.request(&score_text, 160 * 64, font))
            .map_err(UiPresentationError::Text)?;
        let name_width = name.size_64()[0] as f32 / 64.0;
        let score_width = score.size_64()[0] as f32 / 64.0;
        let width =
            (name_width.max(score_width) + NAMEPLATE_HORIZONTAL_PADDING * 2.0).min(viewport_width);
        let height = NAMEPLATE_LINE_HEIGHT * 2.0 + NAMEPLATE_VERTICAL_GAP;
        let left = (anchor.x - width * 0.5).clamp(0.0, (viewport_width - width).max(0.0));
        let top = (anchor.y - height).clamp(0.0, (viewport_height - height).max(0.0));
        let right = left + width;
        nodes.push(solid_node(
            take_node_id(next_id),
            [left, top, right, top + height],
            solid_texture_page,
            [0, 0, 0, 96],
        )?);
        append_clipped_text_node(
            nodes,
            next_id,
            [left, top, right, top + NAMEPLATE_LINE_HEIGHT],
            [
                anchor.x - name_width * 0.5,
                top,
                anchor.x + name_width * 0.5,
                top + NAMEPLATE_LINE_HEIGHT,
            ],
            name,
            [255; 4],
            metrics.shadow(),
        )?;
        let score_top = top + NAMEPLATE_LINE_HEIGHT + NAMEPLATE_VERTICAL_GAP;
        append_clipped_text_node(
            nodes,
            next_id,
            [left, score_top, right, score_top + NAMEPLATE_LINE_HEIGHT],
            [
                anchor.x - score_width * 0.5,
                score_top,
                anchor.x + score_width * 0.5,
                score_top + NAMEPLATE_LINE_HEIGHT,
            ],
            score,
            [255, 255, 85, 255],
            metrics.shadow(),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_scoreboard_nodes(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_texture_page: u16,
    viewport_width: f32,
    viewport_height: f32,
    scoreboard: &PresentedScoreboard,
    opacity: ScoreboardOpacityAuthority,
    hud_textures: Option<&HudTexturePages>,
) -> Result<(), UiPresentationError> {
    let title = layouts
        .layout(metrics.request(
            bounded_visible_text(&scoreboard.title),
            (SCOREBOARD_TITLE_WIDTH * 64.0) as u32,
            font,
        ))
        .map_err(UiPresentationError::Text)?;
    let title_width = title.size_64()[0] as f32 / 64.0;
    let mut content_width = title_width;
    let mut rows = Vec::with_capacity(scoreboard.rows.len());
    for row in &scoreboard.rows {
        let label = layouts
            .layout(metrics.request(
                bounded_visible_text(&row.label),
                (SCOREBOARD_NAME_WIDTH * 64.0) as u32,
                font,
            ))
            .map_err(UiPresentationError::Text)?;
        let cell = prepare_score_cell(layouts, font, metrics, &row.value, hud_textures)?;
        let label_width = label.size_64()[0] as f32 / 64.0;
        content_width =
            content_width.max(label_width + SCOREBOARD_HORIZONTAL_PADDING + cell.width());
        rows.push(PreparedScoreboardRow {
            label,
            label_width,
            cell,
        });
    }
    let width = content_width + SCOREBOARD_MAIN_HORIZONTAL_EXPANSION;
    let height = SCOREBOARD_LIST_OFFSET + SCOREBOARD_TEXT_HEIGHT * rows.len() as f32;
    if width <= 0.0 || viewport_width < width || viewport_height < height {
        return Ok(());
    }
    let left = viewport_width - width;
    let top = (viewport_height - height) * 0.5;
    let right = viewport_width;
    nodes.push(solid_node(
        take_node_id(next_id),
        [left, top, right, top + height],
        solid_texture_page,
        [0, 0, 0, opacity.body],
    )?);
    nodes.push(solid_node(
        take_node_id(next_id),
        [left, top, right, top + SCOREBOARD_TITLE_BACKGROUND_HEIGHT],
        solid_texture_page,
        [0, 0, 0, opacity.title],
    )?);
    let title_left = left + (width - title_width) * 0.5;
    append_clipped_text_node(
        nodes,
        next_id,
        [left, top, right, top + SCOREBOARD_TEXT_HEIGHT],
        [
            title_left,
            top,
            title_left + title_width,
            top + SCOREBOARD_TEXT_HEIGHT,
        ],
        title,
        [255; 4],
        metrics.shadow(),
    )?;
    for (index, row) in rows.into_iter().enumerate() {
        let row_top = top + SCOREBOARD_LIST_OFFSET + SCOREBOARD_TEXT_HEIGHT * index as f32;
        let row_bottom = row_top + SCOREBOARD_TEXT_HEIGHT;
        append_clipped_text_node(
            nodes,
            next_id,
            [left + 2.0, row_top, right - 2.0, row_bottom],
            [
                left + 2.0,
                row_top,
                left + 2.0 + row.label_width,
                row_bottom,
            ],
            row.label,
            [255; 4],
            metrics.shadow(),
        )?;
        match row.cell {
            PreparedScoreCell::Text { layout, width } => {
                append_clipped_text_node(
                    nodes,
                    next_id,
                    [left + 2.0, row_top, right - 2.0, row_bottom],
                    [right - 2.0 - width, row_top, right - 2.0, row_bottom],
                    layout,
                    [255, 0, 0, 255],
                    metrics.shadow(),
                )?;
            }
            PreparedScoreCell::Hearts {
                texture_page,
                sprites,
                width,
            } => {
                let mut heart_left = right - 2.0 - width;
                for sprite in sprites {
                    let top_offset = (SCOREBOARD_TEXT_HEIGHT - f32::from(sprite.size[1])) * 0.5;
                    nodes.push(
                        UiNode::new(
                            take_node_id(next_id),
                            None,
                            rect(
                                heart_left,
                                row_top + top_offset,
                                heart_left + f32::from(sprite.size[0]),
                                row_top + top_offset + f32::from(sprite.size[1]),
                            )?,
                        )
                        .with_visual(UiVisual::Sprite {
                            texture_page,
                            uv: sprite.uv,
                            color: [255; 4],
                        }),
                    );
                    heart_left += SCOREBOARD_HEART_ADVANCE;
                }
            }
        }
    }
    Ok(())
}

fn solid_node(
    id: UiNodeId,
    bounds: [f32; 4],
    texture_page: u16,
    color: [u8; 4],
) -> Result<UiNode, UiPresentationError> {
    Ok(
        UiNode::new(id, None, rect(bounds[0], bounds[1], bounds[2], bounds[3])?).with_visual(
            UiVisual::Solid {
                texture_page,
                color,
            },
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn append_clipped_text_node(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    clip_bounds: [f32; 4],
    text_bounds: [f32; 4],
    layout: Arc<ui::TextLayout>,
    color: [u8; 4],
    shadow: TextShadow,
) -> Result<(), UiPresentationError> {
    let clip_id = take_node_id(next_id);
    nodes.push(
        UiNode::new(
            clip_id,
            None,
            rect(
                clip_bounds[0],
                clip_bounds[1],
                clip_bounds[2],
                clip_bounds[3],
            )?,
        )
        .with_clip_children(true),
    );
    nodes.push(
        UiNode::new(
            take_node_id(next_id),
            Some(clip_id),
            rect(
                text_bounds[0] - clip_bounds[0],
                text_bounds[1] - clip_bounds[1],
                text_bounds[2] - clip_bounds[0],
                text_bounds[3] - clip_bounds[1],
            )?,
        )
        .with_visual(UiVisual::Text {
            layout,
            color,
            shadow,
        }),
    );
    Ok(())
}

fn take_node_id(next_id: &mut u32) -> UiNodeId {
    let id = UiNodeId::new(*next_id);
    *next_id = next_id.saturating_add(1);
    id
}
