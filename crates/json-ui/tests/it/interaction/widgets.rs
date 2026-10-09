//! Retained toggle, slider, edit box and dropdown components and their state controls.

use json_ui::{
    DataSource, InputMode, PointerInput, ResolvedControl, ScreenEvent, ViewState, bind, layout_with,
};
use serde_json::json;

use crate::interaction::harness::*;

// T1/T2: a click flips a plain toggle's retained state and its visual state.
#[test]
fn toggles_keep_their_own_state() {
    let mut pack = toggle(
        "t",
        json!({ "toggle_name": "pack_setting", "toggle_default_state": false }),
    );
    toggle_child_names(&mut pack);
    let mut screen = Screen::new(page(vec![pack]));
    assert!(screen.visible("t_off") && !screen.visible("t_on"));
    let events = screen.click([5.0, 5.0]);
    assert!(events.iter().any(|event| matches!(
        event,
        ScreenEvent::Toggle { name, checked: true, by_click: true, .. } if name == "pack_setting"
    )));
    // Without hover controls a hovered toggle shows neither state, as in vanilla.
    assert!(!screen.visible("t_on") && !screen.visible("t_off"));
    screen.hover([150.0, 150.0]);
    assert!(screen.visible("t_on") && !screen.visible("t_off"));
    screen.click([5.0, 5.0]);
    screen.hover([150.0, 150.0]);
    assert!(screen.visible("t_off"));
}

// T3: selecting a radio member checks it and unchecks the rest of its group.
#[test]
fn radio_groups_stay_exclusive() {
    let mut a = toggle(
        "a",
        json!({ "radio_toggle_group": true, "toggle_group_forced_index": 0, "#toggle_state": true }),
    );
    let mut b = toggle(
        "b",
        json!({ "radio_toggle_group": true, "toggle_group_forced_index": 1, "offset": [50, 0] }),
    );
    toggle_child_names(&mut a);
    toggle_child_names(&mut b);
    let mut screen = Screen::new(page(vec![a, b]));
    let events = screen.click([55.0, 5.0]);
    assert!(events.iter().any(|event| matches!(
        event,
        ScreenEvent::Toggle {
            index: Some(1),
            checked: true,
            ..
        }
    )));
    screen.hover([150.0, 150.0]);
    assert!(screen.visible("b_on") && screen.visible("a_off") && !screen.visible("a_on"));
    assert!(
        screen
            .click([55.0, 5.0])
            .iter()
            .all(|event| !matches!(event, ScreenEvent::Toggle { .. }))
    );
}

#[test]
fn nested_radio_groups_keep_other_parents_checked() {
    for scoped in [false, true] {
        let controls = [("a", true, 0), ("b", false, 50), ("c", true, 100)]
        .into_iter()
        .map(|(name, checked, x)| {
            toggle(
                name,
                json!({"radio_toggle_group": true, "#toggle_state": checked, "offset": [x, 0], "toggle_grid_collection_name": if scoped { Some("custom_dropdown") } else { None }}),
            )
        })
        .collect();
        let mut screen = Screen::new(page(controls));
        let mut regions = screen.regions();
        for region in &mut regions {
            let (parent, option) = match region.name.as_str() {
                "a" => (0, 0),
                "b" => (0, 1),
                "c" => (1, 0),
                _ => continue,
            };
            region.collections = vec![
                ("custom_form".into(), parent),
                ("custom_dropdown".into(), option),
            ];
        }
        let second = regions.iter().find(|region| region.name == "b").unwrap();
        screen.view.focused = Some(second.key.clone());
        screen.dispatcher.button(
            &regions,
            &mut screen.view,
            json_ui::ButtonInput {
                id: "button.menu_select",
                down: true,
                point: None,
                mode: InputMode::Gamepad,
                now: 0.0,
            },
        );
        for (name, expected) in [("a", false), ("b", true), ("c", scoped)] {
            let region = regions.iter().find(|region| region.name == name).unwrap();
            let checked = screen
                .view
                .components
                .bag(&region.key)
                .and_then(|bag| bag.get("#toggle_state"))
                .and_then(serde_json::Value::as_bool)
                .or(region.checked)
                .unwrap_or(false);
            assert_eq!(checked, expected, "radio {name}");
        }
    }
}

