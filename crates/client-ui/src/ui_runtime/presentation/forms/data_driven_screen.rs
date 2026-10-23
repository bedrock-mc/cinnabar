//! Draws the showing data-driven form and exposes its targets to input.

use oreui::data_driven_screen::FormHit;
use ui::{UiNode, UiPoint};
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;

impl UiPresentationRuntime {
    /// Paints the showing form; a new form instance starts unscrolled.
    pub(in super::super) fn append_data_driven_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let form_id = runtime
            .data_driven_ui()
            .active()
            .map(|active| active.form_id);
        if self.screens.data_driven_form != form_id {
            self.screens.data_driven.reset();
            self.screens.data_driven_form = form_id;
        }
        let Some(form) = runtime.data_driven_form() else {
            return Ok(());
        };
        let focus = runtime.data_driven_focus_items();
        self.screens.data_driven.set_editing(focus.editing);
        self.screens
            .data_driven
            .set_open_dropdown(focus.open_dropdown);
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics: self.screens.oreui.accessibility_metrics(metrics),
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate: &|_| None,
            language: [0; 3],
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        Ok(self.screens.data_driven.paint(&form, &mut frame)?)
    }

    /// What a press at the window-logical `position` hits on the data-driven form.
    pub fn hit_test_data_driven(&self, position: UiPoint) -> Option<FormHit> {
        self.screens.data_driven.hit(position)
    }

    /// Where window-logical `x` lies along slider `item`'s thumb travel, from 0 to 1.
    pub fn data_driven_slider_fraction(&self, item: usize, x: f32) -> Option<f32> {
        self.screens.data_driven.slider_fraction(item, x)
    }

    /// Tracks the pointer and the held control for next frame's hover and press state.
    pub fn set_data_driven_pointer(&mut self, position: Option<UiPoint>, pressed: Option<FormHit>) {
        self.screens.data_driven.set_pointer(position);
        self.screens.data_driven.set_pressed(pressed);
    }

    /// Scrolls the open option list under `position`, or else the form's control column, by
    /// `delta` logical px.
    pub fn scroll_data_driven(&mut self, position: Option<UiPoint>, delta: f32) {
        self.screens.data_driven.scroll_at(position, delta);
    }

    /// The data-driven form's targets from the last frame.
    #[cfg(test)]
    pub fn data_driven_hits(&self) -> &[(FormHit, ui::UiRect)] {
        self.screens.data_driven.hits()
    }
}
