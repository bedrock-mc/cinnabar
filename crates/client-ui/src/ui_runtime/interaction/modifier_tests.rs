//! Both physical sides of a modifier remain held independently.
use super::*;

#[test]
fn releasing_one_modifier_does_not_release_its_other_side() {
    for (left, right) in [
        (KeyboardKey::ShiftLeft, KeyboardKey::ShiftRight),
        (KeyboardKey::ControlLeft, KeyboardKey::ControlRight),
    ] {
        let mut keys = InventoryKeys::default();
        for (key, pressed) in [(left, true), (right, true), (left, false)] {
            keys.track_modifier(key, pressed);
        }
        assert!(if left == KeyboardKey::ShiftLeft {
            keys.shift
        } else {
            keys.control
        });
    }
}