// T6: toggle_on_hover flips a plain toggle as the pointer enters.
#[test]
fn toggle_on_hover_flips_on_entry() {
    let mut hovering = toggle("t", json!({ "toggle_on_hover": true }));
    toggle_child_names(&mut hovering);
    let mut screen = Screen::new(page(vec![hovering]));
    let events = screen.hover([5.0, 5.0]).events;
    assert!(events.iter().any(|event| matches!(
        event,
        ScreenEvent::Toggle {
            checked: true,
            by_click: false,
            ..
        }
    )));
}

// T5/T7: directional toggles answer their on/off ids and the stick.
#[test]
fn directional_toggles_answer_on_off_ids() {
    let mut pad = toggle(
        "t",
        json!({
            "enable_directional_toggling": true, "toggle_on_button": "button.enable", "toggle_off_button": "button.disable",
            "button_mappings": [
                { "from_button_id": "button.menu_right", "to_button_id": "button.enable", "mapping_type": "focused" },
                { "from_button_id": "button.menu_left", "to_button_id": "button.disable", "mapping_type": "focused" }
            ]
        }),
    );
    toggle_child_names(&mut pad);
    let mut screen = Screen::new(page(vec![pad]));
    screen.view.focused = Some("/root/t".to_owned());
    screen.press("button.menu_right", true, [150.0, 150.0], 0.0);
    assert!(screen.visible("t_on"));
    screen.press("button.menu_left", true, [150.0, 150.0], 0.1);
    assert!(screen.visible("t_off"));
    let regions = screen.regions();
    screen
        .dispatcher
        .direction(&regions, &mut screen.view, [0.9, 0.0], 1.0);
    assert!(screen.visible("t_on"));
}

// T11: a clear manager unchecks its groups' toggles on interaction.
#[test]
fn toggle_managers_clear_their_groups() {
    let mut member = toggle("t", json!({ "#toggle_state": true }));
    toggle_child_names(&mut member);
    let manager = ctrl(
        "m",
        "input_panel",
        json!({ "size": [10, 10], "offset": [100, 100], "toggle_manager_behavior": "clear", "toggle_manage_groups": ["group"],
            "button_mappings": [{ "from_button_id": "button.reset", "to_button_id": "button.reset", "mapping_type": "global" }] }),
        vec![],
    );
    let mut screen = Screen::new(page(vec![member, manager]));
    assert!(screen.visible("t_on"));
    screen.press("button.reset", true, [150.0, 150.0], 0.0);
    assert!(screen.visible("t_off"));
}

// T9/B1: state controls resolve as named descendants, not only children.
#[test]
fn nested_state_controls_follow_their_state() {
    let button = ctrl(
        "b",
        "button",
        json!({ "size": [40, 20], "default_control": "normal", "hover_control": "over" }),
        vec![ctrl(
            "wrapper",
            "panel",
            json!({}),
            vec![
                ctrl("normal", "label", json!({}), vec![]),
                ctrl("over", "label", json!({}), vec![]),
            ],
        )],
    );
    let mut screen = Screen::new(page(vec![button]));
    assert!(screen.visible("normal") && !screen.visible("over"));
    screen.view.hovered = Some("/root/b".to_owned());
    assert!(!screen.visible("normal") && screen.visible("over"));
}

// B1: hovering hides the default even without a hover control.
#[test]
fn hover_without_a_hover_control_hides_the_default() {
    let button = ctrl(
        "b",
        "button",
        json!({ "size": [40, 20], "default_control": "normal" }),
        vec![ctrl("normal", "label", json!({}), vec![])],
    );
    let mut screen = Screen::new(page(vec![button]));
    screen.view.hovered = Some("/root/b".to_owned());
    assert!(!screen.visible("normal"));
}

// L1/L2: dragging a five-step slider publishes quantized steps and keeps them.
#[test]
fn sliders_keep_their_quantized_value() {
    let mut screen = Screen::new(page(vec![slider(json!({ "slider_steps": 5 }), vec![])]));
    screen.hover([60.0, 5.0]);
    let down = screen.press("button.menu_select", true, [60.0, 5.0], 0.0);
    assert_eq!(slider_values(&down.events), [(2.0, Some(2))]);
    let regions = screen.regions();
    let drag = PointerInput {
        point: Some([95.0, 5.0]),
        held: true,
        mode: InputMode::Mouse,
        now: 0.1,
    };
    let moved = screen.dispatcher.pointer(&regions, &mut screen.view, drag);
    assert_eq!(slider_values(&moved.events), [(4.0, Some(4))]);
    let up = screen.press("button.menu_select", false, [95.0, 5.0], 0.2);
    assert!(
        up.events
            .iter()
            .any(|event| matches!(event, ScreenEvent::Slider { finished: true, .. }))
    );
    let bound = screen.bound();
    let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &screen.view);
    assert_eq!(
        find(&laid, "s").control.properties["#slider_value"],
        json!(4.0)
    );
}

