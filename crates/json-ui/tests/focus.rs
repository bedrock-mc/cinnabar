//! Focus navigation over laid-out synthetic trees: admission, default focus,
//! identifier overrides, the directional sweep, wrapping, focus mappings,
//! containers, modal panels, and controller-direction claims.

use std::collections::BTreeMap;

use json_ui::{
    FocusDirection, FocusMove, HitRegion, LayoutEnv, ResolvedControl, TextMeasure, TextureMeta,
    TextureSource, ViewState, default_focus, focus_order, hit_regions, layout_with, navigate,
    set_focus,
};
use serde_json::{Value, json};

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

const SCREEN: [f64; 2] = [200.0, 200.0];

fn ctrl(name: &str, kind: &str, props: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let mut properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    properties
        .entry("anchor_from".to_owned())
        .or_insert(json!("top_left"));
    properties
        .entry("anchor_to".to_owned())
        .or_insert(json!("top_left"));
    ResolvedControl {
        name: name.to_owned(),
        control_type: Some(kind.to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children,
        factory: None,
    }
}

/// A focusable 20×20 button at `offset` with extra properties.
fn button(name: &str, offset: [f64; 2], extra: Value) -> ResolvedControl {
    let mut props = json!({ "size": [20, 20], "offset": offset, "focus_enabled": true });
    if let (Value::Object(props), Value::Object(extra)) = (&mut props, extra) {
        props.extend(extra);
    }
    ctrl(name, "button", props, vec![])
}

fn regions(children: Vec<ResolvedControl>) -> Vec<HitRegion> {
    let root = ctrl("root", "panel", json!({ "size": SCREEN }), children);
    let env = LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    };
    let (laid, _) = layout_with(&root, SCREEN, &env, &ViewState::default());
    hit_regions(&laid)
}

fn screen() -> json_ui::RectOut {
    json_ui::RectOut {
        x: 0.0,
        y: 0.0,
        w: SCREEN[0],
        h: SCREEN[1],
    }
}

fn focused(key: &str) -> ViewState {
    ViewState {
        focused: Some(key.to_owned()),
        ..ViewState::default()
    }
}

fn step(hits: &[HitRegion], from: &str, direction: FocusDirection) -> FocusMove {
    navigate(hits, &focused(from), direction, screen())
}

fn to(key: &str) -> FocusMove {
    FocusMove::Moved(key.to_owned())
}

fn names(hits: &[HitRegion]) -> Vec<&str> {
    focus_order(hits)
        .into_iter()
        .map(|region| region.name.as_str())
        .collect()
}

// F1: focus admission follows the focus component's enabled flag, not the type.
#[test]
fn focus_enabled_decides_admission() {
    let hits = regions(vec![
        button("excluded", [0.0, 0.0], json!({ "focus_enabled": false })),
        button("preferred", [30.0, 0.0], json!({})),
        ctrl(
            "reader",
            "input_panel",
            json!({ "size": [20, 20], "offset": [60, 0], "focus_enabled": true }),
            vec![],
        ),
        ctrl(
            "plain",
            "button",
            json!({ "size": [20, 20], "offset": [90, 0] }),
            vec![],
        ),
        button("bound", [120.0, 0.0], json!({ "#focus_enabled": false })),
    ]);
    assert_eq!(names(&hits), ["preferred", "reader"]);
}

// F2: default focus takes the highest precedence, not document order.
#[test]
fn default_focus_follows_precedence() {
    let hits = regions(vec![
        button("first", [0.0, 0.0], json!({})),
        button(
            "preferred",
            [0.0, 50.0],
            json!({ "default_focus_precedence": 10 }),
        ),
    ]);
    let pick = default_focus(&hits, SCREEN[0]).expect("default focus");
    assert_eq!(pick.name, "preferred");
    assert_eq!(
        navigate(&hits, &ViewState::default(), FocusDirection::Down, screen()),
        to(&pick.key)
    );
}

// F2: equal precedence falls back to reading order (top row, then left).
#[test]
fn default_focus_ties_in_reading_order() {
    let hits = regions(vec![
        button("low_left", [0.0, 50.0], json!({})),
        button("top_right", [100.0, 0.0], json!({})),
    ]);
    assert_eq!(default_focus(&hits, SCREEN[0]).unwrap().name, "top_right");
}

