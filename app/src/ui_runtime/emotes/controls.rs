//! Cardinal input belongs to an open wheel, even when it is the opening binding.
use bevy::{
    input::gamepad::{Gamepad, GamepadButton},
    prelude::{KeyCode, Query},
};

use crate::semantic_controls::SemanticInputSnapshot;
use semantic_input::Action;
use ui::UiAction;

/// Keeps D-pad selection inside an already-open wheel.
pub(super) fn directional_navigation(pads: &Query<&Gamepad>) -> bool {
    pads.iter().any(|pad| {
        [
            GamepadButton::DPadUp,
            GamepadButton::DPadRight,
            GamepadButton::DPadDown,
            GamepadButton::DPadLeft,
        ]
        .into_iter()
        .any(|button| pad.just_pressed(button))
    })
}

/// Movement and gameplay actions interrupt emote playback.
pub(super) fn should_stop_emote(input: &SemanticInputSnapshot) -> bool {
    input.raw_movement().iter().any(|axis| *axis != 0.0)
        || [Action::Jump, Action::Attack, Action::Use, Action::Sneak]
            .iter()
            .any(|action| input.phase(*action).held)
}

/// Maps the number keys to the wheel's selectable slots.
pub(super) fn slot_key(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(0),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(1),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(2),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(3),
        _ => None,
    }
}

/// Maps keyboard navigation to wheel actions.
pub(super) fn wheel_key(key: KeyCode) -> Option<UiAction> {
    match key {
        KeyCode::Escape => Some(UiAction::Cancel),
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => Some(UiAction::Accept),
        KeyCode::ArrowUp => Some(UiAction::Navigate([0, -1])),
        KeyCode::ArrowRight => Some(UiAction::Navigate([1, 0])),
        KeyCode::ArrowDown => Some(UiAction::Navigate([0, 1])),
        KeyCode::ArrowLeft => Some(UiAction::Navigate([-1, 0])),
        _ => None,
    }
}
