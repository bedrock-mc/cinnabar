//! Surface paint transforms end to end: drawn pixels, presses and slider drags follow the
//! scaled or rotated control, and transitions animate the bound scale.

use std::collections::BTreeMap;

use mod_ui::PanelUiOwner;
use serde_json::{Value, json};
use ui::{
    DpiScale,
    mod_panel::{Event, Panel, Surface, SurfaceTransition, SurfaceValue, TransitionEasing},
};

use super::super::super::snapshot;
use super::mod_panel::frame;
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::test_support::sample_panel as panel;

const GREEN: [u8; 4] = [0, 255, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

/// A filled 40x20 button at (100, 80) routing `action`, scaled by `#grow` about `origin`,
/// and a red 40x4 bar centred at (320, 100) turned by `#turn` degrees.
fn transformed_panel(
    action: &str,
    origin: [f64; 2],
    bindings: [f64; 2],
    transition: Option<SurfaceTransition>,
) -> Panel {
    let fill = |color: [f64; 4]| {
        json!({"fill":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "size":["100%","100%"],"radius":0,"color":color}})
    };
    let placed = |size: [f64; 2], offset: [f64; 2], extra: Value| {
        let mut control = json!({"size":size,"offset":offset,
            "anchor_from":"top_left","anchor_to":"top_left"});
        control
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        control
    };
    let mut panel = panel();
    panel.motion = true;
    panel.surface = Some(Surface { collections: Default::default(), previews: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[
                {"grown": placed([40.0, 20.0], [100.0, 80.0], json!({"type":"button",
                    "cinnabar_transform":{"origin":origin},
                    "bindings":[{"binding_name":"#grow","binding_name_override":"#cinnabar_scale"}],
                    "button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":action,"mapping_type":"pressed"}],
                    "controls":[fill([0.0, 1.0, 0.0, 1.0])]}))},
                {"bar": placed([40.0, 4.0], [300.0, 98.0], json!({"type":"panel",
                    "cinnabar_transform":{},
                    "bindings":[{"binding_name":"#turn","binding_name_override":"#cinnabar_rotation"}],
                    "controls":[fill([1.0, 0.0, 0.0, 1.0])]}))}
            ]
        }})
        .to_string(),
        bindings: BTreeMap::from([
            ("#grow".into(), SurfaceValue::Number(bindings[0])),
            ("#turn".into(), SurfaceValue::Number(bindings[1])),
        ]),
        transitions: transition
            .map(|transition| BTreeMap::from([("#grow".into(), transition)]))
            .unwrap_or_default(),
    });
    panel
}

/// Opens `panel` in a fresh presentation.
fn opened(panel: &Panel) -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    presentation
}

/// The screen pixel at surface layout point `at` of the last frame.
fn screen(presentation: &UiPresentationRuntime, at: [f64; 2]) -> [f32; 2] {
    let frame = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap();
    [
        frame.origin[0] + at[0] as f32 * frame.scale,
        frame.origin[1] + at[1] as f32 * frame.scale,
    ]
}

/// The colour drawn at surface layout point `at` in a fresh 1280x720 frame.
fn pixel(presentation: &mut UiPresentationRuntime, at: [f64; 2]) -> [u8; 4] {
    let rendered = frame(presentation, [1280, 720]);
    let [x, y] = screen(presentation, at);
    snapshot::rasterize(&rendered)
        .get_pixel(x as u32, y as u32)
        .0
}

/// Whether a press at surface layout point `at` reaches the sample panel's button.
fn presses(presentation: &mut UiPresentationRuntime, at: [f64; 2]) -> bool {
    let point = screen(presentation, at);
    let events = presentation
        .mod_ui_mut()
        .mod_panel_events(point, true, true);
    presentation
        .mod_ui_mut()
        .mod_panel_events(point, false, false);
    events
        == [Event {
            id: "binding".into(),
            value: 1.0,
        }]
}

