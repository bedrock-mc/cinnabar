//! Physical key and pointer observations retained while personal panels consume input.

use super::{POINTER_BUTTONS, PhysicalControls};
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput, mouse::MouseButtonInput},
    prelude::{ButtonInput, Entity, KeyCode, Messages, MouseButton},
};

impl PhysicalControls {
    /// Discards queued edges when no granted control owner is active.
    pub(super) fn discard_pending(
        &mut self,
        keyboard: Option<&Messages<KeyboardInput>>,
        mouse: Option<&Messages<MouseButtonInput>>,
    ) {
        if let Some(events) = keyboard {
            self.keys.clear(events);
        }
        if let Some(events) = mouse {
            self.mouse.clear(events);
        }
        self.left_held = false;
        self.held_keys.clear();
        self.held_seeded = false;
        self.packet_pointer_active = false;
    }

    /// Adopts keys already down at attachment as held, without producing press edges.
    pub(super) fn seed_held<'a>(&mut self, pressed: impl Iterator<Item = &'a KeyCode>) {
        if std::mem::replace(&mut self.held_seeded, true) {
            return;
        }
        for key in pressed.filter(|key| !matches!(key, KeyCode::Unidentified(_))) {
            self.track_held(format!("{key:?}"), true);
        }
    }

    /// Tracks held keys from events, since reserved keys are reset out of the shared input state.
    pub(super) fn track_held(&mut self, key: String, pressed: bool) {
        let index = self.held_keys.iter().position(|held| *held == key);
        match (pressed, index) {
            (true, None) if self.held_keys.len() < mod_api::MAX_CONTROL_KEYS => {
                self.held_keys.push(key);
            }
            (false, Some(index)) => {
                self.held_keys.swap_remove(index);
            }
            _ => {}
        }
    }

    /// Observes same-window pointer edges without consuming ordinary gameplay buttons.
    pub(super) fn pointer_edges(
        &mut self,
        events: Option<&Messages<MouseButtonInput>>,
        buttons: Option<&ButtonInput<MouseButton>>,
        window: Entity,
        focused: bool,
    ) -> Vec<String> {
        let mut pressed = Vec::new();
        let mut down =
            POINTER_BUTTONS.map(|(_, name)| self.held_keys.iter().any(|key| key == name));
        if let Some(events) = events {
            for event in self.mouse.read(events) {
                if event.window != window {
                    continue;
                }
                let Some(index) = POINTER_BUTTONS
                    .iter()
                    .position(|(button, _)| *button == event.button)
                else {
                    continue;
                };
                let next = event.state == ButtonState::Pressed;
                if next && !down[index] && focused && pressed.len() < mod_api::MAX_CONTROL_KEYS {
                    pressed.push(POINTER_BUTTONS[index].1.to_owned());
                }
                down[index] = next;
            }
        } else {
            for (index, (button, name)) in POINTER_BUTTONS.iter().enumerate() {
                down[index] = buttons.is_some_and(|buttons| buttons.pressed(*button));
                if focused && buttons.is_some_and(|buttons| buttons.just_pressed(*button)) {
                    pressed.push((*name).to_owned());
                }
            }
        }
        self.left_held = down[0];
        for (index, (_, name)) in POINTER_BUTTONS.iter().enumerate() {
            self.track_held((*name).into(), down[index]);
        }
        pressed
    }

    /// Remembers input ownership independently of guest reload or quarantine.
    pub(super) fn finish_panel(
        &mut self,
        open: bool,
        focused: bool,
        absorbed: bool,
        captured: bool,
    ) -> bool {
        if !focused || absorbed {
            self.restore_capture = false;
        }
        if open && !self.panel_owned {
            self.restore_capture = captured;
        }
        let restore = !open && self.panel_owned && self.restore_capture && focused && !absorbed;
        self.panel_owned = open;
        if !open {
            self.restore_capture = false;
        }
        restore
    }

    /// Tracks accepted packet-panel gestures, separating interruption from a normal release.
    pub(super) fn packet_pointer_sample(
        &mut self,
        open: bool,
        focused: bool,
        absorbed: bool,
        cursor: bool,
        pressed: bool,
    ) -> (bool, bool) {
        let accepting = open && focused && !absorbed && cursor;
        let cancelled = self.packet_pointer_active && !accepting;
        self.packet_pointer_active =
            accepting && self.left_held && (self.packet_pointer_active || pressed);
        (self.packet_pointer_active, cancelled)
    }

    /// Reads Shift from the single physical key owner after UI-reserved keys were consumed.
    pub(super) fn shift_held(&self, focused: bool) -> bool {
        focused
            && self
                .held_keys
                .iter()
                .any(|key| matches!(key.as_str(), "ShiftLeft" | "ShiftRight"))
    }
}
