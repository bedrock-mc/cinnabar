//! Button mappings, hover dispatch, modal reach and sound components.

use json_ui::{InputMode, MappingScope, PointerInput, ScreenEvent};
use serde_json::json;

use crate::harness::*;

// I1: a pressed mapping from any source fires over the control.
#[test]
fn pressed_mappings_remap_any_source_button() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_ok", "to_button_id": "button.test", "mapping_type": "pressed" }
    ]))]));
    screen.hover([5.0, 5.0]);
    let fired = screen.press("button.menu_ok", true, [5.0, 5.0], 0.0);
    assert_eq!(button_ids(&fired.events), ["button.test"]);
    assert!(fired.consumed);
    screen.hover([150.0, 150.0]);
    assert!(
        button_ids(
            &screen
                .press("button.menu_ok", true, [150.0, 150.0], 1.0)
                .events
        )
        .is_empty()
    );
}

// I2/I6: a global mapping fires anywhere; an omitted type reads as pressed.
#[test]
fn global_fires_anywhere_and_omitted_type_is_pressed() {
    let panel = ctrl(
        "p",
        "input_panel",
        json!({ "size": [10, 10], "button_mappings": [
            { "from_button_id": "button.test_source", "to_button_id": "button.test", "mapping_type": "global" },
            { "from_button_id": "button.other", "to_button_id": "button.untyped" }
        ]}),
        vec![],
    );
    let mut screen = Screen::new(page(vec![panel]));
    let far = [150.0, 150.0];
    assert_eq!(
        button_ids(&screen.press("button.test_source", true, far, 0.0).events),
        ["button.test"]
    );
    assert!(button_ids(&screen.press("button.other", true, far, 0.0).events).is_empty());
}

// I4: a focused mapping fires only while its control has focus.
#[test]
fn focused_mappings_need_focus() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_clear", "to_button_id": "button.clear", "mapping_type": "focused" }
    ]))]));
    assert!(
        button_ids(
            &screen
                .press("button.menu_clear", true, [150.0, 150.0], 0.0)
                .events
        )
        .is_empty()
    );
    screen.view.focused = Some("/root/b".to_owned());
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_clear", true, [150.0, 150.0], 0.0)
                .events
        ),
        ["button.clear"]
    );
}

// I5: a double press within half a second and ten pixels fires double_pressed only.
#[test]
fn double_pressed_needs_two_quick_near_presses() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_select", "to_button_id": "button.single", "mapping_type": "pressed" },
        { "from_button_id": "button.menu_select", "to_button_id": "button.double", "mapping_type": "double_pressed" }
    ]))]));
    screen.hover([5.0, 5.0]);
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_select", true, [5.0, 5.0], 0.0)
                .events
        ),
        ["button.single"]
    );
    screen.press("button.menu_select", false, [5.0, 5.0], 0.1);
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_select", true, [6.0, 5.0], 0.2)
                .events
        ),
        ["button.double"]
    );
    screen.press("button.menu_select", false, [6.0, 5.0], 0.25);
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_select", true, [6.0, 5.0], 2.0)
                .events
        ),
        ["button.single"]
    );
}

// I7/X9: a source-less pressed mapping raises its target on hover in the Up state, never as a
// press (a server form's button would otherwise answer itself under the cursor).
#[test]
fn hover_mappings_fire_on_hover() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "to_button_id": "button.hovered", "mapping_type": "pressed" }
    ]))]));
    let entered = screen.hover([5.0, 5.0]).events;
    assert_eq!(hover_ids(&entered), ["button.hovered"]);
    assert!(button_ids(&entered).is_empty(), "hover is not a press");
    assert!(
        hover_ids(&screen.hover([6.0, 5.0]).events).is_empty(),
        "only on entering"
    );
}