#[test]
fn scaled_buttons_draw_and_take_presses_where_they_appear() {
    // Outside the 40x20 layout box but inside its doubled drawing about the centre.
    let beside = [95.0, 75.0];
    let mut plain = opened(&transformed_panel(
        "mod.control:3",
        [0.5; 2],
        [1.0, 0.0],
        None,
    ));
    assert_eq!(pixel(&mut plain, [120.0, 90.0]), GREEN);
    assert_ne!(pixel(&mut plain, beside), GREEN);
    assert!(!presses(&mut plain, beside));

    let mut grown = opened(&transformed_panel(
        "mod.control:3",
        [0.5; 2],
        [2.0, 0.0],
        None,
    ));
    assert_eq!(pixel(&mut grown, beside), GREEN);
    assert!(presses(&mut grown, beside));
    // Layout is unchanged: the hit region keeps its authored box.
    let state = grown.mod_ui().mod_panel_state().unwrap();
    let hit = state
        .frame
        .unwrap()
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("mod.control:3"))
        .unwrap()
        .rect;
    assert_eq!([hit.x, hit.y, hit.w, hit.h], [100.0, 80.0, 40.0, 20.0]);
}

#[test]
fn rotated_content_draws_turned_about_its_centre() {
    // The bar spans y 98..102; a quarter turn stands it upright across y 80..120.
    let below = [320.0, 112.0];
    let mut flat = opened(&transformed_panel(
        "mod.control:3",
        [0.5; 2],
        [1.0, 0.0],
        None,
    ));
    assert_eq!(pixel(&mut flat, [330.0, 100.0]), RED);
    assert_ne!(pixel(&mut flat, below), RED);
    let mut turned = opened(&transformed_panel(
        "mod.control:3",
        [0.5; 2],
        [1.0, 90.0],
        None,
    ));
    assert_eq!(pixel(&mut turned, below), RED);
    assert_ne!(pixel(&mut turned, [332.0, 100.0]), RED);
}

#[test]
fn slider_drags_read_the_fraction_along_the_drawn_control() {
    // Doubled from its left edge, three quarters of the drawn width is the layout's 75%.
    let mut presentation = opened(&transformed_panel(
        "mod.control:1",
        [0.0, 0.5],
        [2.0, 0.0],
        None,
    ));
    frame(&mut presentation, [1280, 720]);
    let point = screen(&presentation, [160.0, 90.0]);
    let events = presentation
        .mod_ui_mut()
        .mod_panel_events(point, true, true);
    assert_eq!(
        events,
        [Event {
            id: "strength".into(),
            value: 75.0
        }]
    );
}