// L3/L4/L10: a vertical inverted slider places its nested box along Y.
#[test]
fn vertical_inverted_sliders_travel_y() {
    let root = page(vec![slider(
        json!({ "size": [20, 100], "slider_direction": "vertical", "slider_inverted": true, "#slider_value": 0.25, "slider_box_control": "thumb" }),
        vec![ctrl(
            "wrap",
            "panel",
            json!({}),
            vec![ctrl(
                "thumb",
                "slider_box",
                json!({ "size": [8, 8] }),
                vec![],
            )],
        )],
    )]);
    let (laid, _) = layout_with(&root, [200.0, 200.0], &env(), &ViewState::default());
    let thumb = find(&laid, "thumb").rect;
    assert_eq!(thumb.y, 75.0 - 4.0);
    assert_eq!(thumb.x, 0.0);
}

// L9/L5: the slider's selected and small-step ids drive its box and value.
#[test]
fn slider_operation_ids_select_and_step() {
    let mut screen = Screen::new(page(vec![slider(
        json!({
            "slider_speed": 10, "slider_small_increase_button": "button.more", "slider_small_decrease_button": "button.less",
            "slider_selected_button": "button.choose", "slider_deselected_button": "button.leave",
            "button_mappings": [
                { "from_button_id": "button.menu_ok", "to_button_id": "button.choose", "mapping_type": "focused" },
                { "from_button_id": "button.menu_right", "to_button_id": "button.more", "mapping_type": "pressed", "ignore_input_scope": true },
                { "from_button_id": "button.menu_cancel", "to_button_id": "button.leave", "mapping_type": "focused" }
            ]
        }),
        vec![],
    )]));
    screen.view.focused = Some("/root/s".to_owned());
    let far = [150.0, 150.0];
    let tap = |screen: &mut Screen, id: &str, now: f64| {
        let events = screen.press(id, true, far, now).events;
        screen.press(id, false, far, now);
        events
    };
    assert!(slider_values(&tap(&mut screen, "button.menu_right", 0.0)).is_empty());
    tap(&mut screen, "button.menu_ok", 0.1);
    assert_eq!(screen.view.components.selected(), Some("/root/s"));
    let stepped = tap(&mut screen, "button.menu_right", 0.2);
    assert!((slider_values(&stepped)[0].0 - 0.1).abs() < 1e-6);
    tap(&mut screen, "button.menu_cancel", 0.3);
    assert_eq!(screen.view.components.selected(), None);
}

// L8/L14: select-on-hover indents the slider box through indent_control.
#[test]
fn slider_box_indents_when_selected() {
    let thumb = ctrl(
        "thumb",
        "slider_box",
        json!({ "size": [8, 8], "default_control": "normal", "hover_control": "hot", "indent_control": "selected" }),
        vec![
            ctrl("normal", "label", json!({}), vec![]),
            ctrl("hot", "label", json!({}), vec![]),
            ctrl("selected", "label", json!({}), vec![]),
        ],
    );
    let mut screen = Screen::new(page(vec![slider(
        json!({ "slider_select_on_hover": true, "slider_box_control": "thumb" }),
        vec![thumb],
    )]));
    assert!(screen.visible("normal"));
    screen.hover([5.0, 5.0]);
    assert!(screen.visible("selected") && !screen.visible("hot") && !screen.visible("normal"));
}

// L12/L13/L11: hover swaps a slider's default, background and progress targets.
#[test]
fn slider_hover_swaps_paired_targets() {
    let names = [
        "bar",
        "bar_hover",
        "bg",
        "bg_hover",
        "progress",
        "progress_hover",
    ];
    let mut screen = Screen::new(page(vec![slider(
        json!({
            "default_control": "bar", "hover_control": "bar_hover",
            "background_control": "bg", "background_hover_control": "bg_hover",
            "progress_control": "progress", "progress_hover_control": "progress_hover"
        }),
        names
            .iter()
            .map(|name| ctrl(name, "image", json!({}), vec![]))
            .collect(),
    )]));
    let shown = |screen: &Screen| names.map(|name| screen.visible(name));
    assert_eq!(shown(&screen), [true, false, true, false, true, false]);
    screen.view.hovered = Some("/root/s".to_owned());
    assert_eq!(shown(&screen), [false, true, false, true, false, true]);
}

