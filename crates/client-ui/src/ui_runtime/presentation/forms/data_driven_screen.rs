//! Draws the showing data-driven form and exposes its targets to input.

use oreui::data_driven_screen::FormHit;
use ui::{UiNode, UiPoint, UiRect};
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

    /// Where `hit` drew last frame, for reading a slider press along its track.
    pub fn data_driven_bounds(&self, hit: FormHit) -> Option<UiRect> {
        self.screens.data_driven.bounds(hit)
    }

    /// Tracks the pointer and the held control for next frame's hover and press state.
    pub fn set_data_driven_pointer(&mut self, position: Option<UiPoint>, pressed: Option<FormHit>) {
        self.screens.data_driven.set_pointer(position);
        self.screens.data_driven.set_pressed(pressed);
    }

    /// Scrolls the form's control column by `delta` logical px.
    pub fn scroll_data_driven(&mut self, delta: f32) {
        self.screens.data_driven.scroll_by(delta);
    }

    /// The dropdown item whose option list is open.
    pub fn data_driven_open_dropdown(&self) -> Option<usize> {
        self.screens.data_driven.open_dropdown()
    }

    /// Opens the option list of dropdown `item`, or closes the open one.
    pub fn set_data_driven_dropdown(&mut self, item: Option<usize>) {
        self.screens.data_driven.set_open_dropdown(item);
    }

    /// Marks the text field item taking typed text.
    pub fn set_data_driven_editing(&mut self, item: Option<usize>) {
        self.screens.data_driven.set_editing(item);
    }
}
