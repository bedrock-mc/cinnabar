//! The bed screen's state: last frame's targets and the tracked pointer for hover.

use ui::UiPoint;

use super::super::engine::{
    rect,
    screen::{PaintError, PaintFrame},
};
use super::{BedHit, BedScreen, bedtime, paint::Canvas, theme};

/// What one bed paint reads.
pub(in super::super) struct BedInput {
    /// Milliseconds since the player lay down; `None` while awake.
    pub(in super::super) asleep_for: Option<u64>,
    pub(in super::super) remote_players: bool,
    pub(in super::super) thunderstorm: bool,
    pub(in super::super) status: Option<crate::ui_runtime::SleepStatus>,
}

impl BedScreen {
    /// Draws the bed screen while the player sleeps; its targets land in window-logical px.
    pub(in super::super) fn paint(
        &mut self,
        input: BedInput,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let Some(elapsed) = input.asleep_for else {
            self.hits.clear();
            return Ok(());
        };
        let hovered = self.pointer.and_then(|point| {
            self.hits
                .iter()
                .find_map(|(hit, bounds)| bounds.contains(point).then_some(*hit))
        });
        let state = bedtime::Bedtime {
            elapsed,
            remote_players: input.remote_players,
            thunderstorm: input.thunderstorm,
            status: input.status,
            hovered,
            pressed: None,
        };
        let mut canvas = Canvas::new(
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            frame.metrics,
            frame.solid_page,
            None,
        );
        canvas.bundle = theme::Bundle::Gameplay;
        let hits = bedtime::draw(&mut canvas, &state, frame.content)?;
        let [left, top] = [frame.safe_area.left(), frame.safe_area.top()];
        self.hits = hits
            .into_iter()
            .filter_map(|(hit, bounds)| {
                let min = bounds.min();
                let max = bounds.max();
                rect(min.x() + left, min.y() + top, max.x() + left, max.y() + top)
                    .ok()
                    .map(|bounds| (hit, bounds))
            })
            .collect();
        Ok(())
    }

    /// What a press at the window-logical `position` hits.
    pub(in super::super) fn hit(&self, position: UiPoint) -> Option<BedHit> {
        self.hits
            .iter()
            .find_map(|(hit, bounds)| bounds.contains(position).then_some(*hit))
    }

    /// Tracks the pointer for next frame's hover state.
    pub(in super::super) fn set_pointer(&mut self, position: Option<UiPoint>) {
        self.pointer = position;
    }

    /// The last frame's targets.
    #[cfg(test)]
    pub(in super::super) fn hits(&self) -> &[(BedHit, ui::UiRect)] {
        &self.hits
    }
}
