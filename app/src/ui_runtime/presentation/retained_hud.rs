use std::sync::Arc;

use assets::{HudTextureRole, RuntimeFontCatalog};
use ui::{
    BossBarStore, BossColor, DisplaySlot, ScoreOwner, ScoreboardStore, TextLayoutCache,
    TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiScale, UiVisual,
};

use super::{HudTexturePages, UiPresentationError, bounded_visible_text, rect};

// Exact classic-profile contracts from the hash-pinned 1.26.3301.0 ui/scoreboards.json.
pub(super) const SCOREBOARD_MAIN_HORIZONTAL_EXPANSION: f32 = 4.0;
pub(super) const SCOREBOARD_TEXT_HEIGHT: f32 = 10.0;
pub(super) const SCOREBOARD_TITLE_BACKGROUND_HEIGHT: f32 = 9.0;
pub(super) const SCOREBOARD_TITLE_WIDTH: f32 = 170.0;
pub(super) const SCOREBOARD_NAME_WIDTH: f32 = 100.0;
pub(super) const SCOREBOARD_LIST_OFFSET: f32 = 10.0;
pub(super) const SCOREBOARD_HORIZONTAL_PADDING: f32 = 10.0;
pub(super) const MAX_PRESENTED_SCOREBOARD_ROWS: usize = 15;

