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

    /// Keeps the painted caret visible, including while its position is animated.
    pub fn reveal_caret(&mut self, caret: f32, viewport: Bounds) {
        let width = viewport[2] - viewport[0];
        self.at[0] = viewport[0] - (caret - width).max(0.0);
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
