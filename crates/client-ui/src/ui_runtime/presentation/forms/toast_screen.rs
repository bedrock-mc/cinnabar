//! Shows the HUD store's toast through the toast screen and records where it takes presses.

use ui::{ToastPress, UiNode, UiPoint};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::toast_screen;

impl UiPresentationRuntime {
    /// What pressing the showing toast at `point` opens.
    pub fn toast_press_at(&self, point: UiPoint) -> Option<ToastPress> {
        let (press, bounds) = self.screens.hud.toast_press?;
        bounds.contains(point).then_some(press)
    }

    /// Draw the showing toast, if any, and where it takes presses.
    pub(in super::super) fn append_toast_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        self.screens.hud.toast_press = None;
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let Some(toast) = runtime.hud().showing_toast(now_millis) else {
            return Ok(());
        };
        let translate = |key: &str| runtime.translation(key);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        let frame = toast_screen::paint(
            &mut self.screens.hud.toast,
            renderer,
            &toast,
            &mut paint_frame,
        )?;
        if let Some(press) = toast.press
            && let Some(frame) = frame
        {
            let origin = [self.safe_area.left(), self.safe_area.top()];
            self.screens.hud.toast_press =
                toast_screen::press_bounds(&frame, origin).map(|bounds| (press, bounds));
        }
        Ok(())
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The toast screen's last laid-out draw nodes, in virtual px.
    pub fn toast_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.hud.toast.nodes()
    }
}
