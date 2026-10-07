#[cfg(test)]
use ui::UiRect;
use ui::{UiNode, UiPoint};

use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::{BedHit, bedtime, paint::Canvas, theme};

impl UiPresentationRuntime {
    /// Draws the OreUI bed screen while the player lies in bed.
    pub(in super::super::super) fn append_bed_screen(
        &mut self,
        runtime: &crate::ui_runtime::UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let bed = &mut self.form_presentation.bed;
        let Some(elapsed) = self.hud_frame.sleep.asleep_for(now_millis) else {
            bed.hits.clear();
            return Ok(());
        };
        let hovered = bed.pointer.and_then(|point| {
            bed.hits
                .iter()
                .find_map(|(hit, bounds)| bounds.contains(point).then_some(*hit))
        });
        let state = bedtime::Bedtime {
            elapsed,
            // The local player is on the list too.
            remote_players: runtime.known_player_names().len() > 1,
            thunderstorm: self.hud_frame.thunderstorm,
            status: runtime.sleep_status(),
            hovered,
            pressed: None,
        };
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            None,
        );
        canvas.bundle = theme::Bundle::Gameplay;
        let hits = bedtime::draw(&mut canvas, &state, size)?;
        let [left, top] = [self.safe_area.left(), self.safe_area.top()];
        self.form_presentation.bed.hits = hits
            .into_iter()
            .filter_map(|(hit, bounds)| {
                let min = bounds.min();
                let max = bounds.max();
                super::super::super::rect(
                    min.x() + left,
                    min.y() + top,
                    max.x() + left,
                    max.y() + top,
                )
                .ok()
                .map(|bounds| (hit, bounds))
            })
            .collect();
        Ok(())
    }

    /// What a press at the window-logical `position` hits on the bed screen.
    pub fn hit_test_bed(&self, position: UiPoint) -> Option<BedHit> {
        self.form_presentation
            .bed
            .hits
            .iter()
            .find_map(|(hit, bounds)| bounds.contains(position).then_some(*hit))
    }

    /// Track the pointer for next frame's hover state.
    pub fn set_bed_pointer(&mut self, position: Option<UiPoint>) {
        self.form_presentation.bed.pointer = position;
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The bed screen's hit rects from the last frame.
    pub fn bed_hits(&self) -> &[(BedHit, UiRect)] {
        &self.form_presentation.bed.hits
    }
}
