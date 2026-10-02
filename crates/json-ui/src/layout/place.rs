//! Placement: a sized control's rect from its anchors and offset, and its
//! animations' offset and size ends measured the same way.

use serde::Deserialize;
use serde_json::Value;

use std::sync::Arc;

use super::{Axis, LayoutEnv, Rect, ResolvedControl, axis_index, measure};
use crate::anim::{AnimGraph, AnimKind, ControlAnims, GRAPH_KEY, Inherited, NodeAnim};
use crate::expr::{self, AxisContext, Length};

/// The child's rect from its resolved size and anchor/offset within `parent_rect`:
/// its `anchor_to` point lands on the parent's `anchor_from` point.
pub(super) fn place_by_anchor(
    control: &ResolvedControl,
    parent_rect: Rect,
    size: [f64; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> Rect {
    let from = anchor_from(control);
    // An anchored offset pins `anchor_to` to `anchor_from`.
    let to = if anchored_offset(control).is_some() {
        from
    } else {
        anchor_to(control)
    };
    let off = offset(control, parent_rect, size, siblings, env);
    let x = parent_rect.x + parent_rect.w * from[0] - size[0] * to[0] + off[0];
    let y = parent_rect.y + parent_rect.h * from[1] - size[1] * to[1] + off[1];
    Rect::new(x, y, size[0], size[1])
}

/// The static `offset` in pixels. An anchored offset replaces an edge-anchored
/// axis with that fraction of the parent, measured in from the anchored edge.
fn offset(
    control: &ResolvedControl,
    parent_rect: Rect,
    size: [f64; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> [f64; 2] {
    let mut offset = control.properties.get("offset").map_or([0.0; 2], |pair| {
        offset_pixels(control, pair, parent_rect, size, siblings, env)
    });
    if let Some(value) = anchored_offset(control) {
        let from = anchor_from(control);
        let parent = [parent_rect.w, parent_rect.h];
        for axis in 0..2 {
            if from[axis] == 0.0 {
                offset[axis] = value[axis] * parent[axis];
            } else if from[axis] == 1.0 {
                offset[axis] = -value[axis] * parent[axis];
            }
        }
    }
    offset
}

/// `use_anchored_offset`'s `#anchored_offset_value_x`/`_y` once either leaves
/// zero; a centred control has none.
fn anchored_offset(control: &ResolvedControl) -> Option<[f64; 2]> {
    if control.properties.get("use_anchored_offset") != Some(&Value::Bool(true))
        || anchor_from(control) == [0.5, 0.5]
    {
        return None;
    }
    let bag = control.properties.get("property_bag");
    // The bound component value first, then the bag's.
    let value = |key: &str| {
        control
            .properties
            .get(&key[1..])
            .or_else(|| control.properties.get(key))
            .or_else(|| bag.and_then(|bag| bag.get(key)))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    let pair = [
        value("#anchored_offset_value_x"),
        value("#anchored_offset_value_y"),
    ];
    (pair != [0.0; 2]).then_some(pair)
}

/// A control's offset delta from `follows_cursor`, `follows_cursor_inside_parent`
/// or a drag, applied to its laid-out `rect` within `parent`.
pub(super) fn offset_delta(
    control: &ResolvedControl,
    rect: Rect,
    parent: Rect,
    pointer: Option<[f64; 2]>,
    dragged: Option<[f64; 2]>,
) -> Option<Rect> {
    let flag = |key: &str| control.properties.get(key) == Some(&Value::Bool(true));
    let moved = |x: f64, y: f64| Some(Rect::new(x, y, rect.w, rect.h));
    if let Some(delta) = dragged {
        let axes = draggable_axes(control);
        let mut delta = [
            if axes[0] { delta[0] } else { 0.0 },
            if axes[1] { delta[1] } else { 0.0 },
        ];
        if flag("contained") {
            let (parent_size, own) = ([parent.w, parent.h], [rect.w, rect.h]);
            for axis in 0..2 {
                delta[axis] = if parent_size[axis] < own[axis] {
                    -(own[axis] - parent_size[axis]) / 2.0
                } else {
                    delta[axis].max(0.0).min(parent_size[axis] - own[axis])
                };
            }
        }
        return moved(rect.x + delta[0], rect.y + delta[1]);
    }
    let pointer = pointer?;
    if flag("follows_cursor") {
        return moved(pointer[0] - rect.w / 2.0, pointer[1] - rect.h / 2.0);
    }
    if flag("follows_cursor_inside_parent") {
        if rect.w == 0.0 || rect.h == 0.0 {
            return moved(rect.x + 30000.0, rect.y + 30000.0);
        }
        let gap = 10.0;
        let x = if pointer[0] + rect.w + gap <= parent.x + parent.w {
            pointer[0] + gap
        } else {
            pointer[0] - rect.w - gap
        };
        let y = if pointer[1] + rect.h + gap <= parent.y + parent.h {
            pointer[1] + gap
        } else {
            pointer[1] - rect.h - gap
        };
        return moved(x, y);
    }
    None
}

/// Whether a control follows the pointer.
pub(super) fn follows_pointer(control: &ResolvedControl) -> bool {
    ["follows_cursor", "follows_cursor_inside_parent"]
        .iter()
        .any(|key| control.properties.get(*key) == Some(&Value::Bool(true)))
}

/// The axes `draggable` moves: `horizontal`, `vertical`, `both`.
pub(crate) fn draggable_axes(control: &ResolvedControl) -> [bool; 2] {
    match control.properties.get("draggable").and_then(Value::as_str) {
        Some("horizontal") => [true, false],
        Some("vertical") => [false, true],
        Some("both") => [true, true],
        _ => [false, false],
    }
}

/// An `[x, y]` offset pair in pixels, its units read like size units (`%` of the
/// parent, `%x`/`%y` own size, `%c`/`%cm` children, `%sm` siblings). An axis
/// that is not an expression (`default`, `fill`) adds no offset.
fn offset_pixels(
    control: &ResolvedControl,
    pair: &Value,
    parent_rect: Rect,
    size: [f64; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> [f64; 2] {
    let Value::Array(items) = pair else {
        return [0.0; 2];
    };
    let children = measure::children(control, env, [Some(size[0]), Some(size[1])]);
    let axis_value = |axis: Axis| {
        let index = axis_index(axis);
        let Some(Length::Terms(terms)) = items.get(index).map(expr::length_from_value) else {
            return 0.0;
        };
        let ctx = AxisContext {
            parent: [parent_rect.w, parent_rect.h][index],
            own_width: Some(size[0]),
            own_height: Some(size[1]),
            children: Some(children.content[index]),
            children_max: Some(children.maximum[index]),
            sibling_max: Some(siblings[index]),
            natural: None,
        };
        Length::Terms(terms).eval_pixels(&ctx)
    };
    [axis_value(Axis::X), axis_value(Axis::Y)]
}

/// The control's animations with offset and size ends in pixels, `%` of the
/// parent and `%x`/`%y` of the control's own size.
pub(super) fn control_anims(
    control: &ResolvedControl,
    key: &str,
    rect: Rect,
    parent_rect: Rect,
    inherited: &Inherited,
    packed: bool,
    env: &LayoutEnv,
) -> Option<Arc<ControlAnims>> {
    let mut graph: AnimGraph = AnimGraph::deserialize(control.properties.get(GRAPH_KEY)?).ok()?;
    if !graph.valid() {
        return None;
    }
    let size = [rect.w, rect.h];
    let rest_offset = offset(control, parent_rect, size, [0.0; 2], env);
    for node in &mut graph.nodes {
        let ends = match node.kind {
            AnimKind::Offset => [&node.from_expr, &node.to_expr]
                .map(|pair| offset_pixels(control, pair, parent_rect, size, [0.0; 2], env)),
            AnimKind::Size => [&node.from_expr, &node.to_expr].map(|pair| match pair {
                Value::Array(_) => offset_pixels(control, pair, parent_rect, size, [0.0; 2], env),
                _ => size,
            }),
            _ => continue,
        };
        // A stack item or grid cell has no offset term, so its offset animation is inert.
        let ends = if packed && node.kind == AnimKind::Offset {
            [rest_offset; 2]
        } else {
            ends
        };
        node.from = [ends[0][0] as f32, ends[0][1] as f32, 0.0, 0.0];
        node.to = [ends[1][0] as f32, ends[1][1] as f32, 0.0, 0.0];
    }
    let context = inherited.own(control);
    let wait_scale = control
        .properties
        .get("property_bag")
        .and_then(|bag| bag.get("wait_duration_scaler"))
        .and_then(Value::as_f64)
        .unwrap_or(1.0) as f32;
    for node in graph
        .nodes
        .iter_mut()
        .filter(|node| node.kind == AnimKind::Wait)
    {
        node.duration *= wait_scale;
    }
    let anchor = anchor_to(control);
    Some(Arc::new(ControlAnims {
        key: key.to_owned(),
        graph,
        rest_alpha: control
            .properties
            .get("alpha")
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32,
        rest_offset: rest_offset.map(|axis| axis as f32),
        rect: [rect.x, rect.y, rect.w, rect.h],
        anchor,
        born: context.born,
        clock: context.clock,
        disable_fast_forward: crate::widgets::bound_bool(control, "disable_anim_fast_forward")
            .unwrap_or(false),
        reset_name: control
            .properties
            .get("animation_reset_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        has_sprite: control.control_type.as_deref() == Some("image"),
    }))
}

/// The static sprite uv and clip a draw's own animations replace.
pub(super) fn sprite_rest(control: &ResolvedControl, node: &mut NodeAnim) {
    let pair = |key: &str| {
        let items = control.properties.get(key)?.as_array()?;
        Some([
            items.first()?.as_f64()? as f32,
            items.get(1)?.as_f64()? as f32,
        ])
    };
    node.uv_rest = pair("uv").unwrap_or([0.0; 2]);
    node.uv_size_rest = pair("uv_size");
    if node
        .own
        .as_ref()
        .is_some_and(|own| own.writes(AnimKind::Clip))
    {
        node.clip_direction = Some(
            control
                .properties
                .get("clip_direction")
                .and_then(Value::as_str)
                .unwrap_or("left")
                .to_owned(),
        );
        node.clip_rest = crate::widgets::bound_number(control, "clip_ratio").unwrap_or(0.0) as f32;
    }
}

pub(super) fn anchor_from(control: &ResolvedControl) -> [f64; 2] {
    anchor(control, "anchor_from")
}

pub(super) fn anchor_to(control: &ResolvedControl) -> [f64; 2] {
    anchor(control, "anchor_to")
}

/// Fractional anchor point `[fx, fy]`, defaulting to `center`.
fn anchor(control: &ResolvedControl, key: &str) -> [f64; 2] {
    // `left`/`right` and `top`/`bottom` appear as either half of a name
    // (`top_left`, `left_middle`), so match on membership, not position.
    let name = control
        .properties
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("center");
    let fx = if name.contains("left") {
        0.0
    } else if name.contains("right") {
        1.0
    } else {
        0.5
    };
    let fy = if name.contains("top") {
        0.0
    } else if name.contains("bottom") {
        1.0
    } else {
        0.5
    };
    [fx, fy]
}
