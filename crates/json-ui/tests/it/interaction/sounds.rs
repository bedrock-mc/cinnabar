//! Sound events follow control mappings and their interaction edges.

use super::harness::{Screen, ctrl, page};
use json_ui::{InputMode, ScreenEvent};
use serde_json::json;

/// Returns only the sound payloads emitted by an input dispatch.
fn sounds(events: &[ScreenEvent]) -> Vec<(&str, f32, f32)> {
    events
        .iter()
        .filter_map(|event| match event {
            ScreenEvent::Sound {
                name,
                volume,
                pitch,
            } => Some((name.as_str(), *volume, *pitch)),
            _ => None,
        })
        .collect()
}

#[test]
fn controls_emit_once_on_their_interaction_edge() {
    for kind in ["button", "toggle", "dropdown", "slider"] {
        for mode in [InputMode::Mouse, InputMode::Touch, InputMode::Gamepad] {
            let control = ctrl(
                "control",
                kind,
                json!({
                    "size": [40, 20], "focus_enabled": true,
                    "toggle_name": "choice", "slider_name": "level",
                    "sounds": [{"event_type": "button_event", "button_name": "button.choose",
                        "sound_name": "custom.press", "sound_volume": 0.5, "sound_pitch": 1.25}],
                    "button_mappings": [{"from_button_id": "button.menu_select",
                        "to_button_id": "button.choose", "mapping_type": "pressed"}]
                }),
                vec![],
            );
            let mut screen = Screen::new(page(vec![control]));
            let region = screen.regions().remove(0);
            screen.view.focused = Some(region.key);
            screen.hover([5.0, 5.0]);
            let down = screen.press_in("button.menu_select", true, [5.0, 5.0], 1.0, mode);
            let held = screen.press_in("button.menu_select", true, [5.0, 5.0], 1.1, mode);
            let up = screen.press_in("button.menu_select", false, [5.0, 5.0], 1.2, mode);
            let repeated_up = screen.press_in("button.menu_select", false, [5.0, 5.0], 1.3, mode);
            let expected = [("custom.press", 0.5, 1.25)];
            assert_eq!(
                sounds(&down.events),
                if mode == InputMode::Touch {
                    &[][..]
                } else {
                    &expected[..]
                },
                "{kind} {mode:?}"
            );
            assert!(sounds(&held.events).is_empty());
            assert_eq!(
                sounds(&up.events),
                if mode == InputMode::Touch {
                    &expected[..]
                } else {
                    &[][..]
                }
            );
            assert!(sounds(&repeated_up.events).is_empty());
        }
    }
}

#[test]
fn sound_cooldowns_belong_to_each_control() {
    let button = |name, x| {
        ctrl(
            name,
            "button",
            json!({
                "size": [40, 20], "offset": [x, 0],
                "sounds": [{"event_type": "button_event", "sound_name": "custom.press",
                    "min_seconds_between_plays": 1}],
                "button_mappings": [{"from_button_id": "button.menu_select",
                    "to_button_id": "button.choose", "mapping_type": "pressed"}]
            }),
            vec![],
        )
    };
    let mut screen = Screen::new(page(vec![button("a", 0), button("b", 50)]));
    screen.hover([5.0, 5.0]);
    assert_eq!(
        sounds(
            &screen
                .press("button.menu_select", true, [5.0, 5.0], 1.0)
                .events
        )
        .len(),
        1
    );
    screen.press("button.menu_select", false, [5.0, 5.0], 1.1);
    assert!(
        sounds(
            &screen
                .press("button.menu_select", true, [5.0, 5.0], 1.5)
                .events
        )
        .is_empty()
    );
    screen.press("button.menu_select", false, [5.0, 5.0], 1.6);
    screen.hover([55.0, 5.0]);
    assert_eq!(
        sounds(
            &screen
                .press("button.menu_select", true, [55.0, 5.0], 1.7)
                .events
        )
        .len(),
        1
    );
}