// L16: a gather manager republishes its sliders on a button release.
#[test]
fn slider_managers_gather_on_release() {
    let manager = ctrl(
        "m",
        "input_panel",
        json!({ "size": [10, 10], "offset": [150, 150], "slider_manager_behavior": "gather", "slider_manage_groups": ["value"],
            "button_mappings": [{ "from_button_id": "button.save", "to_button_id": "button.save", "mapping_type": "global" }] }),
        vec![],
    );
    let mut screen = Screen::new(page(vec![
        slider(json!({ "#slider_value": 0.5 }), vec![]),
        manager,
    ]));
    screen.press("button.save", true, [0.0, 190.0], 0.0);
    let up = screen.press("button.save", false, [0.0, 190.0], 0.1);
    assert_eq!(slider_values(&up.events), [(0.5, None)]);
}

// E1/E3/E8/E9/E13: selecting edits the text target, hides the placeholder, respects max_length.
#[test]
fn edit_boxes_own_their_text() {
    let mut screen = Screen::new(page(vec![edit_box(json!({}))]));
    assert!(screen.visible("hint"));
    let events = screen.click([5.0, 5.0]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ScreenEvent::TextEditSelected { selected: true, .. }))
    );
    assert!(!screen.visible("hint"));
    let regions = screen.regions();
    let typed = screen
        .dispatcher
        .text(&regions, &mut screen.view, "hello!", None);
    assert!(
        typed.events.is_empty(),
        "six characters exceed max_length 5"
    );
    screen
        .dispatcher
        .text(&regions, &mut screen.view, "hey", None);
    assert_eq!(text_of(&screen, "display"), "hey");
    let bound = screen.bound();
    let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &screen.view);
    assert_eq!(
        find(&laid, "display").control.properties["#text_edit_selected"],
        json!(true)
    );
}

// E10/B2: a press elsewhere deselects through the non-consuming global mapping, unless not deselectable.
#[test]
fn outside_presses_deselect_unless_pinned() {
    let mut screen = Screen::new(page(vec![edit_box(json!({}))]));
    screen.click([5.0, 5.0]);
    let events = screen.click([150.0, 150.0]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ScreenEvent::TextEdit { finished: true, .. }))
    );
    assert_eq!(screen.view.components.selected(), None);
    let mut pinned = Screen::new(page(vec![edit_box(json!({ "can_be_deselected": false }))]));
    pinned.click([5.0, 5.0]);
    pinned.click([150.0, 150.0]);
    assert_eq!(pinned.view.components.selected(), Some("/root/edit"));
}

// E4/E11: enabled_newline keeps Enter as text; always_listening takes text unselected.
#[test]
fn newline_and_always_listening_boxes() {
    let mut screen = Screen::new(page(vec![edit_box(
        json!({ "enabled_newline": true, "always_listening": true, "max_length": 10 }),
    )]));
    let regions = screen.regions();
    screen
        .dispatcher
        .text(&regions, &mut screen.view, "a", None);
    screen
        .dispatcher
        .text(&regions, &mut screen.view, "\r", None);
    assert_eq!(
        screen
            .view
            .components
            .edit("/root/edit")
            .map(|edit| edit.text.as_str()),
        Some("a\r")
    );
}

// E12: the placeholder takes its hover colour while hovered.
#[test]
fn placeholder_hover_color_switches() {
    let mut screen = Screen::new(page(vec![edit_box(
        json!({ "place_holder_text_hover_color": [1, 0, 0, 1] }),
    )]));
    screen.hover([5.0, 5.0]);
    let bound = screen.bound();
    let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &screen.view);
    assert_eq!(
        find(&laid, "hint").control.properties["color"],
        json!([1, 0, 0, 1])
    );
    screen.hover([150.0, 150.0]);
    let bound = screen.bound();
    let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &screen.view);
    assert!(!find(&laid, "hint").control.properties.contains_key("color"));
}