fn routed() -> Vec<HitRegion> {
    regions(vec![
        button(
            "a",
            [0.0, 0.0],
            json!({ "focus_identifier": "a", "focus_change_right": "right", "focus_change_down": "FOCUS_OVERRIDE_STOP" }),
        ),
        button("below", [0.0, 50.0], json!({})),
        button(
            "right",
            [120.0, 0.0],
            json!({ "focus_identifier": "right" }),
        ),
        button("near", [40.0, 0.0], json!({})),
    ])
}

// F3/F4: an identifier override wins over geometry; FOCUS_OVERRIDE_STOP pins focus.
#[test]
fn identifier_overrides_route_and_stop() {
    let hits = routed();
    assert_eq!(
        step(&hits, "/root/a", FocusDirection::Right),
        to("/root/right")
    );
    assert_eq!(
        step(&hits, "/root/a", FocusDirection::Down),
        FocusMove::Stayed
    );
}

// F5: without overrides, cardinal moves pick the nearest control ahead.
#[test]
fn cardinal_moves_sweep_geometrically() {
    let hits = routed();
    assert_eq!(
        step(&hits, "/root/below", FocusDirection::Up),
        to("/root/a")
    );
    assert_eq!(
        step(&hits, "/root/near", FocusDirection::Left),
        to("/root/a")
    );
    assert_eq!(
        step(&hits, "/root/near", FocusDirection::Right),
        to("/root/right")
    );
    // Nothing lies above the top row, so Up wraps to the bottom-most control.
    assert_eq!(
        step(&hits, "/root/near", FocusDirection::Up),
        to("/root/below")
    );
}

// F6: focus_wrap_enabled false stops at the edge instead of wrapping.
#[test]
fn disabled_wrap_stops_at_the_edge() {
    let wrapping = regions(vec![
        button("top", [0.0, 0.0], json!({})),
        button("bottom", [0.0, 50.0], json!({})),
    ]);
    assert_eq!(
        step(&wrapping, "/root/bottom", FocusDirection::Down),
        to("/root/top")
    );
    let pinned = regions(vec![
        button("top", [0.0, 0.0], json!({})),
        button(
            "bottom",
            [0.0, 50.0],
            json!({ "focus_wrap_enabled": false }),
        ),
    ]);
    assert_eq!(
        step(&pinned, "/root/bottom", FocusDirection::Down),
        FocusMove::Stayed
    );
}

// F7: the magnet flag is read onto the focus component.
#[test]
fn magnet_flag_is_read() {
    let hits = regions(vec![button(
        "m",
        [0.0, 0.0],
        json!({ "focus_magnet_enabled": true }),
    )]);
    assert!(hits[0].focus.as_ref().unwrap().magnet);
}

// F8: reset_on_focus_lost false keeps the old control's hover look after focus leaves.
#[test]
fn focus_loss_resets_unless_disabled() {
    let hits = regions(vec![
        button("keep", [0.0, 0.0], json!({ "reset_on_focus_lost": false })),
        button("reset", [0.0, 50.0], json!({})),
        button("other", [0.0, 100.0], json!({})),
    ]);
    let mut state = focused("/root/keep");
    set_focus(&mut state, &hits, Some("/root/reset".to_owned()));
    assert!(state.is_hovered("/root/keep"));
    set_focus(&mut state, &hits, Some("/root/other".to_owned()));
    assert!(!state.is_hovered("/root/reset"));
    assert!(!state.is_hovered("/root/keep"));
}

// F9: a focus_mapping entry supplies the identified control's override.
#[test]
fn focus_mapping_supplies_overrides() {
    let hits = regions(vec![
        button(
            "slot",
            [0.0, 0.0],
            json!({
                "focus_identifier": "slot8",
                "focus_mapping": [{ "focus_identifier": "slot8", "focus_change_right": "FOCUS_OVERRIDE_STOP" }]
            }),
        ),
        button("next", [40.0, 0.0], json!({})),
    ]);
    assert_eq!(
        step(&hits, "/root/slot", FocusDirection::Right),
        FocusMove::Stayed
    );
    assert_eq!(
        step(&hits, "/root/next", FocusDirection::Left),
        to("/root/slot")
    );
}

fn container(
    name: &str,
    offset: [f64; 2],
    props: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let mut all = json!({ "size": [60, 60], "offset": offset, "focus_container": true });
    if let (Value::Object(all), Value::Object(props)) = (&mut all, props) {
        all.extend(props);
    }
    ctrl(name, "panel", all, children)
}

