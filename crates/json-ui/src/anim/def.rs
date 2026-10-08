//! Animation definitions as vanilla reads them:
//! the base fields every type shares and each type's own values, linked into a
//! per-control graph so a `next` cycle re-enters the same instance.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::ease::Easing;

/// What an animation writes (`ui::AnimationType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimKind {
    Alpha,
    Clip,
    Color,
    FlipBook,
    Aseprite,
    Offset,
    Size,
    Uv,
    Wait,
}

impl AnimKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alpha" => AnimKind::Alpha,
            "clip" => AnimKind::Clip,
            "color" => AnimKind::Color,
            "flip_book" => AnimKind::FlipBook,
            "aseprite_flip_book" => AnimKind::Aseprite,
            "offset" => AnimKind::Offset,
            "size" => AnimKind::Size,
            "uv" => AnimKind::Uv,
            "wait" => AnimKind::Wait,
            _ => return None,
        })
    }

    /// The key a referencing property takes its initial value from.
    pub(crate) fn initial_key(self) -> &'static str {
        match self {
            AnimKind::FlipBook | AnimKind::Aseprite => "initial_uv",
            _ => "from",
        }
    }
}

/// One animation instance's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimNode {
    pub kind: AnimKind,
    pub duration: f32,
    pub easing: Easing,
    /// Interpolated ends: scalars, colours, uv pairs; offset/size ends are
    /// length expressions until layout gives them pixels.
    pub from: [f32; 4],
    pub to: [f32; 4],
    #[serde(default)]
    pub from_expr: Value,
    #[serde(default)]
    pub to_expr: Value,
    pub play_event: Option<String>,
    pub reset_event: Option<String>,
    pub end_event: Option<String>,
    pub destroy_at_end: Option<String>,
    pub wait_until_rendered: bool,
    pub resettable: bool,
    pub scale_from_starting_alpha: bool,
    pub fps: f32,
    pub frame_count: i32,
    pub reversible: bool,
    pub vertical: bool,
    pub looping: bool,
    /// Index of the `next` animation in the owning graph.
    pub next: Option<usize>,
}

/// A control's animations: every node its references reach, and the heads the
/// control plays, in the order the factory added them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimGraph {
    pub nodes: Vec<AnimNode>,
    pub heads: Vec<usize>,
}

impl AnimGraph {
    /// Whether every head and link addresses a node in this graph.
    pub(crate) fn valid(&self) -> bool {
        self.heads.iter().all(|&index| index < self.nodes.len())
            && self
                .nodes
                .iter()
                .all(|node| node.next.is_none_or(|index| index < self.nodes.len()))
    }
}

fn number(props: &Map<String, Value>, key: &str, fallback: f32) -> f32 {
    props
        .get(key)
        .and_then(Value::as_f64)
        .map_or(fallback, |value| value as f32)
}

fn flag(props: &Map<String, Value>, key: &str, fallback: bool) -> bool {
    match props.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(text)) if text == "true" => true,
        Some(Value::String(text)) if text == "false" => false,
        _ => fallback,
    }
}

fn text(props: &Map<String, Value>, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// `[r, g, b, a]` from a 3- or 4-item array, else `fallback`.
fn color(value: Option<&Value>, fallback: [f32; 4]) -> [f32; 4] {
    let Some(items) = value.and_then(Value::as_array) else {
        return fallback;
    };
    let channel = |index: usize, default: f32| {
        items
            .get(index)
            .and_then(Value::as_f64)
            .map_or(default, |value| value as f32)
    };
    if items.len() < 3 {
        return fallback;
    }
    [
        channel(0, 1.0),
        channel(1, 1.0),
        channel(2, 1.0),
        channel(3, 1.0),
    ]
}

fn pair(value: Option<&Value>) -> [f32; 4] {
    let items = value.and_then(Value::as_array);
    let at = |index: usize| {
        items
            .and_then(|items| items.get(index))
            .and_then(Value::as_f64)
            .map_or(0.0, |value| value as f32)
    };
    [at(0), at(1), 0.0, 0.0]
}

impl AnimNode {
    /// The node `props` (already substituted) define; `None` without a known type.
    pub fn parse(props: &Map<String, Value>) -> Option<Self> {
        let kind = AnimKind::from_name(props.get("anim_type")?.as_str()?)?;
        let scalar = |key: &str| [number(props, key, 1.0), 0.0, 0.0, 0.0];
        let (from, to) = match kind {
            AnimKind::Alpha | AnimKind::Clip => (scalar("from"), scalar("to")),
            AnimKind::Color => (
                color(props.get("from"), [1.0; 4]),
                color(props.get("to"), [1.0; 4]),
            ),
            AnimKind::Uv => (pair(props.get("from")), pair(props.get("to"))),
            _ => ([0.0; 4], [0.0; 4]),
        };
        let expr = |key: &str| match kind {
            AnimKind::Offset | AnimKind::Size => props.get(key).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        Some(Self {
            kind,
            duration: number(props, "duration", 1.0),
            easing: props
                .get("easing")
                .and_then(Value::as_str)
                .map_or(Easing::Linear, Easing::from_name),
            from,
            to,
            from_expr: expr("from"),
            to_expr: expr("to"),
            play_event: text(props, "play_event"),
            reset_event: text(props, "reset_event"),
            end_event: text(props, "end_event"),
            destroy_at_end: text(props, "destroy_at_end"),
            wait_until_rendered: flag(props, "wait_until_rendered_to_play", false),
            resettable: flag(props, "resettable", true),
            scale_from_starting_alpha: flag(props, "scale_from_starting_alpha", false),
            fps: number(props, "fps", 0.0),
            // Only an integer count is read; anything else is one frame.
            frame_count: props
                .get("frame_count")
                .and_then(|value| value.as_i64())
                .map_or(1, |count| {
                    count.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
                }),
            reversible: flag(props, "reversible", false),
            vertical: props.get("orientation").and_then(Value::as_str) == Some("vertical"),
            looping: flag(props, "looping", true),
            next: None,
        })
    }
}
