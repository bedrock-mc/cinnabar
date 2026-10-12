//! Paints the active End credits and answers their completion and skip queries.

use ui::UiNode;

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::{PaintFrame, Screen};
use ui_engine::text_metrics::TextMetrics;
use ui_screens::credits_screen::CreditsInput;

impl UiPresentationRuntime {
    pub fn credits_finished(&self, session: u64, sequence: u64) -> bool {
        self.screens.credits.finished((session, sequence))
    }

    pub fn credits_skip_contains(&self, session: u64, sequence: u64, point: [f32; 2]) -> bool {
        self.screens.credits.shows((session, sequence))
            && self.screens.credits_hits.iter().any(|(_, rect)| {
                point[0] >= rect.min().x()
                    && point[0] <= rect.max().x()
                    && point[1] >= rect.min().y()
                    && point[1] <= rect.max().y()
            })
    }

    /// Paints the active credits; `false` when none are active or the engine drew nothing.
    pub(in super::super) fn append_credits_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now: u64,
    ) -> Result<bool, UiPresentationError> {
        let Some(active) = runtime.credits().active() else {
            return Ok(false);
        };
        let Some(engine) = self.engine.as_deref() else {
            return Ok(false);
        };
        let translate = |key: &str| runtime.translation(key);
        let input = CreditsInput {
            engine,
            pack: self
                .engine_pack
                .as_deref()
                .and_then(|pack| pack.view.as_ref()),
            identity: (runtime.session_id(), active.sequence),
            player_name: runtime.credits_player_name(),
            scroll_pixels: active.scroll_pixels(),
            skip_visible: active.skip_visible(now),
            cover_alpha: active.cover_alpha(now),
            clocks: &self.scene_clock,
        };
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        // New credits start without the previous run's skip regions, even if painting fails.
        if !self.screens.credits.shows(input.identity) {
            self.screens.credits_hits.clear();
        }
        let output = self.screens.credits.paint(input, &mut frame)?;
        self.screens.credits_hits = output.hits;
        Ok(output.frame.is_some())
    }
}