// I8/I10: "any" hands every button to the controller; the scope rides on events.
#[test]
fn any_mapping_and_scope_reach_the_controller() {
    let panel = ctrl(
        "p",
        "input_panel",
        json!({ "size": [10, 10], "button_mappings": [
            { "from_button_id": "any", "mapping_type": "global", "scope": "view" },
            { "from_button_id": "button.x", "to_button_id": "button.y", "mapping_type": "global", "scope": "global" }
        ]}),
        vec![],
    );
    let mut screen = Screen::new(page(vec![panel]));
    let events = screen.press("button.x", true, [150.0, 150.0], 0.0).events;
    let scopes: Vec<(String, MappingScope)> = events
        .iter()
        .filter_map(|event| match event {
            ScreenEvent::Button(button) => Some((button.id.clone(), button.scope)),
            _ => None,
        })
        .collect();
    assert_eq!(
        scopes,
        [
            ("button.x".to_owned(), MappingScope::View),
            ("button.y".to_owned(), MappingScope::Global)
        ]
    );
}

// I11: input_mode_condition limits a mapping to gamepad or pointer input.
#[test]
fn input_mode_conditions_gate_mappings() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_select", "to_button_id": "button.pad", "mapping_type": "pressed", "input_mode_condition": "gamepad" },
        { "from_button_id": "button.menu_select", "to_button_id": "button.mouse", "mapping_type": "pressed", "input_mode_condition": "not_gamepad" }
    ]))]));
    screen.hover([5.0, 5.0]);
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_select", true, [5.0, 5.0], 0.0)
                .events
        ),
        ["button.mouse"]
    );
    screen.view.focused = Some("/root/b".to_owned());
    let pad = screen.press_in(
        "button.menu_select",
        true,
        [5.0, 5.0],
        1.0,
        InputMode::Gamepad,
    );
    assert_eq!(button_ids(&pad.events), ["button.pad"]);
}

// I12: button_up_right_of_first_refusal delivers the release outside after a press inside.
#[test]
fn first_refusal_delivers_the_release() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_select", "to_button_id": "button.drag", "mapping_type": "pressed", "button_up_right_of_first_refusal": true }
    ]))]));
    screen.hover([5.0, 5.0]);
    screen.press("button.menu_select", true, [5.0, 5.0], 0.0);
    screen.hover([150.0, 150.0]);
    let up = screen.press("button.menu_select", false, [150.0, 150.0], 0.1);
    assert!(
        up.events.iter().any(
            |event| matches!(event, ScreenEvent::Button(b) if b.id == "button.drag" && !b.down)
        )
    );
}

// I15: ignore_input_scope fires a pressed mapping without the pointer over it.
#[test]
fn ignore_input_scope_skips_the_hover_test() {
    let mut screen = Screen::new(page(vec![mapped(json!([
        { "from_button_id": "button.menu_left", "to_button_id": "button.less", "mapping_type": "pressed", "ignore_input_scope": true }
    ]))]));
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_left", true, [150.0, 150.0], 0.0)
                .events
        ),
        ["button.less"]
    );
}

// B2: consume_event false lets the press reach the control beneath.
#[test]
fn non_consuming_mappings_let_the_event_continue() {
    let under = ctrl(
        "under",
        "button",
        json!({ "size": [40, 20], "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.under", "mapping_type": "pressed" }
        ]}),
        vec![],
    );
    let over = ctrl(
        "over",
        "input_panel",
        json!({ "size": [40, 20], "layer": 5, "consume_hover_events": false, "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.over", "mapping_type": "pressed", "consume_event": false }
        ]}),
        vec![],
    );
    let mut screen = Screen::new(page(vec![under, over]));
    screen.hover([5.0, 5.0]);
    let fired = screen.press("button.menu_select", true, [5.0, 5.0], 0.0);
    assert_eq!(button_ids(&fired.events), ["button.over", "button.under"]);
}

// X8: a hover-consuming control hides the one beneath from hover; a non-consuming one does not.
#[test]
fn consume_hover_events_decides_the_hover_chain() {
    let make = |consume: bool| {
        page(vec![
            mapped(json!([{ "to_button_id": "button.below", "mapping_type": "pressed" }])),
            ctrl(
                "top",
                "input_panel",
                json!({ "size": [40, 20], "layer": 5, "consume_hover_events": consume }),
                vec![],
            ),
        ])
    };
    let mut blocked = Screen::new(make(true));
    assert!(hover_ids(&blocked.hover([5.0, 5.0]).events).is_empty());
    let mut through = Screen::new(make(false));
    assert_eq!(
        hover_ids(&through.hover([5.0, 5.0]).events),
        ["button.below"]
    );
}

