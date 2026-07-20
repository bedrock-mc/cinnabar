use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::RuntimeFontCatalog;
use ui::{
    DisplaySlot, ScoreOwner, ScoreboardStore, TextLayoutCache, TextLayoutRequest, TextStyle,
    UiNode, UiNodeId, UiScale, UiVisual,
};

use super::{UiPresentationError, UiPresentationRuntime, bounded_visible_text, rect};

// Exact classic-profile contracts from the hash-pinned official sample ui/scoreboards.json.
pub(super) const SCOREBOARD_MAIN_HORIZONTAL_EXPANSION: f32 = 4.0;
pub(super) const SCOREBOARD_TEXT_HEIGHT: f32 = 10.0;
pub(super) const SCOREBOARD_TITLE_BACKGROUND_HEIGHT: f32 = 9.0;
pub(super) const SCOREBOARD_TITLE_WIDTH: f32 = 170.0;
pub(super) const SCOREBOARD_NAME_WIDTH: f32 = 100.0;
pub(super) const SCOREBOARD_LIST_OFFSET: f32 = 10.0;
pub(super) const SCOREBOARD_HORIZONTAL_PADDING: f32 = 10.0;
pub(super) const MAX_PRESENTED_SCOREBOARD_ROWS: usize = 15;
pub(super) const MAX_PRESENTED_PLAYER_LIST_ROWS: usize = protocol::MAX_PLAYER_LIST_RECORDS;
pub(super) const MAX_PRESENTED_BELOW_NAME_ROWS: usize = ui::MAX_SCORES;

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
pub(super) struct PresentedScoreboardRow {
    pub(super) label: Arc<str>,
    pub(super) score: String,
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
                ScoreOwner::Player(_) | ScoreOwner::Entity(_) => {
                    resolve_protocol_owner(&row.owner)?
                }
                ScoreOwner::None => return None,
            };
            Some(PresentedScoreboardRow {
                label,
                score: row.score.to_string(),
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
    score: Arc<ui::TextLayout>,
    label_width: f32,
    score_width: f32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_scoreboard_nodes(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    solid_texture_page: u16,
    viewport_width: f32,
    viewport_height: f32,
    scoreboard: &PresentedScoreboard,
    opacity: ScoreboardOpacityAuthority,
) -> Result<(), UiPresentationError> {
    let title = layouts
        .layout(TextLayoutRequest {
            text: bounded_visible_text(&scoreboard.title),
            style: TextStyle::default(),
            width_64: (SCOREBOARD_TITLE_WIDTH * 64.0) as u32,
            scale: UiScale::default(),
            font,
        })
        .map_err(UiPresentationError::Text)?;
    let title_width = title.size_64()[0] as f32 / 64.0;
    let mut content_width = title_width;
    let mut rows = Vec::with_capacity(scoreboard.rows.len());
    for row in &scoreboard.rows {
        let label = layouts
            .layout(TextLayoutRequest {
                text: bounded_visible_text(&row.label),
                style: TextStyle::default(),
                width_64: (SCOREBOARD_NAME_WIDTH * 64.0) as u32,
                scale: UiScale::default(),
                font,
            })
            .map_err(UiPresentationError::Text)?;
        let score = layouts
            .layout(TextLayoutRequest {
                text: &row.score,
                style: TextStyle::default(),
                width_64: (SCOREBOARD_TITLE_WIDTH * 64.0) as u32,
                scale: UiScale::default(),
                font,
            })
            .map_err(UiPresentationError::Text)?;
        let label_width = label.size_64()[0] as f32 / 64.0;
        let score_width = score.size_64()[0] as f32 / 64.0;
        content_width =
            content_width.max(label_width + SCOREBOARD_HORIZONTAL_PADDING + score_width);
        rows.push(PreparedScoreboardRow {
            label,
            score,
            label_width,
            score_width,
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
        )?;
        append_clipped_text_node(
            nodes,
            next_id,
            [left + 2.0, row_top, right - 2.0, row_bottom],
            [
                right - 2.0 - row.score_width,
                row_top,
                right - 2.0,
                row_bottom,
            ],
            row.score,
            [255, 0, 0, 255],
        )?;
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
        .with_visual(UiVisual::Text { layout, color }),
    );
    Ok(())
}

fn take_node_id(next_id: &mut u32) -> UiNodeId {
    let id = UiNodeId::new(*next_id);
    *next_id = next_id.saturating_add(1);
    id
}