// D1: a dropdown's toggle state opens its content through the content's view binding.
#[test]
fn dropdowns_open_their_content() {
    let content = ctrl(
        "content",
        "input_panel",
        json!({ "size": [100, 80], "bindings": [{
            "binding_type": "view", "source_control_name": "drop", "source_property_name": "#toggle_state",
            "target_property_name": "#visible", "resolve_sibling_scope": true
        }] }),
        vec![],
    );
    let mut drop = toggle(
        "drop",
        json!({ "dropdown_name": "drop", "dropdown_content_control": "content", "property_bag": { "#toggle_state": false } }),
    );
    drop.control_type = Some("dropdown".to_owned());
    toggle_child_names(&mut drop);
    let mut screen = Screen::new(page(vec![drop, content]));
    assert!(!screen.visible("content"));
    screen.click([5.0, 5.0]);
    assert!(screen.visible("content"));
}

// D2/D3: content drops from the dropdown row, raised to end inside the area.
#[test]
fn dropdown_content_is_clamped_into_its_area() {
    let row = ctrl(
        "row",
        "panel",
        json!({ "offset": [0, 180], "size": [100, 20] }),
        vec![
            ctrl(
                "drop",
                "dropdown",
                json!({ "size": [100, 20], "dropdown_name": "drop", "dropdown_area": "area", "dropdown_content_control": "content" }),
                vec![],
            ),
            ctrl("content", "panel", json!({ "size": [100, 80] }), vec![]),
        ],
    );
    let area = ctrl("area", "panel", json!({ "size": [200, 200] }), vec![row]);
    let root = ctrl("root", "panel", json!({ "size": [200, 200] }), vec![area]);
    let (laid, _) = layout_with(&root, [200.0, 200.0], &env(), &ViewState::default());
    assert_eq!(find(&laid, "content").rect.y, 120.0);
}

// T4: a default manager restores each radio group to its default-selected member.
#[test]
fn default_managers_select_the_group_default() {
    let mut a = toggle(
        "a",
        json!({ "radio_toggle_group": true, "toggle_group_forced_index": 0, "toggle_group_default_selected": 1 }),
    );
    let mut b = toggle(
        "b",
        json!({ "radio_toggle_group": true, "toggle_group_forced_index": 1, "toggle_group_default_selected": 1, "offset": [50, 0] }),
    );
    toggle_child_names(&mut a);
    toggle_child_names(&mut b);
    let manager = ctrl(
        "m",
        "input_panel",
        json!({ "size": [10, 10], "offset": [100, 100], "toggle_manager_behavior": "default", "toggle_manage_groups": ["group"],
            "button_mappings": [{ "from_button_id": "button.reset", "to_button_id": "button.reset", "mapping_type": "global" }] }),
        vec![],
    );
    let mut screen = Screen::new(page(vec![a, b, manager]));
    screen.press("button.reset", true, [150.0, 150.0], 0.0);
    assert!(screen.visible("b_on") && screen.visible("a_off"));
}

// L7: a step slider's factory creates its inner step marks across the slider.
#[test]
fn step_sliders_create_their_step_marks() {
    struct Steps;
    impl json_ui::ControlLibrary for Steps {
        fn resolve(&self, reference: &json_ui::ControlRef) -> Option<ResolvedControl> {
            Some(ctrl(
                &reference.name,
                "image",
                json!({ "size": [2, 6], "offset": "$step_offset" }),
                vec![],
            ))
        }
    }
    let mut control = slider(json!({ "slider_steps": 5, "#slider_value": 1 }), vec![]);
    control.factory = Some(json_ui::Factory {
        name: Some("slider_step_factory".to_owned()),
        control_ids: [
            ("slider_step", "common.slider_step"),
            ("slider_step_progress", "common.slider_step_progress"),
        ]
        .into_iter()
        .map(|(id, reference)| (id.to_owned(), json_ui::ControlRef::parse(reference, "")))
        .collect(),
        ..Default::default()
    });
    let bound = bind(&page(vec![control]), &DataSource::new(), &Steps);
    let slider = &bound.children[0];
    let names: Vec<&str> = slider
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "slider_step",
            "slider_step_progress",
            "slider_step_progress"
        ]
    );
}