// Exact classic-profile contracts from the hash-pinned 1.26.3301.0 ui/hud_screen.json and
// ui/ui_common.json. The bar images are carried from that same reviewed source identity.
pub(super) const BOSS_PANEL_WIDTH: f32 = 182.0;
pub(super) const BOSS_PANEL_HEIGHT: f32 = 20.0;
pub(super) const BOSS_PROGRESS_HEIGHT: f32 = 5.0;
pub(super) const BOSS_PROGRESS_TOP: f32 = 10.0;
pub(super) const BOSS_STACK_TOP: f32 = 2.0;
pub(super) const BOSS_STACK_VIEWPORT_FRACTION: f32 = 0.30;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboardRow {
    pub(super) label: Arc<str>,
    pub(super) score: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PresentedScoreboard {
    pub(super) title: Arc<str>,
    pub(super) rows: Vec<PresentedScoreboardRow>,
}

#[derive(Debug, Default)]
pub(super) struct PresentedScoreboardCache {
    revision: Option<u64>,
    projection: Option<PresentedScoreboard>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScoreboardOpacityAuthority {
    body: u8,
    title: u8,
}

impl ScoreboardOpacityAuthority {
    #[must_use]
    pub(super) const fn from_native_alpha_bytes(body: u8, title: u8) -> Self {
        Self { body, title }
    }
}

impl PresentedScoreboardCache {
    pub(super) fn refresh(&mut self, store: &ScoreboardStore) -> Option<&PresentedScoreboard> {
        let revision = store.revision();
        if self.revision != Some(revision) {
            self.projection = project_scoreboard_sidebar(store);
            self.revision = Some(revision);
        }
        self.projection.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PresentedBossBar {
    pub(super) title: Arc<str>,
    pub(super) panel: [f32; 4],
    pub(super) progress: [f32; 4],
    pub(super) fill: [f32; 4],
    pub(super) color: [u8; 4],
}

pub(super) fn project_scoreboard_sidebar(store: &ScoreboardStore) -> Option<PresentedScoreboard> {
    let projection = store.projection_bounded(
        DisplaySlot::Sidebar,
        MAX_PRESENTED_SCOREBOARD_ROWS,
        |owner| matches!(owner, ScoreOwner::FakePlayer(_)),
    )?;
    let rows = projection
        .rows
        .into_iter()
        .filter_map(|row| {
            let ScoreOwner::FakePlayer(label) = row.owner else {
                return None;
            };
            Some(PresentedScoreboardRow {
                label,
                score: row.score.to_string(),
            })
        })
        .collect();
    Some(PresentedScoreboard {
        title: projection.display_name,
        rows,
    })
}

pub(super) fn project_boss_bars(
    store: &BossBarStore,
    viewport_width: f32,
    viewport_height: f32,
) -> Vec<PresentedBossBar> {
    if !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || viewport_width < BOSS_PANEL_WIDTH
        || viewport_height <= BOSS_STACK_TOP
    {
        return Vec::new();
    }
    let available_height = (viewport_height * BOSS_STACK_VIEWPORT_FRACTION - BOSS_STACK_TOP)
        .clamp(0.0, viewport_height);
    let capacity = (available_height / BOSS_PANEL_HEIGHT).floor() as usize;
    if capacity == 0 {
        return Vec::new();
    }
    let left = (viewport_width - BOSS_PANEL_WIDTH) * 0.5;
    store
        .stacked()
        .into_iter()
        .take(capacity)
        .enumerate()
        .map(|(index, bar)| {
            let top = BOSS_STACK_TOP + index as f32 * BOSS_PANEL_HEIGHT;
            let progress_top = top + BOSS_PROGRESS_TOP;
            let health = bar.health.clamp(0.0, 1.0);
            PresentedBossBar {
                title: if bar.filtered_title.is_empty() {
                    bar.title
                } else {
                    bar.filtered_title
                },
                panel: [left, top, left + BOSS_PANEL_WIDTH, top + BOSS_PANEL_HEIGHT],
                progress: [
                    left,
                    progress_top,
                    left + BOSS_PANEL_WIDTH,
                    progress_top + BOSS_PROGRESS_HEIGHT,
                ],
                fill: [
                    left,
                    progress_top,
                    left + BOSS_PANEL_WIDTH * health,
                    progress_top + BOSS_PROGRESS_HEIGHT,
                ],
                color: boss_color(bar.style.color),
            }
        })
        .collect()
}

const fn boss_color(color: BossColor) -> [u8; 4] {
    match color {
        BossColor::Pink => [255, 85, 255, 255],
        BossColor::Blue => [85, 85, 255, 255],
        BossColor::Red => [255, 85, 85, 255],
        BossColor::Green => [85, 255, 85, 255],
        BossColor::Yellow => [255, 255, 85, 255],
        BossColor::Purple => [170, 0, 170, 255],
        BossColor::RebeccaPurple => [102, 51, 153, 255],
        BossColor::White => [255, 255, 255, 255],
    }
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

pub(super) fn append_boss_nodes(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &RuntimeFontCatalog,
    textures: &HudTexturePages,
    bars: Vec<PresentedBossBar>,
) -> Result<(), UiPresentationError> {
    for bar in bars {
        let title = layouts
            .layout(TextLayoutRequest {
                text: bounded_visible_text(&bar.title),
                style: TextStyle::default(),
                width_64: ((bar.panel[2] - bar.panel[0]) * 64.0) as u32,
                scale: UiScale::default(),
                font,
            })
            .map_err(UiPresentationError::Text)?;
        let title_width = title.size_64()[0] as f32 / 64.0;
        let title_left = bar.panel[0] + ((bar.panel[2] - bar.panel[0] - title_width) * 0.5);
        append_clipped_text_node(
            nodes,
            next_id,
            [bar.panel[0], bar.panel[1], bar.panel[2], bar.progress[1]],
            [
                title_left.max(bar.panel[0]),
                bar.panel[1],
                (title_left + title_width).min(bar.panel[2]),
                bar.progress[1],
            ],
            title,
            [255; 4],
        )?;
        let empty = textures.sprite(HudTextureRole::BossProgressEmpty);
        nodes.push(
            UiNode::new(
                take_node_id(next_id),
                None,
                rect(
                    bar.progress[0],
                    bar.progress[1],
                    bar.progress[2],
                    bar.progress[3],
                )?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: textures.page,
                uv: empty.uv,
                color: [255; 4],
            }),
        );
        if bar.fill[2] > bar.fill[0] {
            let clip_id = take_node_id(next_id);
            nodes.push(
                UiNode::new(
                    clip_id,
                    None,
                    rect(bar.fill[0], bar.fill[1], bar.fill[2], bar.fill[3])?,
                )
                .with_clip_children(true),
            );
            let filled = textures.sprite(HudTextureRole::BossProgressFilled);
            nodes.push(
                UiNode::new(
                    take_node_id(next_id),
                    Some(clip_id),
                    rect(0.0, 0.0, BOSS_PANEL_WIDTH, BOSS_PROGRESS_HEIGHT)?,
                )
                .with_visual(UiVisual::Sprite {
                    texture_page: textures.page,
                    uv: filled.uv,
                    color: bar.color,
                }),
            );
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
