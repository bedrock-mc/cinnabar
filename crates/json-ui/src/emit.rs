//! Turn a laid-out tree into a flat, layer-ordered list of draw commands. Sprites
//! carry the texture path and a normalised source sub-rect; the atlas that maps a
//! path to a page and pixel UVs binds later. An `image`'s quads come from
//! [`crate::sprite`]; a `custom` control emits an opaque [`Draw::Custom`] the
//! caller renders itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::layout::{LaidOut, LayoutEnv, Rect};
use crate::tree::ResolvedControl;

/// A normalised source sub-rect (0..1 of the texture) for a sprite quad.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UvRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl UvRect {
    pub fn full() -> Self {
        Self {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
        }
    }
}

/// A rect derived by nine-slicing: where to draw and which source sub-rect to sample.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteQuad {
    pub dest: RectOut,
    pub uv: UvRect,
}

/// A serialisable rect; layout's [`Rect`] is copied into it for emit output.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RectOut {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl From<Rect> for RectOut {
    fn from(rect: Rect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A sprite's `bilinear` sampling and `grayscale` material choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteFilter {
    pub bilinear: bool,
    pub grayscale: bool,
}

/// A single primitive: what the carrier binds to the atlas and font later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Draw {
    Solid {
        color: [u8; 4],
    },
    Sprite {
        texture: String,
        uv: UvRect,
        color: [u8; 4],
        #[serde(default)]
        filter: SpriteFilter,
    },
    Text {
        text: String,
        color: [u8; 4],
        shadow: bool,
        align: TextAlign,
        /// `font_scale_factor`: glyphs draw this many times their natural size.
        scale: f32,
        /// Whether the text is a language key or `%token` text (`localize`, default on).
        localize: bool,
        #[serde(default)]
        options: crate::label::TextOptions,
    },
    /// A `custom` control (`renderer` names it, e.g. `inventory_item_renderer`)
    /// with its bound `#` values, drawn by the caller.
    Custom {
        renderer: String,
        data: BTreeMap<String, Value>,
    },
}

/// One positioned primitive, in final draw order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawNode {
    /// The source control's instance name, so a bound node can be traced back.
    pub name: String,
    /// The source control's layout key (see [`crate::state`]).
    pub key: String,
    pub dest: RectOut,
    pub clip: RectOut,
    pub layer: i32,
    pub alpha: f32,
    /// Animations reaching this draw; see [`DrawNode::animate`].
    #[serde(skip)]
    pub anim: Option<std::sync::Arc<crate::anim::NodeAnim>>,
    pub draw: Draw,
    /// State children this node sits under, from [`emit_gated`]; see [`DrawNode::shown`].
    #[serde(default)]
    pub gates: Vec<StateGate>,
}

/// A state child (hover, pressed, …) of the control at `key`, shown under the
/// interaction states set in `mask`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateGate {
    pub key: String,
    pub mask: u8,
}

impl DrawNode {
    /// Whether the node shows under `state`: every state child it sits under is
    /// the one its control shows.
    pub fn shown(&self, state: &crate::state::ViewState) -> bool {
        self.gates
            .iter()
            .all(|gate| gate.mask & (1 << crate::widgets::state_index(state, &gate.key)) != 0)
    }
}

/// Flatten a laid-out tree to draw commands, ordered by `layer` then document order.
/// Invisible controls and their descendants are dropped.
pub fn emit(root: &LaidOut, env: &LayoutEnv) -> Vec<DrawNode> {
    let mut nodes = Vec::new();
    let mut order = 0usize;
    collect(root, env, &mut nodes, &mut order);
    nodes.sort_by_key(|(layer, index, _)| (*layer, *index));
    nodes.into_iter().map(|(_, _, node)| node).collect()
}

/// [`emit`] of a tree laid out with no hover, press or focus, keeping every state
/// child's subtree gated by the states it shows under, so an interaction change
/// only filters nodes ([`DrawNode::shown`]) instead of laying out again.
pub fn emit_gated(root: &LaidOut, env: &LayoutEnv) -> Vec<DrawNode> {
    let mut nodes = Vec::new();
    let mut order = 0usize;
    let mut gates = Vec::new();
    collect_gated(
        root,
        env,
        &mut nodes,
        &mut order,
        &mut gates,
        &mut Vec::new(),
    );
    nodes.sort_by_key(|(layer, index, _)| (*layer, *index));
    nodes.into_iter().map(|(_, _, node)| node).collect()
}

