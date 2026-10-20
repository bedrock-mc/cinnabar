//! Paints text feedback from the retained glyph layout without changing the panel catalog.

use super::{
    edit::Editor,
    field::Field,
    widgets::{LABEL_SCALE, Palette},
};
use std::sync::{Arc, OnceLock};
use ui_engine::engine::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
    text_paint::{UNWRAPPED_LOGICAL, painted_label_request},
};

pub(super) const RENDERER: &str = "cinnabar_native_field";
const BLINK_SECONDS: f64 = 0.5;

pub(super) struct FieldRenderer<'a> {
    editor: Option<&'a mut Editor>,
    now: f64,
    palette: Palette,
}

impl<'a> FieldRenderer<'a> {
    /// Borrows the editor for one paint, with caller-owned animation time.
    pub fn new(editor: Option<&'a mut Editor>, now: f64, palette: Palette) -> Self {
        Self {
            editor,
            now,
            palette,
        }
    }
}

impl CustomRenderer for FieldRenderer<'_> {
    /// Paints selection, text, composition underline and a blinking rectangle from one layout.
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != RENDERER {
            return Ok(false);
        }
        let Some(Editor::Field { state, .. }) = self.editor.as_deref_mut() else {
            return Ok(true);
        };
        let options = OPTIONS.get_or_init(|| json_ui::TextOptions {
            font_type: Some(ui::mod_panel::FONT_NAME.into()),
            hide_hyphen: true,
            ..Default::default()
        });
        let px = canvas.px();
        // Unwrapped geometry preserves every glyph and its exact caret stops while scrolling.
        let dest = node.dest;
        let request = painted_label_request(
            canvas.metrics(),
            &state.display,
            [
                dest[0],
                dest[1],
                dest[0] + UNWRAPPED_LOGICAL as f32,
                dest[3],
            ],
            canvas.font(),
            LABEL_SCALE as f32,
            options,
            json_ui::TextAlign::Left,
            px,
        );
        let layout = if let Some(layout) = &state.layout
            && layout.matches_request(request)
        {
            Arc::clone(layout)
        } else {
            let layout = canvas.layouts().layout(request).map_err(PaintError::Text)?;
            state.layout = Some(Arc::clone(&layout));
            state.feedback = None;
            layout
        };
        let geometry = match state.feedback {
            Some(geometry) if geometry.dest == dest && geometry.px == px => geometry,
            _ => {
                let geometry = Geometry::new(state, &layout, dest, px);
                state.feedback = Some(geometry);
                #[cfg(test)]
                {
                    state.feedback_updates += 1;
                }
                geometry
            }
        };
        let previous_clip = canvas.clip().unwrap_or(canvas.screen());
        let clip = [
            dest[0].max(previous_clip[0]),
            dest[1].max(previous_clip[1]),
            dest[2].min(previous_clip[2]),
            dest[3].min(previous_clip[3]),
        ];
        if clip[0] >= clip[2] || clip[1] >= clip[3] {
            return Ok(true);
        }
        canvas.group(clip)?;
        if let Some(selection) = geometry.selection {
            let mut color = color(self.palette.accent);
            color[3] = 90;
            canvas.solid(selection, node.alpha(color))?;
        }
        canvas.push(
            ui::UiVisual::Text {
                layout: Arc::clone(&layout),
                color: node.alpha(color(self.palette.text)),
                shadow: ui::TextShadow::None,
            },
            geometry.text,
        )?;
        if let Some(composition) = geometry.composition {
            canvas.solid(composition, node.alpha(color(self.palette.text)))?;
        }
        let start = *state.blink_start.get_or_insert(self.now);
        let shown = ((self.now - start).max(0.) / BLINK_SECONDS) as u64 % 2 == 0;
        if shown && (state.composition.is_empty() || state.composition_cursor.is_some()) {
            canvas.solid(geometry.caret, node.alpha(color(self.palette.text)))?;
        }
        canvas.group(previous_clip)?;
        Ok(true)
    }
}

/// Feedback rectangles stay unchanged until editing, font metrics or the field bounds change.
#[derive(Clone, Copy)]
pub(super) struct Geometry {
    dest: [f32; 4],
    px: f32,
    text: [f32; 4],
    caret: [f32; 4],
    selection: Option<[f32; 4]>,
    composition: Option<[f32; 4]>,
}

impl Geometry {
    /// Resolves caret stops once per dirty edit and scrolls to keep the active edge visible.
    fn new(state: &mut Field, layout: &ui::TextLayout, dest: [f32; 4], px: f32) -> Self {
        let caret = layout.first_line_caret_x_64(state.display_cursor()) as f32 / 64.;
        let width = (dest[2] - dest[0] - px).max(1.);
        let end = layout.first_line_caret_x_64(state.display.len()) as f32 / 64.;
        let max_scroll = (end.max(layout.size_64()[0] as f32 / 64.) - width).max(0.);
        state.scroll = state
            .scroll
            .min(caret)
            .max(caret - width)
            .max(0.)
            .min(max_scroll);
        let left = dest[0] - state.scroll;
        let height = (layout.size_64()[1] as f32 / 64.).max(px * 8.);
        let top = dest[1] + ((dest[3] - dest[1] - height) * 0.5).max(0.);
        state.left = left;
        state.ime_position = [left + caret, top + height];
        let selection = state.selection();
        let x = |at| left + layout.first_line_caret_x_64(at) as f32 / 64.;
        Self {
            dest,
            px,
            text: [
                left,
                top,
                left + (layout.size_64()[0] as f32 / 64.).max(1.),
                top + height,
            ],
            caret: [
                left + caret,
                top,
                left + caret + (px * 0.5).max(0.5),
                top + height,
            ],
            selection: (state.composition.is_empty() && !selection.is_empty()).then(|| {
                [
                    x(selection.start),
                    top,
                    x(selection.end).max(x(selection.start) + px * 0.5),
                    top + height,
                ]
            }),
            composition: (!state.composition.is_empty()).then(|| {
                [
                    x(selection.start),
                    top + height - px * 0.5,
                    x(selection.start + state.composition.len()).max(x(selection.start) + px * 0.5),
                    top + height,
                ]
            }),
        }
    }
}

static OPTIONS: OnceLock<json_ui::TextOptions> = OnceLock::new();

/// Converts the native palette into the canvas's byte color representation.
fn color(color: [f64; 4]) -> [u8; 4] {
    color.map(|component| (component.clamp(0., 1.) * 255.).round() as u8)
}
