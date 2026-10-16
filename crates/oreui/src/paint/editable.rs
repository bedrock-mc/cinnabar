//! One retained layout supplies editable text, feedback and pointer positions.

use super::{Arc, Bounds, Canvas, PaintError, Type};

pub(in super::super) struct EditableLine {
    pub layout: Arc<ui::TextLayout>,
    pub at: [f32; 2],
    pub vertical: [f32; 2],
    pub placeholder: bool,
}

impl EditableLine {
    /// Returns the source byte's pen position in the retained layout.
    pub fn caret(&self, byte: usize) -> f32 {
        if self.placeholder {
            0.0
        } else {
            self.layout.first_line_caret_x_64(byte) as f32 / 64.0
        }
    }

    /// Reveals the animated caret within the current text extent; placeholders stay anchored.
    pub fn reveal_caret(&mut self, caret: f32, viewport: Bounds) {
        let width = viewport[2] - viewport[0];
        let maximum = if self.placeholder {
            0.0
        } else {
            (self.layout.size_64()[0] as f32 / 64.0 - width).max(0.0)
        };
        self.at[0] = viewport[0] - (caret - width).clamp(0.0, maximum);
    }
}

impl Canvas<'_> {
    /// Preserves field whitespace and places the layout's ink within the field.
    pub(in super::super) fn editable_line(
        &mut self,
        value: &str,
        placeholder: &str,
        bounds: Bounds,
        style: Type,
    ) -> Result<EditableLine, PaintError> {
        let placeholder_shown = value.is_empty();
        let shown = if placeholder_shown {
            placeholder
        } else {
            value
        };
        let layout = self.layout(shown, super::MEASUREMENT_WIDTH_64, style)?;
        let y = Self::centered_line_y(&layout, bounds);
        let height = layout.size_64()[1] as f32 / 64.0;
        Ok(EditableLine {
            layout,
            at: [bounds[0], y],
            vertical: [y, y + height],
            placeholder: placeholder_shown,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{review_tests::paint, theme::BODY};

    #[test]
    fn reveal_clamps_after_deleting_from_a_scrolled_field() {
        paint(Default::default(), |canvas| {
            let viewport = [20.0, 20.0, 120.0, 60.0];
            let value = "02".repeat(40);
            let mut before = canvas.editable_line(&value, "", viewport, BODY).unwrap();
            let caret = before.caret(value.len());
            before.reveal_caret(caret, viewport);
            assert!(before.at[0] < viewport[0], "the original field is scrolled");

            let mut after = canvas.editable_line("0", "", viewport, BODY).unwrap();
            after.reveal_caret(caret, viewport);
            assert_eq!(
                after.at[0], viewport[0],
                "the remaining text is visible immediately"
            );
        });
    }

    #[test]
    fn reveal_recomputes_its_bounds_after_replacement_removal_and_resize() {
        paint(Default::default(), |canvas| {
            let original = [20.0, 20.0, 120.0, 60.0];
            let value = "02".repeat(40);
            let before = canvas.editable_line(&value, "", original, BODY).unwrap();
            let caret = before.caret(value.len());
            let full_width = before.layout.size_64()[0] as f32 / 64.0;
            let placeholder = "Long placeholder ".repeat(10);
            for (text, viewport) in [
                (&value[..value.len() - 1], original),
                ("02", original),
                (&value[..12], original),
                ("", original),
                (value.as_str(), [30.0, 20.0, 80.0, 60.0]),
                (value.as_str(), [30.0, 20.0, 30.0 + full_width + 50.0, 60.0]),
            ] {
                let mut line = canvas
                    .editable_line(text, &placeholder, viewport, BODY)
                    .unwrap();
                line.reveal_caret(caret, viewport);
                let width = line.layout.size_64()[0] as f32 / 64.0;
                if text.is_empty() || width <= viewport[2] - viewport[0] {
                    assert_eq!(
                        line.at[0], viewport[0],
                        "fitting text and placeholders stay anchored"
                    );
                } else {
                    assert!(line.at[0] <= viewport[0]);
                    assert!(
                        line.at[0] + width >= viewport[2] - 0.01,
                        "scrolling cannot expose empty space beyond the text"
                    );
                }
                line.reveal_caret(0.0, viewport);
                assert_eq!(
                    line.at[0], viewport[0],
                    "returning home resets the reveal offset"
                );
            }
        });
    }
}
