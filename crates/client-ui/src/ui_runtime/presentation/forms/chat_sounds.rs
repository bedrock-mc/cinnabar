//! Authored feedback for chat controls handled by the host input adapter.

use super::{
    super::UiPresentationRuntime,
    chat_screen::{ChatHit, ChatScreen},
};

/// Compares a settings control across changes to its value.
fn same_control(a: ChatHit, b: ChatHit) -> bool {
    match (a, b) {
        (ChatHit::SettingsAction(a), ChatHit::SettingsAction(b)) => {
            super::menu_sounds::same_control(a, b)
        }
        _ => a == b,
    }
}

/// Dispatches every matching entry once, retaining its own replay deadline.
fn dispatch(chat: &ChatScreen, hit: ChatHit, now: f64, mut receive: impl FnMut(&str, f32, f32)) {
    let mut times = chat
        .sound_times
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (index, candidate, sound) in super::menu_sounds::matching(&chat.sounds, hit, same_control) {
        if times.admit(
            candidate,
            index,
            now,
            f64::from(sound.min_seconds),
            same_control,
        ) {
            receive(&sound.name, sound.volume, sound.pitch);
        }
    }
}

impl UiPresentationRuntime {
    /// Forwards a chat control's declared feedback through the active sound packs.
    pub fn play_chat_sound(&self, hit: ChatHit, now: f64) {
        dispatch(
            &self.form_presentation.chat,
            hit,
            now,
            crate::sound_requests::ui_sound,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_preserves_all_authored_entries_and_their_own_intervals() {
        let mut chat = ChatScreen::default();
        chat.sounds = vec![
            (
                ChatHit::Send,
                json_ui::ControlSound {
                    name: "custom.send".into(),
                    volume: 0.5,
                    pitch: 1.25,
                    min_seconds: 1.0,
                },
            ),
            (
                ChatHit::Send,
                json_ui::ControlSound {
                    name: "custom.muted".into(),
                    volume: 0.0,
                    pitch: 1.0,
                    min_seconds: 0.0,
                },
            ),
            (
                ChatHit::Close,
                json_ui::ControlSound {
                    name: "custom.send".into(),
                    volume: 1.0,
                    pitch: 1.0,
                    min_seconds: 1.0,
                },
            ),
        ];
        let mut sounds = Vec::new();
        dispatch(&chat, ChatHit::Send, 1.0, |name, volume, pitch| {
            sounds.push((name.to_owned(), volume, pitch))
        });
        assert_eq!(
            sounds,
            [
                ("custom.send".into(), 0.5, 1.25),
                ("custom.muted".into(), 0.0, 1.0)
            ]
        );
        sounds.clear();
        dispatch(&chat, ChatHit::Send, 2.0, |name, _, _| {
            sounds.push((name.to_owned(), 0.0, 0.0))
        });
        assert_eq!(
            sounds.len(),
            1,
            "the interval is strict and only limits its own entry"
        );
        sounds.clear();
        dispatch(&chat, ChatHit::Close, 1.5, |name, volume, pitch| {
            sounds.push((name.to_owned(), volume, pitch))
        });
        assert_eq!(sounds, [("custom.send".into(), 1.0, 1.0)]);
        let (_, allocations) = crate::allocation_count::count(|| {
            dispatch(&chat, ChatHit::Link(0), 3.0, |_, _, _| {
                panic!("undeclared control")
            });
        });
        assert_eq!(allocations, 0);
    }
}