fn collect_gated(
    node: &LaidOut,
    env: &LayoutEnv,
    out: &mut Vec<(i32, usize, DrawNode)>,
    order: &mut usize,
    gates: &mut Vec<StateGate>,
    pending: &mut Vec<(usize, StateGate)>,
) {
    let gate = pending
        .iter()
        .rev()
        .find(|(target, _)| *target == std::ptr::from_ref(node.control).addr())
        .map(|(_, gate)| gate.clone());
    // A state control the neutral state hides still emits, gated, if any state shows it.
    let shows_somewhere = gate.as_ref().is_some_and(|gate| gate.mask != 0);
    if !(node.visible || shows_somewhere) {
        return;
    }
    let gated = gate.is_some();
    gates.extend(gate);
    let first = out.len();
    emit_own(node, env, out, order);
    for (_, _, drawn) in &mut out[first..] {
        drawn.gates.clone_from(gates);
    }
    let before = pending.len();
    pending.extend(
        node.state_targets
            .iter()
            .flat_map(|targets| targets.iter())
            .map(|&(target, mask)| {
                (
                    target,
                    StateGate {
                        key: node.key.clone(),
                        mask,
                    },
                )
            }),
    );
    for child in &node.children {
        collect_gated(child, env, out, order, gates, pending);
    }
    pending.truncate(before);
    if gated {
        gates.pop();
    }
}

fn collect(
    node: &LaidOut,
    env: &LayoutEnv,
    out: &mut Vec<(i32, usize, DrawNode)>,
    order: &mut usize,
) {
    if !node.visible {
        return;
    }
    emit_own(node, env, out, order);
    for child in &node.children {
        collect(child, env, out, order);
    }
}

/// The node's own primitives; a sprite crops to its progress clip.
pub(crate) fn emit_own(
    node: &LaidOut,
    env: &LayoutEnv,
    out: &mut Vec<(i32, usize, DrawNode)>,
    order: &mut usize,
) {
    for (dest, draw) in draws_for(node, env) {
        if matches!(&draw, Draw::Text { text, .. } if text.is_empty())
            && !crate::label::is_editable(node.control)
        {
            continue;
        }
        // A primitive wholly outside its clip (a scrolled-away cell) draws nothing;
        // a hover tooltip draws beside the pointer instead of in its rect.
        let clipped = dest.intersect(node.clip);
        let floats =
            matches!(&draw, Draw::Custom { renderer, .. } if renderer == "hover_text_renderer");
        let moves_geometry = node
            .anim
            .as_ref()
            .is_some_and(|anim| !anim.movers.is_empty() || !anim.clip_movers.is_empty());
        if dest.w > 0.0
            && dest.h > 0.0
            && (clipped.w <= 0.0 || clipped.h <= 0.0)
            && !floats
            && !moves_geometry
        {
            continue;
        }
        out.push((
            node.layer,
            *order,
            DrawNode {
                name: node.control.name.clone(),
                key: node.key.clone(),
                dest: dest.into(),
                clip: node.clip.into(),
                layer: node.layer,
                alpha: node.alpha,
                anim: node.anim.clone(),
                draw,
                gates: Vec::new(),
            },
        ));
        *order += 1;
    }
}

