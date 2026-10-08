//! Control-rendering contracts at the emit boundary: which controls draw what,
//! with vanilla tints, clipping inheritance and renderer configuration.

use std::collections::BTreeMap;

use json_ui::{
    Draw, DrawNode, LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource, emit,
    layout,
};
use serde_json::{Value, json};

struct MonoText;
impl TextMeasure for MonoText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

/// Every texture is a plain 16x16 image.
struct Icons;
impl TextureSource for Icons {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        (path != "missing").then(|| TextureMeta::plain([16.0, 16.0]))
    }
}

fn ctrl(name: &str, kind: &str, props: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
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

fn draw(root: &ResolvedControl) -> Vec<DrawNode> {
    let env = LayoutEnv {
        text: &MonoText,
        textures: &Icons,
    };
    emit(&layout(root, [100.0, 100.0], &env), &env)
}

fn node<'a>(nodes: &'a [DrawNode], name: &str) -> &'a DrawNode {
    nodes.iter().find(|node| node.name == name).unwrap()
}

// A label with a texture still draws text; a panel's `fill` paints nothing.
#[test]
fn control_type_decides_what_draws() {
    let label = ctrl(
        "label",
        "label",
        json!({ "text": "Hello", "texture": "textures/ui/icon", "size": [30, 10] }),
        vec![],
    );
    assert!(matches!(&draw(&label)[0].draw, Draw::Text { text, .. } if text == "Hello"));
    let panel = ctrl(
        "panel",
        "panel",
        json!({ "size": [20, 20], "fill": true, "color": [1, 0, 0] }),
        vec![],
    );
    assert!(draw(&panel).is_empty());
}

// A `#color` binding target tints an image; vanilla names like `yellow` parse.
#[test]
fn image_tints_read_color_targets_and_names() {
    let tinted = ctrl(
        "image",
        "image",
        json!({ "texture": "textures/ui/icon", "#color": [1, 0, 0], "size": [16, 16] }),
        vec![],
    );
    assert!(
        matches!(&draw(&tinted)[0].draw, Draw::Sprite { color, .. } if *color == [255, 0, 0, 255])
    );
    let named = ctrl(
        "image",
        "image",
        json!({ "texture": "textures/ui/icon", "color": "yellow", "size": [16, 16] }),
        vec![],
    );
    assert!(
        matches!(&draw(&named)[0].draw, Draw::Sprite { color, .. } if *color == [255, 255, 0, 255])
    );
}

// `bilinear` and `grayscale` reach the sprite.
#[test]
fn sampling_flags_reach_the_sprite() {
    let image = ctrl(
        "image",
        "image",
        json!({ "texture": "textures/ui/icon", "bilinear": true, "grayscale": true, "size": [13, 13] }),
        vec![],
    );
    let Draw::Sprite { filter, .. } = &draw(&image)[0].draw else {
        panic!("not a sprite");
    };
    assert!(filter.bilinear && filter.grayscale);
}

// `allow_clipping: false` escapes a clipping parent; `clip_offset` insets the clip on both sides.
#[test]
fn clipping_follows_allow_clipping_and_clip_offset() {
    let child = |name: &str, allow: bool| {
        ctrl(
            name,
            "image",
            json!({ "texture": "t", "size": [20, 20], "allow_clipping": allow }),
            vec![],
        )
    };
    let parent = ctrl(
        "panel",
        "panel",
        json!({ "size": [10, 10], "clips_children": true, "clip_offset": [2, 3] }),
        vec![child("inside", true), child("outside", false)],
    );
    let nodes = draw(&parent);
    let inside = node(&nodes, "inside").clip;
    assert_eq!(
        [inside.x, inside.y, inside.w, inside.h],
        [47.0, 48.0, 6.0, 4.0]
    );
    let outside = node(&nodes, "outside").clip;
    assert_eq!([outside.w, outside.h], [100.0, 100.0]);
}

