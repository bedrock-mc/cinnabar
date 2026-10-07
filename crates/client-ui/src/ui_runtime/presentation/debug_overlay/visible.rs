//! Equality and row limits for the text that can appear in the overlay.

use ui::UiScale;

use super::super::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics};
use super::{DebugLines, INSET, LINE_HEIGHT, MAX_LINES_PER_COLUMN, bounded_visible_text};

/// Matches the JSON-UI row budget, including the current content height.
pub(super) fn row_limit(root: [f64; 2]) -> usize {
    let rows = (((root[1] - 2.0 * INSET) / LINE_HEIGHT).max(0.0) + 1e-4).floor() as usize;
    rows.min(MAX_LINES_PER_COLUMN)
}

/// Changes past the displayed rows and bounded text prefix cannot change paint.
pub(super) fn same_lines(old: &DebugLines, new: &DebugLines, root: [f64; 2]) -> bool {
    let rows = row_limit(root);
    [&old.left, &old.right]
        .into_iter()
        .zip([&new.left, &new.right])
        .all(|(old, new)| {
            old.iter()
                .take(rows)
                .map(|line| bounded_visible_text(line))
                .eq(new.iter().take(rows).map(|line| bounded_visible_text(line)))
        })
}
/// Keeps font size independent of changing diagnostics by reserving the row budget.
/// Horizontal overflow is truncated; viewport height, DPI and GUI scale set the font size.
pub(in super::super) fn fitted_metrics(
    mut metrics: TextMetrics,
    content_height: f32,
) -> TextMetrics {
    let px = f64::from(metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32);
    let height = (2.0 * INSET + LINE_HEIGHT * MAX_LINES_PER_COLUMN as f64) * px;
    let factor = (f64::from(content_height) / height).clamp(0.5, 1.0) as f32;
    if let Ok(scale) = UiScale::new_display(metrics.scale.get() * factor) {
        metrics.scale = scale;
        metrics.gui_scale *= factor;
    }
    metrics
}
