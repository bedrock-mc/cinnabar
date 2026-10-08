//! Both physical sides of a modifier remain held independently.
use super::*;

#[test]
fn releasing_one_modifier_does_not_release_its_other_side() {
    use bevy::input::keyboard::{Key, NativeKey};
    for (left, right) in [
        (KeyCode::ShiftLeft, KeyCode::ShiftRight),
        (KeyCode::ControlLeft, KeyCode::ControlRight),
    ] {
        let mut keys = InventoryKeys::default();
        for (key_code, state) in [
            (left, ButtonState::Pressed),
            (right, ButtonState::Pressed),
            (left, ButtonState::Released),
        ] {
            keys.track_modifier(&KeyboardInput {
                key_code,
                state,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                text: None,
                repeat: false,
                window: bevy::prelude::Entity::PLACEHOLDER,
            });
        }
        assert!(if left == KeyCode::ShiftLeft {
            keys.shift
        } else {
            keys.control
        });
    }
}