// T8: a toggle's component writes its grid collection name into its bag.
#[test]
fn toggles_publish_their_grid_collection_name() {
    let screen = Screen::new(page(vec![toggle(
        "t",
        json!({ "toggle_grid_collection_name": "rows" }),
    )]));
    let bound = screen.bound();
    assert_eq!(
        bound.children[0].properties["#collection_name"],
        json!("rows")
    );
}

// Anonymous array entries (vanilla `"@ns.radio_with_label"` items) share a name, not hover.
#[test]
fn anonymous_siblings_hover_and_click_independently() {
    let row = |index: usize| {
        let mut core = toggle(
            "core",
            json!({
                "toggle_name": format!("radio_{index}"),
                "checked_control": format!("on_{index}"),
                "unchecked_control": format!("off_{index}"),
                "unchecked_hover_control": format!("hover_{index}"),
            }),
        );
        core.children = ["on", "off", "hover"]
            .iter()
            .map(|state| ctrl(&format!("{state}_{index}"), "panel", json!({}), vec![]))
            .collect();
        ctrl(
            "",
            "panel",
            json!({ "size": [20, 20], "offset": [0, 30 * index] }),
            vec![core],
        )
    };
    let mut screen = Screen::new(page(vec![row(0), row(1)]));
    let regions = screen.regions();
    assert_ne!(regions[0].key, regions[1].key);
    screen.hover([5.0, 5.0]);
    assert!(screen.visible("hover_0") && !screen.visible("hover_1"));
    assert!(screen.visible("off_1"));
    let events = screen.click([5.0, 35.0]);
    assert!(events.iter().any(|event| matches!(
        event,
        ScreenEvent::Toggle { name, checked: true, .. } if name == "radio_1"
    )));
}

#[test]
fn review_disabled_edit_boxes_and_toggles_reject_nonbutton_input() {
    let mut screen = Screen::new(page(vec![edit_box(
        json!({"enabled":false, "always_listening":true}),
    )]));
    let regions = screen.regions();
    let typed = screen
        .dispatcher
        .text(&regions, &mut screen.view, "x", None);
    assert!(!typed.consumed && typed.events.is_empty());
    let mut screen = Screen::new(page(vec![toggle(
        "t",
        json!({"enabled":false, "enable_directional_toggling":true}),
    )]));
    screen.view.focused = Some("/root/t".into());
    let regions = screen.regions();
    let directed = screen
        .dispatcher
        .direction(&regions, &mut screen.view, [1.0, 0.0], 0.0);
    assert!(!directed.consumed && directed.events.is_empty());
}

#[test]
fn review_slider_release_outside_its_region_clears_pointer_capture() {
    let mut screen = Screen::new(page(vec![slider(
        json!({"button_mappings":[{"from_button_id":"button.menu_select", "to_button_id":"button.slider_track", "mapping_type":"pressed"}]}),
        vec![],
    )]));
    screen.hover([5.0, 5.0]);
    screen.press("button.menu_select", true, [5.0, 5.0], 0.0);
    screen.press("button.menu_select", false, [150.0, 150.0], 0.1);
    let regions = screen.regions();
    let input = PointerInput {
        point: Some([75.0, 5.0]),
        held: true,
        mode: InputMode::Mouse,
        now: 0.2,
    };
    let later = screen.dispatcher.pointer(&regions, &mut screen.view, input);
    assert!(slider_values(&later.events).is_empty());
}

// A screen controller sets an edit box's text by its text_box_name: the text target shows it,
// cut to max_length, the placeholder hides, and no edit event fires.
#[test]
fn host_sets_edit_box_text_by_name() {
    let mut screen = Screen::new(page(vec![edit_box(json!({}))]));
    let regions = screen.regions();
    let set = screen
        .dispatcher
        .set_edit_text(&regions, &mut screen.view, "name", "stonecutter");
    assert!(set.events.is_empty(), "{:?}", set.events);
    assert_eq!(text_of(&screen, "display"), "stone");
    assert!(!screen.visible("hint"));
    let other = screen
        .dispatcher
        .set_edit_text(&regions, &mut screen.view, "other", "x");
    assert!(other.events.is_empty());
    assert_eq!(text_of(&screen, "display"), "stone");
    // Typing continues from the host's text.
    screen.click([5.0, 5.0]);
    let regions = screen.regions();
    screen
        .dispatcher
        .text(&regions, &mut screen.view, "\u{8}", None);
    assert_eq!(text_of(&screen, "display"), "ston");
}
