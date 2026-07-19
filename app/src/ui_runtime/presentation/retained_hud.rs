use std::sync::Arc;

use assets::RuntimeFontCatalog;
use ui::{
    BossBarStore, BossColor, BossOverlay, DisplaySlot, ScoreOwner, ScoreboardStore,
    TextLayoutCache, TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiScale, UiVisual,
};

use super::{UiPresentationError, bounded_visible_text, rect};

// These bounds keep the first retained renderer deterministic and allocation-bounded. They are
// provisional presentation values, not protocol-1001 vanilla evidence. Geometry, textures,
// opacity, and palette remain subject to the pinned native Phase 5 visual acceptance gate.
pub(super) const SCOREBOARD_WIDTH: f32 = 174.0;
pub(super) const SCOREBOARD_TEXT_HEIGHT: f32 = 10.0;
pub(super) const SCOREBOARD_TITLE_WIDTH: f32 = 170.0;
pub(super) const SCOREBOARD_NAME_WIDTH: f32 = 100.0;
pub(super) const MAX_PRESENTED_SCOREBOARD_ROWS: usize = 15;

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
    pub(super) notch_x: Vec<f32>,
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
            let segments = overlay_segments(bar.style.overlay);
            let notch_x = (1..segments)
                .map(|segment| left + BOSS_PANEL_WIDTH * segment as f32 / segments as f32)
                .collect();
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
                notch_x,
            }
        })
        .collect()
}

const fn overlay_segments(overlay: BossOverlay) -> usize {
    match overlay {
        BossOverlay::Progress => 1,
        BossOverlay::Notched6 => 6,
        BossOverlay::Notched10 => 10,
        BossOverlay::Notched12 => 12,
        BossOverlay::Notched20 => 20,
    }
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
) -> Result<(), UiPresentationError> {
    let height = SCOREBOARD_TEXT_HEIGHT * (scoreboard.rows.len().saturating_add(1)) as f32;
    if viewport_width < SCOREBOARD_WIDTH || viewport_height < height || height <= 0.0 {
        return Ok(());
    }
    let left = viewport_width - SCOREBOARD_WIDTH;
    let top = (viewport_height - height) * 0.5;
    let right = viewport_width;
    let bottom = top + height;
    nodes.push(
        UiNode::new(take_node_id(next_id), None, rect(left, top, right, bottom)?).with_visual(
            UiVisual::Solid {
                texture_page: solid_texture_page,
                color: [0, 0, 0, 128],
            },
        ),
    );
    nodes.push(
        UiNode::new(
            take_node_id(next_id),
            None,
            rect(left + 2.0, top, right - 2.0, top + SCOREBOARD_TEXT_HEIGHT)?,
        )
        .with_visual(UiVisual::Solid {
            texture_page: solid_texture_page,
            color: [0, 0, 0, 160],
        }),
    );
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
    let title_left = left + ((SCOREBOARD_WIDTH - title_width) * 0.5).max(2.0);
    append_clipped_text_node(
        nodes,
        next_id,
        [left + 2.0, top, right - 2.0, top + SCOREBOARD_TEXT_HEIGHT],
        [
            title_left,
            top,
            (title_left + title_width).min(right - 2.0),
            top + SCOREBOARD_TEXT_HEIGHT,
        ],
        title,
        [255; 4],
    )?;
    for (index, row) in scoreboard.rows.iter().enumerate() {
        let row_top = top + SCOREBOARD_TEXT_HEIGHT * (index.saturating_add(1)) as f32;
        let row_bottom = row_top + SCOREBOARD_TEXT_HEIGHT;
        let label = layouts
            .layout(TextLayoutRequest {
                text: bounded_visible_text(&row.label),
                style: TextStyle::default(),
                width_64: (SCOREBOARD_NAME_WIDTH * 64.0) as u32,
                scale: UiScale::default(),
                font,
            })
            .map_err(UiPresentationError::Text)?;
        let label_bounds = [
            left + 2.0,
            row_top,
            left + 2.0 + SCOREBOARD_NAME_WIDTH,
            row_bottom,
        ];
        append_clipped_text_node(nodes, next_id, label_bounds, label_bounds, label, [255; 4])?;
        let score = layouts
            .layout(TextLayoutRequest {
                text: &row.score,
                style: TextStyle::default(),
                width_64: (SCOREBOARD_WIDTH * 64.0) as u32,
                scale: UiScale::default(),
                font,
            })
            .map_err(UiPresentationError::Text)?;
        let score_width = score.size_64()[0] as f32 / 64.0;
        let score_left = (right - 2.0 - score_width).max(left + SCOREBOARD_NAME_WIDTH + 12.0);
        append_clipped_text_node(
            nodes,
            next_id,
            [
                left + SCOREBOARD_NAME_WIDTH + 12.0,
                row_top,
                right - 2.0,
                row_bottom,
            ],
            [score_left, row_top, right - 2.0, row_bottom],
            score,
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
    solid_texture_page: u16,
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
        nodes.push(solid_node(
            take_node_id(next_id),
            bar.progress,
            solid_texture_page,
            [32, 32, 32, 255],
        )?);
        if bar.fill[2] > bar.fill[0] {
            nodes.push(solid_node(
                take_node_id(next_id),
                bar.fill,
                solid_texture_page,
                bar.color,
            )?);
        }
        for notch_x in bar.notch_x {
            nodes.push(solid_node(
                take_node_id(next_id),
                [
                    notch_x - 0.5,
                    bar.progress[1],
                    notch_x + 0.5,
                    bar.progress[1] + BOSS_PROGRESS_HEIGHT,
                ],
                solid_texture_page,
                [16, 16, 16, 255],
            )?);
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