// X7: hover_enabled false never hovers, so its hover state never shows.
#[test]
fn hover_disabled_controls_never_hover() {
    let button = ctrl(
        "b",
        "button",
        json!({ "size": [40, 20], "hover_enabled": false, "default_control": "d", "hover_control": "h" }),
        vec![
            ctrl("d", "panel", json!({}), vec![]),
            ctrl("h", "panel", json!({}), vec![]),
        ],
    );
    let mut screen = Screen::new(page(vec![button]));
    screen.hover([5.0, 5.0]);
    assert_eq!(screen.view.hovered, None);
    assert!(!screen.visible("h"));
}

// X10: prevent_touch_input ignores touch hover.
#[test]
fn prevent_touch_input_ignores_touch() {
    let mut screen = Screen::new(page(vec![ctrl(
        "b",
        "button",
        json!({ "size": [40, 20], "prevent_touch_input": true }),
        vec![],
    )]));
    let regions = screen.regions();
    let touch = PointerInput {
        point: Some([5.0, 5.0]),
        held: false,
        mode: InputMode::Touch,
        now: 0.0,
    };
    screen.dispatcher.pointer(&regions, &mut screen.view, touch);
    assert_eq!(screen.view.hovered, None);
}

// X1: a modal panel keeps global mappings outside it from firing.
#[test]
fn modal_panels_stop_input_outside_them() {
    let behind = ctrl(
        "behind",
        "input_panel",
        json!({ "size": [10, 10], "button_mappings": [
            { "from_button_id": "button.menu_cancel", "to_button_id": "button.behind", "mapping_type": "global" }
        ]}),
        vec![],
    );
    let dialog = ctrl(
        "dialog",
        "input_panel",
        json!({ "size": [100, 100], "modal": true, "layer": 10, "button_mappings": [
            { "from_button_id": "button.menu_cancel", "to_button_id": "button.inside", "mapping_type": "global" }
        ]}),
        vec![],
    );
    let mut screen = Screen::new(page(vec![behind, dialog]));
    let fired = screen.press("button.menu_cancel", true, [150.0, 150.0], 0.0);
    assert_eq!(button_ids(&fired.events), ["button.inside"]);
}

// X3: always_listen_to_input keeps a control reachable behind a modal and by rect.
#[test]
fn always_listening_controls_reach_past_modals() {
    let listener = ctrl(
        "listener",
        "input_panel",
        json!({ "size": [10, 10], "always_listen_to_input": true, "button_mappings": [
            { "from_button_id": "button.chat", "to_button_id": "button.chat", "mapping_type": "global" }
        ]}),
        vec![],
    );
    let dialog = ctrl(
        "dialog",
        "input_panel",
        json!({ "size": [100, 100], "modal": true, "layer": 10 }),
        vec![],
    );
    let mut screen = Screen::new(page(vec![listener, dialog]));
    assert_eq!(
        button_ids(
            &screen
                .press("button.chat", true, [150.0, 150.0], 0.0)
                .events
        ),
        ["button.chat"]
    );
}

// A1/A2: shorthand and per-button sounds play on the interaction, rate-limited.
#[test]
fn sound_components_play_on_interaction() {
    let button = ctrl(
        "b",
        "button",
        json!({
            "size": [40, 20], "sound_name": "random.click", "sound_volume": 0.5, "sound_pitch": 1.2,
            "sounds": [{
                "sound_name": "random.pop", "sound_volume": 0.25, "sound_pitch": 0.8,
                "event_type": "button_event", "button_name": "button.test", "min_seconds_between_plays": 1
            }],
            "button_mappings": [
                { "from_button_id": "button.menu_select", "to_button_id": "button.test", "mapping_type": "pressed" }
            ]
        }),
        vec![],
    );
    let sounds = |events: &[ScreenEvent]| -> Vec<(String, f32, f32)> {
        events
            .iter()
            .filter_map(|event| match event {
                ScreenEvent::Sound {
                    name,
                    volume,
                    pitch,
                } => Some((name.clone(), *volume, *pitch)),
                _ => None,
            })
            .collect()
    };
    let mut screen = Screen::new(page(vec![button]));
    screen.hover([5.0, 5.0]);
    let first = screen.press("button.menu_select", true, [5.0, 5.0], 0.0);
    assert_eq!(
        sounds(&first.events),
        [
            ("random.click".to_owned(), 0.5, 1.2),
            ("random.pop".to_owned(), 0.25, 0.8)
        ]
    );
    screen.press("button.menu_select", false, [5.0, 5.0], 0.1);
    let again = screen.press("button.menu_select", true, [5.0, 5.0], 0.5);
    assert_eq!(
        sounds(&again.events),
        [("random.click".to_owned(), 0.5, 1.2)]
    );
}

