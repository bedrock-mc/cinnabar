use std::ops::Range;

use super::MAX_PRESENTED_CHAT_SUGGESTIONS;

pub(super) fn visible_suggestion_range(total: usize, selected: Option<usize>) -> Range<usize> {
    let selected = selected.unwrap_or(0).min(total.saturating_sub(1));
    let end = total.min(
        selected
            .saturating_add(1)
            .max(MAX_PRESENTED_CHAT_SUGGESTIONS),
    );
    end.saturating_sub(MAX_PRESENTED_CHAT_SUGGESTIONS)..end
}
