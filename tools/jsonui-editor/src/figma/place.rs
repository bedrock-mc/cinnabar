//! Where a control sits: constraints to anchors, offsets and sizes, and
//! stack children's sizing and cross-axis alignment.

use serde_json::{Value, json};

use super::convert::Body;
use super::{Align, Constraint, Node, Sizing};

/// By `[vertical][horizontal]` part: start, middle, end.
const ANCHORS: [[&str; 3]; 3] = [
    ["top_left", "top_middle", "top_right"],
    ["left_middle", "center", "right_middle"],
    ["bottom_left", "bottom_middle", "bottom_right"],
];

/// Where a control sits in its parent.
#[derive(Clone, Copy)]
pub(super) enum Place {
    /// Fills its parent: the converted frame, and a button's state variants.
    Fill,
    /// Positioned by its constraints in a parent of this size.
    Free { parent: [f64; 2] },
    /// Packed by a stack panel along `main`; `anchored` aligns it on the
    /// cross axis by `counter`.
    Stack {
        main: usize,
        counter: Align,
        anchored: bool,
    },
}

/// Sets `size`, `offset` and anchors for `place` of a control `extent` in
/// size. `auto` axes size to content.
pub(super) fn place_control(
    body: &mut Body,
    node: &Node,
    extent: [f64; 2],
    auto: [bool; 2],
    place: Place,
) {
    match place {
        Place::Fill => {
            body.insert("size".into(), json!(["100%", "100%"]));
        }
        Place::Free { parent } => {
            let constraints = [node.constraints.horizontal, node.constraints.vertical];
            let position = [node.x, node.y];
            let mut parts = [0; 2];
            let mut offset = [Value::Null, Value::Null];
            let mut size = [Value::Null, Value::Null];
            for axis in 0..2 {
                (parts[axis], offset[axis], size[axis]) = free_axis(
                    constraints[axis],
                    position[axis],
                    extent[axis],
                    parent[axis],
                    auto[axis],
                );
            }
            body.insert("size".into(), json!(size));
            body.insert("offset".into(), json!(offset));
            anchor(body, parts);
        }
        Place::Stack {
            main,
            counter,
            anchored,
        } => {
            let size: [Value; 2] = std::array::from_fn(|axis| match node.sizing[axis] {
                Sizing::Fill if axis == main => json!("fill"),
                Sizing::Fill => json!("100%"),
                _ if auto[axis] => json!("default"),
                _ => number(extent[axis]),
            });
            body.insert("size".into(), json!(size));
            if anchored {
                let mut parts = [0; 2];
                parts[1 - main] = match counter {
                    Align::Center => 1,
                    Align::Max => 2,
                    _ => 0,
                };
                anchor(body, parts);
            }
        }
    }
}

/// One axis of a constrained layer: its anchor part (start, middle, end),
/// offset and size.
fn free_axis(
    constraint: Constraint,
    position: f64,
    length: f64,
    parent: f64,
    auto: bool,
) -> (usize, Value, Value) {
    let size = if auto {
        json!("default")
    } else {
        number(length)
    };
    match constraint {
        Constraint::Max => (2, number(position + length - parent), size),
        Constraint::Center => (1, number(position + length / 2.0 - parent / 2.0), size),
        Constraint::Stretch => (0, number(position), stretch(parent - length)),
        Constraint::Scale if parent > 0.0 => {
            (0, percent(position / parent), percent(length / parent))
        }
        _ => (0, number(position), size),
    }
}

pub(super) fn anchor(body: &mut Body, [horizontal, vertical]: [usize; 2]) {
    let name = ANCHORS[vertical][horizontal];
    body.insert("anchor_from".into(), name.into());
    body.insert("anchor_to".into(), name.into());
}

/// Pixels at 1/100 precision, integral when whole.
pub(super) fn number(value: f64) -> Value {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
        json!(rounded as i64)
    } else {
        json!(rounded)
    }
}

fn percent(ratio: f64) -> Value {
    json!(format!("{}%", number(ratio * 100.0)))
}

/// The parent's extent less `inset` pixels.
pub(super) fn stretch(inset: f64) -> Value {
    match number(inset) {
        Value::Number(n) if n.as_f64() == Some(0.0) => json!("100%"),
        _ if inset > 0.0 => json!(format!("100% - {}px", number(inset))),
        _ => json!(format!("100% + {}px", number(-inset))),
    }
}
