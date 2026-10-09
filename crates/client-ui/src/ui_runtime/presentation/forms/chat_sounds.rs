//! Authored feedback for chat controls handled by the host input adapter.

use super::{super::UiPresentationRuntime, chat_screen::ChatHit};

/// Compares a settings control across changes to its value.
fn same_control(a: ChatHit, b: ChatHit) -> bool {
    match (a, b) {
        (ChatHit::SettingsAction(a), ChatHit::SettingsAction(b)) => {
            super::menu_sounds::same_control(a, b)
        }
        _ => a == b,
    }
}

impl UiPresentationRuntime {
    /// Forwards one focused chat control's declared feedback through the active packs.
    pub fn play_chat_sound(&self, hit: ChatHit, now: f64) {
        let chat = &self.form_presentation.chat;
        if let Some((_, _, key)) = chat
            .hits
            .iter()
            .find(|(candidate, _, _)| same_control(*candidate, hit))
        {
            chat.audio.activate(
                key,
                json_ui::InputMode::Gamepad,
                now,
                crate::sound_requests::ui_sound,
            );
        }
    }

    /// Routes a mouse press through the actual rendered chat control.
    pub fn sound_chat_mouse(&self, point: ui::UiPoint, now: f64) {
        self.form_presentation
            .chat
            .audio
            .mouse(point, now, crate::sound_requests::ui_sound);
    }

    /// Sounds an accepted touch release on its captured JSON-UI control.
    pub fn sound_chat_touch(
        &self,
        id: u64,
        point: Option<ui::UiPoint>,
        pressed: bool,
        held: bool,
        now: f64,
    ) {
        self.form_presentation.chat.audio.touch(
            id,
            point,
            pressed,
            held,
            now,
            crate::sound_requests::ui_sound,
        );
    }

    /// Cancels held chat touches when input ownership changes.
    pub fn cancel_chat_sound_touches(&self) {
        self.form_presentation.chat.audio.cancel_touches();
    }
}
