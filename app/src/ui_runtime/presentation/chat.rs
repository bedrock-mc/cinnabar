use std::ops::Range;

use ui::UiPoint;

use super::{MAX_PRESENTED_CHAT_SUGGESTIONS, UiPresentationRuntime};

impl UiPresentationRuntime {
    pub(crate) fn hit_test_chat_suggestion(
        &self,
        position: UiPoint,
        logical_size: [f32; 2],
    ) -> Option<usize> {
        let expected = self.chat_hit_logical_size?;
        if expected.map(f32::to_bits) != logical_size.map(f32::to_bits) {
            return None;
        }
        self.chat_suggestion_hits
            .iter()
            .find_map(|(index, bounds)| bounds.contains(position).then_some(*index))
    }
}

pub(super) fn visible_suggestion_range(total: usize, selected: Option<usize>) -> Range<usize> {
    let selected = selected.unwrap_or(0).min(total.saturating_sub(1));
    let end = total.min(
        selected
            .saturating_add(1)
            .max(MAX_PRESENTED_CHAT_SUGGESTIONS),
    );
    end.saturating_sub(MAX_PRESENTED_CHAT_SUGGESTIONS)..end
}
