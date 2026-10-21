//! `@button` states: component-set variants, named children and their
//! copies, drawn through the engine's interaction state machinery.

use std::collections::BTreeMap;

use json_ui::Draw;
use jsonui_editor::Frame;
use jsonui_editor::figma::{self, Node};
use serde_json::json;

use super::{control, fixture, laid_out, messages, pixel, rect, ui};

// A `@button` component set, or an instance of one, takes its states from the
// `State` variants (Disabled meaning locked); an instance keeps its other
// variant properties, and the button takes the default variant's size.
#[test]
fn state_variants_become_button_states() {
    let conversion = figma::convert(&fixture("variants"), &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Variant Test / Menu Button @button=play / State=Focused".to_owned(),
            "variant state `Focused` is not a button state".to_owned(),
        )],
        "the set's own stroke is canvas chrome"
    );
    let ui = ui(&conversion);
    let menu = control(&ui, "menu_button");
    assert_eq!(menu["type"], "button");
    assert_eq!(menu["button_mappings"][0]["to_button_id"], "button.play");
    for (property, state) in [
        ("default_control", "default"),
        ("hover_control", "hover"),
        ("pressed_control", "pressed"),
        ("locked_control", "locked"),
    ] {
        assert_eq!(menu[property], state, "{property}");
    }
    assert_eq!(menu["size"], json!([80, 20]));
    let states: Vec<&String> = menu["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|control| control.as_object()?.keys().next())
        .collect();
    assert_eq!(
        states,
        ["default", "hover", "pressed", "locked"],
        "pressed copies default"
    );
    assert_eq!(control(menu, "default")["size"], json!(["100%", "100%"]));
    assert_eq!(control(control(menu, "default"), "label")["text"], "Play");
    let quit = control(&ui, "quit");
    assert_eq!(
        (&quit["default_control"], &quit["hover_control"]),
        (&json!("default"), &json!("hover"))
    );
    let background = |state: &str| &control(control(quit, state), "background")["color"];
    assert_eq!(background("default"), &json!([0.5, 0, 0]));
    assert_eq!(
        background("hover"),
        &json!([0.8, 0, 0]),
        "the Large hover variant"
    );
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "menu_button"), [0.0, 0.0, 80.0, 20.0]);
    assert_eq!(rect(&frame, "quit"), [100.0, 70.0, 80.0, 24.0]);
}

// A `@button` instance's variants already carry its opacity; it applies once.
#[test]
fn instance_opacity_applies_once_to_button_states() {
    let conversion = figma::convert(&fixture("variant_opacity"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    let [r, g, b, _] = pixel(&rgba, 100, 20, 10);
    assert!(
        (120..=136).contains(&r) && r == g && g == b,
        "white at 50% over black: {r} {g} {b}"
    );
}

/// Fixed-advance text and no textures, for laying out the bound tree directly.
struct Plain;

impl json_ui::TextMeasure for Plain {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 6.0, 9.0]
    }
}

impl json_ui::TextureSource for Plain {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Sprite tints drawn within the box named `name` under the interaction state
/// `state` makes from its key, through the engine's button state machinery.
fn tints(frame: &Frame, name: &str, state: impl Fn(String) -> json_ui::ViewState) -> Vec<[u8; 4]> {
    let laid = frame.boxes.iter().find(|laid| laid.name == name).unwrap();
    let env = json_ui::LayoutEnv {
        text: &Plain,
        textures: &Plain,
    };
    let (out, _) = json_ui::layout_with(&frame.bound, frame.root, &env, &state(laid.key.clone()));
    let [x, y, w, h] = laid.rect;
    json_ui::emit(&out, &env)
        .into_iter()
        .filter(|node| {
            node.dest.x >= x
                && node.dest.y >= y
                && node.dest.x + node.dest.w <= x + w
                && node.dest.y + node.dest.h <= y + h
        })
        .filter_map(|node| match node.draw {
            Draw::Sprite { color, .. } => Some(color),
            _ => None,
        })
        .collect()
}

fn rest(_: String) -> json_ui::ViewState {
    json_ui::ViewState::default()
}

fn hover(key: String) -> json_ui::ViewState {
    json_ui::ViewState {
        hovered: Some(key),
        ..Default::default()
    }
}

fn press(key: String) -> json_ui::ViewState {
    json_ui::ViewState {
        hovered: Some(key.clone()),
        pressed: Some(key),
        ..Default::default()
    }
}

// Buttons stay drawn when hovered and pressed: without states the content shows
// in every state, and a missing state shows the default's look.
#[test]
fn buttons_draw_in_every_interaction_state() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (_, _, frame) = laid_out(&conversion, 1);
    let grey = [51, 51, 51, 255];
    for state in [rest, hover, press] {
        assert_eq!(tints(&frame, "plain", state), [grey]);
    }
    let (dark, light) = ([102, 0, 0, 255], [204, 0, 0, 255]);
    assert_eq!(tints(&frame, "pair", rest), [dark]);
    assert_eq!(tints(&frame, "pair", hover), [light]);
    assert_eq!(
        tints(&frame, "pair", press),
        [dark],
        "no pressed state: the default look"
    );
}

