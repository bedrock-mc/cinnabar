//! The open chat's adapter: projects the runtime's chat state into the chat screen and
//! answers the input path's chat, link, settings and coordinate queries.

use std::sync::Arc;

use launcher::menu::settings_options::SettingsOptions;
use ui::{UiNode, UiPoint};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::chat_screen::{ChatHit, ChatInput};

impl UiPresentationRuntime {
    /// Draw the open chat; `Ok(false)` when the engine is not loaded.
    pub(in super::super) fn append_chat_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        let annotations = self.screens.mod_ui.mod_chat_actions().to_vec();
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
        self.screens.chat.paint(
            renderer,
            ChatInput {
                editor: runtime.chat_editor(),
                messages: runtime.chat().messages(),
                suggestions: runtime.chat_suggestions(),
                usage: runtime.chat_usage_hint(),
                selected_suggestion: runtime.chat_selected_suggestion(),
                annotations: &annotations,
                clocks: &self.scene_clock,
                now_millis,
            },
            &mut paint_frame,
        )?;
        Ok(true)
    }

    /// Returns a displayed web target from the last chat frame.
    pub fn chat_link(&self, index: usize) -> Option<&str> {
        self.screens.chat.link(index)
    }

    /// Returns the command carried by a clicked message's Send control.
    pub fn chat_submission(&self, index: usize) -> Option<&str> {
        self.screens.chat.submission(index)
    }

    /// Forget the open chat's scroll and hover once the screen closes.
    pub(in super::super) fn close_chat_screen(&mut self) {
        self.screens.chat.close();
    }

    /// What a press at the window-logical `position` hits on the open chat.
    pub fn hit_test_chat(&self, position: UiPoint) -> Option<ChatHit> {
        self.screens.chat.hit_test(position)
    }

    /// Track the pointer for next frame's hover state.
    pub fn set_chat_pointer(&mut self, position: Option<UiPoint>) {
        self.screens.chat.set_pointer(position);
    }

    /// Scroll the history by a wheel delta in notches, or logical px when
    /// `pixels`; positive scrolls toward older messages.
    pub fn scroll_chat(&mut self, delta: f32, pixels: bool) {
        self.screens.chat.scroll(delta, pixels);
    }

    /// Retains the clicked target while the native popup owns chat input.
    pub fn request_chat_link(&mut self, index: usize) {
        self.screens.chat.request_link(index);
    }

    pub fn chat_link_confirmation_open(&self) -> bool {
        self.screens.chat.link_confirmation_open()
    }

    pub fn cancel_chat_link(&mut self) {
        self.screens.chat.cancel_link();
    }

    /// Call only after the popup's explicit Open action.
    pub fn take_confirmed_chat_link(&mut self) -> Option<String> {
        self.screens.chat.take_confirmed_link()
    }

    /// Records a native chat click as a bounded extension control event.
    pub fn request_chat_extension_action(&mut self, index: usize) {
        self.screens.chat.request_extension_action(index);
    }

    /// Consumes native chat actions once; the caller routes them to their cosmetic owner.
    pub fn take_chat_extension_actions(&mut self) -> Vec<String> {
        self.screens.chat.take_extension_actions()
    }

    /// Keeps gameplay chat bound to current settings without opening the launcher menu.
    pub fn set_chat_settings_snapshot(&mut self, snapshot: (Arc<SettingsOptions>, Option<u16>)) {
        self.screens.chat.settings.set_snapshot(snapshot);
    }

    /// Opens or closes only the native chat settings popup, retaining the chat draft.
    pub fn set_chat_settings_open(&mut self, open: bool) {
        self.screens.chat.settings.set_open(open);
    }

    /// A modal consumes editing keys until it is closed.
    pub fn chat_settings_open(&self) -> bool {
        self.screens.chat.settings.is_open()
    }

    /// Supplies current local feet and the same verified block target used by picking.
    pub fn set_chat_coordinates(&mut self, position: Option<[f32; 3]>, block: Option<[i32; 3]>) {
        self.screens.chat.coordinates.set(position, block);
    }

    /// Edits only the transient header selection; it never changes the chat draft.
    pub fn select_chat_coordinates(&mut self, facing: Option<bool>) {
        self.screens.chat.coordinates.select(facing);
    }

    /// Returns copyable text only while the Creator control is enabled.
    pub fn chat_coordinate_text(&self) -> Option<String> {
        self.screens.chat.coordinate_text()
    }

    /// Starts native feedback only after the platform clipboard accepted the copy.
    pub fn chat_coordinates_copied(&mut self, now_millis: u64) {
        self.screens.chat.coordinates.copied(now_millis);
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    pub(in crate::ui_runtime::presentation) fn set_native_chat_fixture(
        &mut self,
        native: bool,
    ) -> Option<()> {
        self.engine.as_deref_mut()?.set_native_chat_fixture(native);
        Some(())
    }

    pub(in crate::ui_runtime::presentation) fn chat_scene_settings(
        &self,
    ) -> Option<json_ui::ScreenSettings> {
        let engine = self.engine.as_deref()?;
        Some(engine.scene_settings(ui_screens::chat_screen::CHAT_SCREEN, engine.context()))
    }
}

#[cfg(any(test, feature = "test-support"))]
impl UiPresentationRuntime {
    /// The open chat's last laid-out draw nodes, in virtual px.
    pub fn chat_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.chat.nodes()
    }

    /// The open chat's hit rects from the last frame.
    pub fn chat_hits(&self) -> Vec<(ChatHit, ui::UiRect)> {
        self.screens.chat.hits()
    }
}