#[test]
fn transitions_animate_a_bound_scale_from_its_opening_value() {
    let transition = SurfaceTransition {
        duration: 0.25,
        easing: TransitionEasing::default(),
        from: Some(SurfaceValue::Number(0.5)),
        exit: None,
    };
    let mut presentation = opened(&transformed_panel(
        "mod.control:3",
        [0.5; 2],
        [1.0, 0.0],
        Some(transition),
    ));
    let build = |presentation: &mut UiPresentationRuntime, millis| {
        presentation
            .build(
                &player_state::PlayerState::new(1),
                &UiRuntime::new(1),
                millis,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    // Near the left edge: outside the half-size opening frame, inside once settled.
    let edge = [103.0, 90.0];
    let opening = build(&mut presentation, 1_000);
    let [x, y] = screen(&presentation, edge);
    assert_ne!(
        snapshot::rasterize(&opening)
            .get_pixel(x as u32, y as u32)
            .0,
        GREEN
    );
    assert!(!presses(&mut presentation, edge));
    assert!(presentation.mod_ui().mod_panel_motion_active());
    let settled = build(&mut presentation, 1_250);
    assert_eq!(
        snapshot::rasterize(&settled)
            .get_pixel(x as u32, y as u32)
            .0,
        GREEN
    );
    assert!(!presentation.mod_ui().mod_panel_motion_active());
    assert!(presses(&mut presentation, edge));
}

/// Adds a declared transform whose `target` field reads `binding` to `control`.
fn bound_transform(mut control: Value, binding: &str, target: &str) -> Value {
    let object = control.as_object_mut().unwrap();
    object.insert("cinnabar_transform".into(), json!({}));
    object.insert(
        "bindings".into(),
        json!([{"binding_name":binding,"binding_name_override":target}]),
    );
    control
}

/// A small mod menu card with an entrance scale, a pressed row, a hovered knob, a turned
/// chevron and a turned tile, bound in that order to `values`.
fn menu_panel(values: [f64; 5]) -> Panel {
    let corner = |mut control: Value, size: [f64; 2], offset: [f64; 2]| {
        let object = control.as_object_mut().unwrap();
        object.insert("size".into(), json!(size));
        object.insert("offset".into(), json!(offset));
        object.insert("anchor_from".into(), json!("top_left"));
        object.insert("anchor_to".into(), json!("top_left"));
        control
    };
    let rounded = |size: [f64; 2], radius: f64, color: [f64; 4]| {
        corner(
            json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
                "radius":radius,"color":color}),
            size,
            [0.0; 2],
        )
    };
    let label = |text: &str, offset: [f64; 2]| {
        corner(
            json!({"type":"label","text":text,"color":[0.9,0.92,0.95],
                "font_type":ui::mod_panel::FONT_NAME}),
            [120.0, 10.0],
            offset,
        )
    };
    let row = |index: usize| {
        corner(
            json!({"type":"panel","controls":[
                {"background":rounded([196.0, 26.0], 6.0, [0.18, 0.2, 0.25, 1.0])},
                {"label":label(&format!("Module {}", index + 1), [10.0, 8.0])}
            ]}),
            [196.0, 26.0],
            [12.0, 40.0 + index as f64 * 32.0],
        )
    };
    let knob = corner(
        json!({"type":"panel","controls":[
            {"dot":rounded([14.0, 14.0], 7.0, [0.45, 0.75, 1.0, 1.0])}]}),
        [14.0, 14.0],
        [180.0, 114.0],
    );
    let chevron = corner(
        json!({"type":"custom","renderer":"cinnabar_vector_path","view_box":[0,0,12,12],
            "stroke_width":2,"closed":false,"paths":[[[3,4],[6,7],[9,4]]],
            "color":[0.9,0.92,0.95,1]}),
        [12.0, 12.0],
        [192.0, 12.0],
    );
    let tile = corner(
        json!({"type":"panel","controls":[
            {"square":rounded([48.0, 48.0], 6.0, [1.0, 0.55, 0.2, 1.0])}]}),
        [48.0, 48.0],
        [290.0, 80.0],
    );
    let card = corner(
        json!({"type":"panel","controls":[
            {"background":rounded([220.0, 150.0], 10.0, [0.08, 0.09, 0.12, 0.96])},
            {"title":label("Combat", [12.0, 14.0])},
            {"chevron":bound_transform(chevron, "#chevron", "#cinnabar_rotation")},
            {"row_0":row(0)},
            {"row_1":bound_transform(row(1), "#press", "#cinnabar_scale")},
            {"row_2":row(2)},
            {"knob":bound_transform(knob, "#hover", "#cinnabar_scale")}
        ]}),
        [220.0, 150.0],
        [40.0, 40.0],
    );
    let mut panel = panel();
    let names = ["#entrance", "#press", "#hover", "#chevron", "#spin"];
    panel.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[
                {"card":bound_transform(card, "#entrance", "#cinnabar_scale")},
                {"tile":bound_transform(tile, "#spin", "#cinnabar_rotation")}
            ]
        }})
        .to_string(),
        bindings: names
            .iter()
            .zip(values)
            .map(|(name, value)| ((*name).into(), SurfaceValue::Number(value)))
            .collect(),
        transitions: BTreeMap::new(),
    });
    panel
}

#[test]
fn a_transformed_menu_draws_differently_from_its_identity_values() {
    let mut before = opened(&menu_panel([1.0, 1.0, 1.0, 0.0, 0.0]));
    let before = frame(&mut before, [1280, 720]);
    let mut after = opened(&menu_panel([0.97, 0.975, 1.23, 45.0, 30.0]));
    let after = frame(&mut after, [1280, 720]);
    snapshot::write(&before, "mod-surface-transforms-before");
    snapshot::write(&after, "mod-surface-transforms-after");
    assert!(snapshot::rasterize(&before) != snapshot::rasterize(&after));
}