// Renderer configuration beyond `#` bindings reaches the custom draw.
#[test]
fn custom_renderers_receive_their_configuration() {
    let doll = ctrl(
        "doll",
        "custom",
        json!({
            "renderer": "paper_doll_renderer",
            "camera_tilt_degrees": -10,
            "use_selected_skin": true,
            "color1": [1, 0, 0, 1],
            "size": [10, 10]
        }),
        vec![],
    );
    let Draw::Custom { data, .. } = &draw(&doll)[0].draw else {
        panic!("not custom");
    };
    assert_eq!(data["camera_tilt_degrees"], json!(-10));
    assert_eq!(data["use_selected_skin"], json!(true));
    assert_eq!(data["color1"], json!([1, 0, 0, 1]));
    assert!(!data.contains_key("size"));
}

// An `image_cycler` draws its first entry; a disabled parent locks a label.
#[test]
fn cyclers_and_locked_labels_draw() {
    let cycler = ctrl(
        "cycler",
        "image_cycler",
        json!({ "images": [{ "texture_path": "textures/ui/a" }], "size": [16, 16] }),
        vec![],
    );
    assert!(
        matches!(&draw(&cycler)[0].draw, Draw::Sprite { texture, .. } if texture == "textures/ui/a")
    );
    let label = ctrl(
        "label",
        "label",
        json!({ "text": "Locked", "color": [1, 1, 1], "locked_color": [1, 0, 0], "size": [36, 10] }),
        vec![],
    );
    let button = ctrl("button", "panel", json!({ "enabled": false }), vec![label]);
    assert!(
        matches!(&node(&draw(&button), "label").draw, Draw::Text { color, .. } if *color == [255, 0, 0, 255])
    );
}

// An edit box hides its placeholder once its text control has text.
#[test]
fn edit_box_placeholder_hides_behind_text() {
    let edit = |text: &str| {
        ctrl(
            "edit",
            "edit_box",
            json!({ "text_control": "value", "place_holder_control": "hint", "size": [60, 10] }),
            vec![
                ctrl("value", "label", json!({ "text": text }), vec![]),
                ctrl("hint", "label", json!({ "text": "Hint" }), vec![]),
            ],
        )
    };
    let texts = |nodes: Vec<DrawNode>| {
        nodes
            .into_iter()
            .filter_map(|node| match node.draw {
                Draw::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(draw(&edit(""))), ["Hint"]);
    assert_eq!(texts(draw(&edit("X"))), ["X"]);
}

// An empty texture draws nothing even with a colour; an unresolved one only
// without `allow_debug_missing_texture: false`.
#[test]
fn empty_and_missing_textures() {
    let image = |props: Value| draw(&ctrl("image", "image", props, vec![]));
    assert!(image(json!({ "texture": "", "color": [1, 0, 0], "size": [8, 8] })).is_empty());
    assert_eq!(
        image(json!({ "texture": "missing", "size": [8, 8] })).len(),
        1
    );
    let hidden =
        json!({ "texture": "missing", "allow_debug_missing_texture": false, "size": [8, 8] });
    assert!(image(hidden).is_empty());
}

/// Zeqa's sidebar box: a 6x6 rounded `Black_sb` with no sidecar.
struct RoundedBox;
impl TextureSource for RoundedBox {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        Some(TextureMeta::plain([6.0, 6.0]))
    }
}

// A control `nineslice_size` slices a sidecar-less texture: stretching the whole
// 6x6 drew its transparent corner texels as a stepped double box (Zeqa's top bar).
#[test]
fn control_nineslice_keeps_a_rounded_box_one_box() {
    let entry = ctrl(
        "entry",
        "image",
        json!({
            "texture": "textures/ui/zeqa/sb/Black_sb",
            "nineslice_size": 4,
            "alpha": 0.6,
            "size": [47, 15]
        }),
        vec![],
    );
    let env = LayoutEnv {
        text: &MonoText,
        textures: &RoundedBox,
    };
    let nodes = emit(&layout(&entry, [100.0, 100.0], &env), &env);
    assert_eq!(nodes.len(), 9);
    let corner = nodes[0].dest;
    assert_eq!([corner.w, corner.h], [4.0, 4.0]);
    let centre = nodes[4].dest;
    assert_eq!([centre.w, centre.h], [39.0, 7.0]);
    assert!(nodes.iter().all(|node| node.alpha == 0.6));
}
