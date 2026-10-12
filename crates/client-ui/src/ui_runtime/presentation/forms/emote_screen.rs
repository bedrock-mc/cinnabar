//! Draws the emote wheel from the runtime's emote state with the live player preview.

use json_ui::InputMode;
use ui::{UiNode, UiPoint};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::frame::EngineFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::emote_screen::{EmoteHit, EmoteInput};

impl UiPresentationRuntime {
    pub(in super::super) fn append_emote_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let preview = self.player_preview_icon();
        // The native wheel authors image controls. Their dynamic texture marker
        // is replaced by the same animated player mesh used by the HUD doll.
        self.player_preview_view = super::super::player_preview::PreviewView::Hud;
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let state = runtime.emotes();
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
            now: now_millis as f64 / 1_000.0,
            nodes,
            next,
            renderers: &mut [],
        };
        self.screens.emote.paint(
            renderer,
            EmoteInput {
                equipping: state.is_equipping(),
                selected_slot: state.selected_slot(),
                slots: state.slots(),
                preview,
            },
            &mut paint_frame,
        )?;
        Ok(())
    }

    pub fn hit_test_emote(&self, point: UiPoint) -> Option<EmoteHit> {
        self.screens.emote.hit_test(point)
    }

    pub fn set_emote_pointer(&mut self, pointer: Option<UiPoint>) {
        self.screens.emote.set_pointer(pointer);
    }

    /// Last active device; stationary cursor coordinates preserve modality.
    pub fn set_emote_input_mode(&mut self, mode: InputMode) {
        self.screens.emote.set_input_mode(mode);
    }

    pub fn emote_frame(&self) -> Option<&EngineFrame> {
        self.screens.emote.frame()
    }

    pub(in super::super) fn close_emote_screen(&mut self) {
        self.screens.emote.close();
    }
}

#[cfg(test)]
#[path = "emote_screen_tests.rs"]
mod tests;