// X2: an inline modal still leaves the scroll views around it scrolling.
#[test]
fn inline_modals_keep_neighbouring_views_scrolling() {
    let view = ctrl(
        "list",
        "scroll_view",
        json!({
            "size": [100, 100], "scroll_content": "content", "scroll_view_port": "port",
            "scrollbar_track": "track", "scrollbar_box": "box"
        }),
        vec![
            ctrl(
                "port",
                "panel",
                json!({ "size": [100, 100] }),
                vec![ctrl(
                    "content",
                    "panel",
                    json!({ "size": [100, 300] }),
                    vec![],
                )],
            ),
            ctrl("track", "scroll_track", json!({ "size": [4, 100] }), vec![]),
            ctrl("box", "scrollbar_box", json!({ "size": [4, 10] }), vec![]),
        ],
    );
    let popup = |inline: bool| {
        ctrl(
            "popup",
            "input_panel",
            json!({ "size": [100, 24], "layer": 5, "modal": true, "inline_modal": inline }),
            vec![],
        )
    };
    for inline in [false, true] {
        let bound = Screen::new(page(vec![view.clone(), popup(inline)])).bound();
        let state = json_ui::ViewState::default();
        let (laid, report) = json_ui::layout_with(&bound, [200.0, 200.0], &env(), &state);
        let regions = json_ui::hit_regions(&laid);
        assert_eq!(
            json_ui::scroll_target(&regions, &report, [5.0, 5.0]).is_some(),
            inline
        );
    }
}

// X11: holding the gesture button publishes pointer motion as gesture deltas.
#[test]
fn gesture_buttons_track_pointer_motion() {
    let doll = ctrl(
        "doll",
        "input_panel",
        json!({ "size": [50, 50], "gesture_tracking_button": "button.turn_doll", "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.turn_doll", "mapping_type": "pressed" }
        ]}),
        vec![],
    );
    let mut screen = Screen::new(page(vec![doll]));
    screen.hover([10.0, 10.0]);
    screen.press("button.menu_select", true, [10.0, 10.0], 0.0);
    screen.hover([16.0, 12.0]);
    let bag = screen
        .view
        .components
        .bag("/root/doll")
        .cloned()
        .unwrap_or_default();
    assert_eq!(bag["#gesture_mouse_delta_x"], json!(6.0));
    assert_eq!(bag["#gesture_mouse_delta_y"], json!(2.0));
    screen.press("button.menu_select", false, [16.0, 12.0], 0.1);
    let bag = screen
        .view
        .components
        .bag("/root/doll")
        .cloned()
        .unwrap_or_default();
    assert_eq!(bag["#gesture_mouse_delta_x"], json!(0.0));
}

// I14: an alternate-scope mapping fires by position even under a hover-consuming control.
#[test]
fn alternate_input_scope_reads_the_position() {
    let under = ctrl(
        "under",
        "button",
        json!({ "size": [40, 20], "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.alt", "mapping_type": "pressed", "alternate_input_scope": true }
        ]}),
        vec![],
    );
    let cover = ctrl(
        "cover",
        "input_panel",
        json!({ "size": [40, 20], "layer": 5 }),
        vec![],
    );
    let mut screen = Screen::new(page(vec![under, cover]));
    screen.hover([5.0, 5.0]);
    assert_eq!(
        button_ids(
            &screen
                .press("button.menu_select", true, [5.0, 5.0], 0.0)
                .events
        ),
        ["button.alt"]
    );
}
