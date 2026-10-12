//! Draws the active sign edit through the sign screen and answers its exit presses.

use ui::{UiNode, UiPoint};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::sign_editor::SignInput;

impl UiPresentationRuntime {
    /// Whether a press at `point` lands outside the sign, which closes the editor.
    pub fn sign_editor_exit_hit(&self, point: UiPoint) -> bool {
        self.screens.sign.exit_hit(point)
    }

    /// Drops the hidden editor's hit regions.
    pub(in crate::ui_runtime::presentation) fn hide_sign_editor(&mut self) {
        self.screens.sign.hide();
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui_runtime::presentation) fn append_sign_editor(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        self.screens.sign.hide();
        let Some(edit) = runtime.sign_editor().active() else {
            return Ok(());
        };
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let look = edit.look();
        let translate = |key: &str| runtime.translation(key);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        self.screens.sign.paint(
            renderer,
            SignInput {
                texture: &look.texture,
                hanging: look.hanging,
                lines: edit.lines(),
                cursor: edit.cursor(),
                color: edit.color(),
                now_millis,
                clocks: &self.scene_clock,
            },
            &mut paint_frame,
        )?;
        Ok(())
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The sign screen's last laid-out draw nodes, in virtual px.
    pub fn sign_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.sign.nodes()
    }
}