fn panels(left: Value) -> Vec<HitRegion> {
    regions(vec![
        container(
            "left_panel",
            [0.0, 0.0],
            left,
            vec![
                button("l1", [0.0, 0.0], json!({})),
                button("l2", [30.0, 0.0], json!({})),
            ],
        ),
        container(
            "right_panel",
            [100.0, 0.0],
            json!({ "use_last_focus": true }),
            vec![
                button("r1", [0.0, 0.0], json!({})),
                button(
                    "target",
                    [0.0, 30.0],
                    json!({ "focus_identifier": "target" }),
                ),
            ],
        ),
    ])
}

// F10/F12: stop keeps focus in the container; contained wraps inside it; none leaves.
#[test]
fn navigation_modes_govern_leaving_a_container() {
    let from = "/root/left_panel/l2";
    let free = panels(json!({}));
    assert_eq!(
        step(&free, from, FocusDirection::Right),
        to("/root/right_panel/r1")
    );
    let none = panels(json!({ "focus_navigation_mode_right": "none" }));
    assert_eq!(
        step(&none, from, FocusDirection::Right),
        to("/root/right_panel/r1")
    );
    let stop = panels(json!({ "focus_navigation_mode_right": "stop" }));
    assert_eq!(step(&stop, from, FocusDirection::Right), FocusMove::Stayed);
    let contained = panels(json!({ "focus_navigation_mode_right": "contained" }));
    assert_eq!(
        step(&contained, from, FocusDirection::Right),
        to("/root/left_panel/l1")
    );
}

// F13: a custom side enters the named container at focus_id_inside, else nearest.
#[test]
fn custom_routes_enter_named_containers() {
    let from = "/root/left_panel/l2";
    let inside = panels(json!({
        "focus_navigation_mode_right": "custom",
        "focus_container_custom_right": [{ "other_focus_container_name": "right_panel", "focus_id_inside": "target" }]
    }));
    assert_eq!(
        step(&inside, from, FocusDirection::Right),
        to("/root/right_panel/target")
    );
    let nearest = panels(json!({
        "focus_navigation_mode_right": "custom",
        "focus_container_custom_right": [{ "other_focus_container_name": "right_panel" }]
    }));
    assert_eq!(
        step(&nearest, from, FocusDirection::Right),
        to("/root/right_panel/r1")
    );
}

// F11: re-entering a use_last_focus container restores its last focused control.
#[test]
fn use_last_focus_restores_the_container_memory() {
    let hits = panels(json!({}));
    let mut state = focused("/root/right_panel/r1");
    set_focus(
        &mut state,
        &hits,
        Some("/root/right_panel/target".to_owned()),
    );
    set_focus(&mut state, &hits, Some("/root/left_panel/l2".to_owned()));
    assert_eq!(
        navigate(&hits, &state, FocusDirection::Right, screen()),
        to("/root/right_panel/target")
    );
}

// X1: an open modal panel limits focus to its own subtree.
#[test]
fn modal_panel_limits_focus() {
    let hits = regions(vec![
        button("behind", [0.0, 0.0], json!({})),
        ctrl(
            "dialog",
            "input_panel",
            json!({ "size": [100, 100], "offset": [50, 50], "modal": true, "layer": 5 }),
            vec![button("inside", [10.0, 10.0], json!({}))],
        ),
    ]);
    assert_eq!(names(&hits), ["inside"]);
}

// X5: always_handle_controller_direction claims the direction instead of moving focus.
#[test]
fn controller_direction_can_be_claimed() {
    let hits = regions(vec![
        button(
            "slider_like",
            [0.0, 0.0],
            json!({ "always_handle_controller_direction": true }),
        ),
        button("other", [0.0, 50.0], json!({})),
    ]);
    assert_eq!(
        step(&hits, "/root/slider_like", FocusDirection::Down),
        FocusMove::Claimed("/root/slider_like".to_owned())
    );
}

#[test]
fn review_custom_container_exit_runs_without_a_spatial_sweep_target() {
    let hits = regions(vec![
        container(
            "left",
            [0.0, 0.0],
            json!({"focus_navigation_mode_left":"custom", "focus_container_custom_left":[{"other_focus_container_name":"right", "focus_id_inside":"target"}]}),
            vec![button(
                "source",
                [0.0, 0.0],
                json!({"focus_wrap_enabled":false}),
            )],
        ),
        container(
            "right",
            [100.0, 0.0],
            json!({}),
            vec![button(
                "target",
                [0.0, 0.0],
                json!({"focus_identifier":"target"}),
            )],
        ),
    ]);
    assert_eq!(
        step(&hits, "/root/left/source", FocusDirection::Left),
        to("/root/right/target")
    );
}
