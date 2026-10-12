//! Caret placement in edited text: character boundaries for a byte caret and for a press.

use ui::{TextLayoutCache, UiRect, UiScale};

use crate::{
    engine::text_paint::{UNWRAPPED_LOGICAL, width_64},
    text_metrics::TextMetrics,
};

/// `byte` clamped into `text` and back to a character boundary.
pub fn caret_byte(text: &str, byte: usize) -> usize {
    let mut at = byte.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The character boundary of `text` nearest `x`, from each prefix's drawn width.
pub fn nearest_boundary(text: &str, x: f32, mut width: impl FnMut(&str) -> f32) -> usize {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect();
    // Widths grow with the prefix: find the first boundary at or past `x`.
    let (mut low, mut high) = (0, bounds.len() - 1);
    while low < high {
        let middle = (low + high) / 2;
        if width(&text[..bounds[middle]]) < x {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low > 0 && x - width(&text[..bounds[low - 1]]) < width(&text[..bounds[low]]) - x {
        low -= 1;
    }
    bounds[low]
}

/// Where a text box draws its text, to place a pressed caret by character.
pub struct TextSpot<F> {
    pub field: F,
    /// The box's press area, window-logical px.
    pub bounds: UiRect,
    /// Where the text starts, window-logical px.
    pub left: f32,
    /// The text's scale over the frame metrics, and its named font (`None`: the default).
    pub factor: f32,
    pub font: Option<String>,
    pub letter_spacing_64: i32,
    pub metrics: TextMetrics,
}

impl<F> TextSpot<F> {
    /// `text`'s drawn width, window-logical px.
    pub fn width(
        &self,
        layouts: &mut TextLayoutCache,
        font: &assets::RuntimeFontCatalog,
        text: &str,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let font = self
            .font
            .as_deref()
            .map_or(font, |name| font.font_named(name));
        let mut request = self
            .metrics
            .request(text, width_64(UNWRAPPED_LOGICAL), font);
        if let Ok(scale) = UiScale::new_display(self.metrics.scale.get() * self.factor) {
            request.scale = scale;
        }
        request.wrap.letter_spacing_64 = self.letter_spacing_64;
        layouts
            .layout(request)
            .map_or(0.0, |layout| layout.size_64()[0] as f32 / 64.0)
    }
}