// States nest inside an auto-layout button's generated stack and still switch.
#[test]
fn auto_layout_button_states_are_found_in_figma_children() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    let ui = ui(&conversion);
    let auto = control(&ui, "auto");
    assert_eq!(
        (&auto["default_control"], &auto["hover_control"]),
        (&json!("default"), &json!("hover"))
    );
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(tints(&frame, "auto", rest), [[0, 102, 0, 255]]);
    assert_eq!(tints(&frame, "auto", hover), [[0, 204, 0, 255]]);
}

/// Sprite tints and rects drawn within the box named `name` under `state`.
fn sprites(
    frame: &Frame,
    name: &str,
    state: impl Fn(String) -> json_ui::ViewState,
) -> Vec<([u8; 4], [f64; 4])> {
    let laid = frame.boxes.iter().find(|laid| laid.name == name).unwrap();
    let env = json_ui::LayoutEnv {
        text: &Plain,
        textures: &Plain,
    };
    let (out, _) = json_ui::layout_with(&frame.bound, frame.root, &env, &state(laid.key.clone()));
    json_ui::emit(&out, &env)
        .into_iter()
        .filter_map(|node| match node.draw {
            Draw::Sprite { color, .. } => {
                Some((color, [node.dest.x, node.dest.y, node.dest.w, node.dest.h]))
            }
            _ => None,
        })
        .filter(|(_, [x, ..])| *x >= laid.rect[0] && *x < laid.rect[0] + laid.rect[2])
        .collect()
}

// Every state of an auto-layout button overlays its flowing default's slot,
// including the pressed state copied from the default.
#[test]
fn button_states_overlay_the_default_slot() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    let (_, _, frame) = laid_out(&conversion, 1);
    let slot = [140.0, 0.0, 60.0, 20.0];
    let (dark, light) = ([0, 102, 0, 255], [0, 204, 0, 255]);
    assert_eq!(sprites(&frame, "auto", rest), [(dark, slot)]);
    assert_eq!(sprites(&frame, "auto", hover), [(light, slot)]);
    assert_eq!(sprites(&frame, "auto", press), [(dark, slot)]);
}

// A text `@button` sized to its text hugs the label, as in Figma, instead of
// filling its parent.
#[test]
fn text_buttons_hug_their_label() {
    let node: Node = serde_json::from_value(json!({
        "id": "20:1", "name": "Hug Test", "type": "FRAME", "width": 200, "height": 100,
        "children": [{
            "id": "20:2", "name": "Play @button", "type": "TEXT", "x": 10, "y": 10, "width": 24, "height": 9,
            "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": "Play", "font_family": "Minecraft" }
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let (_, _, frame) = laid_out(&conversion, 1);
    let label = rect(&frame, "content");
    assert_eq!(
        rect(&frame, "play"),
        label,
        "the button is the label's size"
    );
    assert!(label[2] < 100.0 && label[3] < 50.0, "{label:?}");
    assert_eq!(label[..2], [10.0, 10.0]);
}

// A missing state copied from the default keeps the default's directives.
#[test]
fn copied_states_keep_their_directives() {
    let node: Node = serde_json::from_value(json!({
        "id": "21:1", "name": "Copy Test", "type": "FRAME", "width": 100, "height": 40,
        "children": [{
            "id": "21:2", "name": "Go @button", "type": "FRAME", "width": 60, "height": 20,
            "children": [{
                "id": "21:3", "name": "Default $binding=#Title", "type": "TEXT", "width": 60, "height": 9,
                "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
                "text": { "characters": "Go", "font_family": "Minecraft" }
            }]
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let ui = ui(&conversion);
    let go = control(&ui, "go");
    for state in ["default", "hover", "pressed"] {
        assert_eq!(control(go, state)["text"], "#Title", "{state}");
    }
    laid_out(&conversion, 1);
}