/// The primitives a single control contributes at `rect`: only an `image`
/// draws its texture and only a `label` its text, whatever else it declares.
fn draws_for(node: &LaidOut, env: &LayoutEnv) -> Vec<(Rect, Draw)> {
    let (control, rect) = (node.control, node.rect);
    // A clip animation crops at paint time instead.
    let clip_ratio = node.clip_ratio.filter(|_| {
        node.anim
            .as_ref()
            .is_none_or(|anim| anim.clip_direction.is_none())
    });
    match control.control_type.as_deref() {
        Some("image") => match control.properties.get("texture").and_then(Value::as_str) {
            // An empty texture (an unset binding) draws nothing, as in vanilla.
            Some("") => Vec::new(),
            Some(path) => sprite_draws(control, rect, path, clip_ratio, env),
            None => solid_or_empty(control, rect),
        },
        // An `image_cycler` shows its first `images` entry until it cycles.
        Some("image_cycler") => control
            .properties
            .get("images")
            .and_then(Value::as_array)
            .and_then(|images| images.first()?.get("texture_path")?.as_str())
            .filter(|path| !path.is_empty())
            .map(|path| sprite_draws(control, rect, path, clip_ratio, env))
            .unwrap_or_default(),
        _ if crate::label::is_label(control) => vec![(rect, text_draw(control, node.enabled))],
        Some("custom") => custom_draw(control)
            .map(|draw| vec![(rect, draw)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn sprite_draws(
    control: &ResolvedControl,
    rect: Rect,
    path: &str,
    clip_ratio: Option<f32>,
    env: &LayoutEnv,
) -> Vec<(Rect, Draw)> {
    let color = image_color(control);
    let filter = SpriteFilter {
        bilinear: crate::widgets::bound_bool(control, "bilinear") == Some(true),
        grayscale: crate::widgets::bound_bool(control, "grayscale") == Some(true),
    };
    let meta = env.textures.texture(path);
    // Without `allow_debug_missing_texture` an unresolved image draws nothing
    // rather than the host's missing-texture image.
    if meta.is_none()
        && control.properties.get("allow_debug_missing_texture") == Some(&Value::Bool(false))
    {
        return Vec::new();
    }
    crate::sprite::quads(control, rect, meta.as_ref(), clip_ratio)
        .into_iter()
        .map(|(dest, uv)| {
            (
                dest,
                Draw::Sprite {
                    texture: path.to_owned(),
                    uv,
                    color,
                    filter,
                },
            )
        })
        .collect()
}

/// A sprite's tint: a `#color` binding target wins over `color`.
fn image_color(control: &ResolvedControl) -> [u8; 4] {
    match control.properties.get("#color") {
        Some(value) => color_from_value(value, [255, 255, 255, 255]),
        None => color_of(control, [255, 255, 255, 255]),
    }
}

fn text_draw(control: &ResolvedControl, enabled: bool) -> Draw {
    let mut text = crate::label::text(control);
    // A selected edit box's text target draws its blinking caret after the text.
    if crate::widgets::bound_bool(control, crate::component::CARET_PROPERTY) == Some(true) {
        text.push(crate::component::CARET_GLYPH);
    }
    Draw::Text {
        text,
        color: crate::label::color(control, enabled),
        shadow: matches!(control.properties.get("shadow"), Some(Value::Bool(true))),
        align: alignment(control),
        scale: crate::label::font_scale(control) as f32,
        localize: crate::label::localizes(control),
        options: crate::label::options(control),
    }
}

/// Layout and tree keys a custom renderer never reads; every other property
/// is renderer configuration and travels with the draw.
const STRUCTURAL: [&str; 11] = [
    "controls",
    "bindings",
    "anims",
    "variables",
    "size",
    "offset",
    "anchor_from",
    "anchor_to",
    "layer",
    "button_mappings",
    "type",
];

fn custom_draw(control: &ResolvedControl) -> Option<Draw> {
    let renderer = control.properties.get("renderer")?.as_str()?.to_owned();
    let mut data: BTreeMap<String, Value> = control
        .properties
        .iter()
        .filter(|(key, _)| !STRUCTURAL.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // The renderer also reads its `property_bag` options (`is_durability`, …).
    if let Some(Value::Object(bag)) = control.properties.get("property_bag") {
        for (key, value) in bag.iter().filter(|(key, _)| !key.starts_with('#')) {
            data.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Some(Draw::Custom { renderer, data })
}

fn solid_or_empty(control: &ResolvedControl, rect: Rect) -> Vec<(Rect, Draw)> {
    match control.properties.get("color") {
        Some(value) => vec![(
            rect,
            Draw::Solid {
                color: color_from_value(value, [255, 255, 255, 255]),
            },
        )],
        None => Vec::new(),
    }
}

fn alignment(control: &ResolvedControl) -> TextAlign {
    match control
        .properties
        .get("text_alignment")
        .and_then(Value::as_str)
    {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

fn color_of(control: &ResolvedControl, fallback: [u8; 4]) -> [u8; 4] {
    match control.properties.get("color") {
        Some(value) => color_from_value(value, fallback),
        None => fallback,
    }
}

/// JSON-UI colours are `[r, g, b]`/`[r, g, b, a]` floats in 0..1, a `#rrggbb` hex, or
/// one of a few names. Anything else falls back.
/// A JSON-UI colour value (`[r, g, b(, a)]` in 0..1, `#rrggbb`, or a name).
pub fn color_value(value: &Value) -> Option<[u8; 4]> {
    match value {
        Value::Array(items) if items.len() == 3 || items.len() == 4 => {
            let channel = |index: usize| {
                items
                    .get(index)
                    .and_then(Value::as_f64)
                    .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
            };
            let alpha = if items.len() == 4 {
                channel(3).unwrap_or(255)
            } else {
                255
            };
            Some([channel(0)?, channel(1)?, channel(2)?, alpha])
        }
        Value::String(text) => named_color(text),
        _ => None,
    }
}

fn color_from_value(value: &Value, fallback: [u8; 4]) -> [u8; 4] {
    color_value(value).unwrap_or(fallback)
}

fn named_color(text: &str) -> Option<[u8; 4]> {
    if let Some(hex) = text.strip_prefix('#')
        && hex.len() == 6
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        return Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]);
    }
    match text {
        "white" => Some([255, 255, 255, 255]),
        "black" => Some([0, 0, 0, 255]),
        "gray" | "grey" => Some([128, 128, 128, 255]),
        "red" => Some([255, 0, 0, 255]),
        "green" => Some([0, 255, 0, 255]),
        "blue" => Some([0, 0, 255, 255]),
        "yellow" => Some([255, 255, 0, 255]),
        "orange" => Some([217, 128, 51, 255]),
        "purple" => Some([255, 0, 255, 255]),
        "cyan" => Some([0, 255, 255, 255]),
        "nil" => Some([0, 0, 0, 0]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_floats_round_to_bytes() {
        let color = color_from_value(&serde_json::json!([0.3, 0.3, 0.3]), [1, 1, 1, 1]);
        assert_eq!(color, [77, 77, 77, 255]);
    }
}
