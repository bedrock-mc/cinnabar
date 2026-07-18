use std::sync::Arc;

use ui::{BossBarStore, BossColor, BossOverlay, DisplaySlot, ScoreOwner, ScoreboardStore};

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
