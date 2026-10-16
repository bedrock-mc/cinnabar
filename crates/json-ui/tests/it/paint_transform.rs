//! Cinnabar paint transforms: scale and rotation that move draws, clips and hits without
//! changing layout, honoured only in catalogs that opt in.

use json_ui::{
    BindState, Catalog, Context, DataSource, DrawNode, HitRegion, LayoutEnv, Scalar, TextMeasure,
    TextureMeta, TextureSource, ViewState, bind_screen, hit_test, render_bound, resolve,
    transform::Affine,
};
use serde_json::json;

struct MonoText;
impl TextMeasure for MonoText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

struct Plain;
impl TextureSource for Plain {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta::plain([16.0, 16.0]))
    }
}

/// A 40x20 card at (10, 10) doubled from its top-left corner, holding a solid fill, a
/// button, and a clipping panel whose child spills past it, with a bound rotation.
fn document() -> String {
    let corner = json!({"anchor_from":"top_left","anchor_to":"top_left"});
    let at = |size: [f64; 2], offset: [f64; 2], extra: serde_json::Value| {
        let mut control = corner.clone();
        control["size"] = json!(size);
        control["offset"] = json!(offset);
        for (key, value) in extra.as_object().unwrap() {
            control[key] = value.clone();
        }
        control
    };
    json!({"namespace":"t","root":{"type":"screen","size":["100%","100%"],"controls":[
        {"card": at([40.0, 20.0], [10.0, 10.0], json!({"type":"panel",
            "cinnabar_transform":{"scale":2,"origin":[0,0]},
            "bindings":[{"binding_name":"#turn","binding_name_override":"#cinnabar_rotation",
                "binding_type":"global"}],
            "controls":[
                {"fill": at([10.0, 10.0], [0.0, 0.0], json!({"type":"image","color":[1,1,1]}))},
                {"press": at([10.0, 10.0], [20.0, 0.0], json!({"type":"button",
                    "button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":"t.press","mapping_type":"pressed"}]}))},
                {"window": at([10.0, 10.0], [0.0, 10.0], json!({"type":"panel",
                    "clips_children":true,"controls":[
                        {"spill": at([30.0, 5.0], [0.0, 0.0], json!({"type":"image",
                            "color":[1,0,0]}))}]}))}
            ]}))}
    ]}})
    .to_string()
}

/// The screen's draws and hits, with `#turn` set to `turn` degrees.
fn render(transforms: bool, turn: f64) -> (Vec<DrawNode>, Vec<HitRegion>) {
    let document = document();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &br#"{"ui_defs":["ui/t.json"]}"#[..]),
        ("ui/t.json", document.as_bytes()),
    ])
    .unwrap();
    let catalog = if transforms {
        catalog.with_paint_transforms()
    } else {
        catalog
    };
    let mut data = DataSource::new();
    data.set_global("#turn", Scalar::Num(turn));
    let env = LayoutEnv {
        text: &MonoText,
        textures: &Plain,
    };
    let context = Context::default();
    let root = resolve(&catalog, "t.root", &context).control.unwrap();
    let bound = bind_screen(&root, &catalog, &context, &data, &mut BindState::new());
    let render = render_bound(bound, [200.0, 100.0], &env, &ViewState::default());
    (render.nodes.to_vec(), render.hits.to_vec())
}

fn transform_of(nodes: &[DrawNode], name: &str) -> Option<Affine> {
    nodes
        .iter()
        .find(|node| node.name == name)
        .unwrap()
        .anim
        .as_ref()
        .and_then(|anim| anim.transform)
}

fn close(left: [f64; 2], right: [f64; 2]) -> bool {
    (left[0] - right[0]).abs() < 1e-9 && (left[1] - right[1]).abs() < 1e-9
}

#[test]
fn transforms_leave_layout_and_are_ignored_outside_opted_in_catalogs() {
    let (plain, plain_hits) = render(false, 0.0);
    let (scaled, scaled_hits) = render(true, 0.0);
    let dests = |nodes: &[DrawNode]| nodes.iter().map(|node| node.dest).collect::<Vec<_>>();
    assert_eq!(dests(&plain), dests(&scaled));
    assert_eq!(plain_hits[0].rect, scaled_hits[0].rect);
    assert!(plain.iter().all(|node| node.anim.is_none()));
    assert_eq!(plain_hits[0].transform, None);
    // Without the declared property a bound rotation has nothing to write.
    let (turned, turned_hits) = render(false, 90.0);
    assert!(turned.iter().all(|node| node.anim.is_none()));
    assert_eq!(turned_hits[0].transform, None);
    let doubled = transform_of(&scaled, "fill").unwrap();
    assert!(close(doubled.apply([20.0, 20.0]), [30.0, 30.0]));
}

#[test]
fn hits_follow_the_drawn_button() {
    let (_, hits) = render(true, 0.0);
    // The button lays out at (30, 10)-(40, 20) and draws at (50, 10)-(70, 30).
    assert_eq!(hit_test(&hits, [65.0, 25.0]).unwrap().name, "press");
    assert!(hit_test(&hits, [35.0, 15.0]).is_none());
    let (_, plain) = render(false, 0.0);
    assert_eq!(hit_test(&plain, [35.0, 15.0]).unwrap().name, "press");
}

#[test]
fn bound_rotation_composes_with_the_authored_scale() {
    let (nodes, hits) = render(true, 90.0);
    let world = transform_of(&nodes, "fill").unwrap();
    // A clockwise quarter turn and doubling about the card's corner.
    assert!(close(world.apply([20.0, 10.0]), [10.0, 30.0]));
    // The button's layout centre (35, 15) turns to (0, 60) around (10, 10); its drawn box
    // spans x -10..10 and y 50..70, half of it left of the screen.
    assert!(close(world.apply([35.0, 15.0]), [0.0, 60.0]));
    assert_eq!(hit_test(&hits, [5.0, 55.0]).unwrap().name, "press");
    assert!(hit_test(&hits, [65.0, 25.0]).is_none());
}

#[test]
fn clips_inside_a_scaled_control_scale_with_it() {
    let (nodes, _) = render(true, 0.0);
    let spill = nodes.iter().find(|node| node.name == "spill").unwrap();
    // The window lays out at (10, 20)-(20, 30) and draws doubled from (10, 10).
    assert_eq!(
        [spill.clip.x, spill.clip.y, spill.clip.w, spill.clip.h],
        [10.0, 30.0, 20.0, 20.0]
    );
    let (plain, _) = render(false, 0.0);
    let spill = plain.iter().find(|node| node.name == "spill").unwrap();
    assert_eq!(
        [spill.clip.x, spill.clip.y, spill.clip.w, spill.clip.h],
        [10.0, 20.0, 10.0, 10.0]
    );
}
