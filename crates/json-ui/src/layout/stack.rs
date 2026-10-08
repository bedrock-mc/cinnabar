//! Stack panels as the client lays them out: each child starts where the
//! previous one ended (a hidden one adds nothing), `fill` children share the
//! leftover main-axis space, and anchors take part only with
//! `use_child_anchors`. Children ignore `offset`. `use_priority` hides the
//! lowest-priority children that overflow.

use serde_json::Value;

use super::{Axis, LayoutEnv, Rect, ResolvedControl, axis_index, measure, other, place, size};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Orientation {
    Horizontal,
    Vertical,
    None,
}

/// A stack panel's `orientation`; vertical by default.
pub(super) fn orientation(control: &ResolvedControl) -> Option<Orientation> {
    if control.control_type.as_deref() != Some("stack_panel") {
        return None;
    }
    Some(
        match control
            .properties
            .get("orientation")
            .and_then(Value::as_str)
        {
            Some("horizontal") => Orientation::Horizontal,
            Some("none") => Orientation::None,
            _ => Orientation::Vertical,
        },
    )
}

/// The axis a stack packs along; `none` packs along both.
pub(super) fn main_axis(control: &ResolvedControl) -> Option<Axis> {
    match orientation(control)? {
        Orientation::Horizontal => Some(Axis::X),
        Orientation::Vertical => Some(Axis::Y),
        Orientation::None => None,
    }
}

fn flag(control: &ResolvedControl, key: &str) -> bool {
    matches!(control.properties.get(key), Some(Value::Bool(true)))
}

/// Children's `[w, h]` in a stack of `extent`: `fill` children take an equal
/// share of what the visible others leave, at least zero and at most their
/// `max_size` (read only when a width maximum exists, as the client does).
pub(super) fn child_sizes(
    parent: &ResolvedControl,
    extent: [Option<f64>; 2],
    env: &LayoutEnv,
) -> Vec<[f64; 2]> {
    let Some(main) = main_axis(parent) else {
        return measure::relative_sizes(parent, extent, env);
    };
    let fill = |child: &ResolvedControl| size::is_fill(child, main);
    let mut sizes = measure::resolve_children(parent, extent, env, |child| !fill(child));
    let visible = |child: &ResolvedControl| super::visible(child);
    measure::apply_inherit(parent, &mut sizes, |child| !fill(child));
    let main_index = axis_index(main);
    let taken: f64 = parent
        .children
        .iter()
        .zip(&sizes)
        .filter(|(child, _)| visible(child) && !fill(child))
        .map(|(_, size)| size[main_index])
        .sum();
    let shares = parent
        .children
        .iter()
        .filter(|child| visible(child) && fill(child))
        .count()
        .max(1);
    let share = (extent[main_index].unwrap_or(0.0) - taken) / shares as f64;
    let siblings = measure::sibling_maxima(parent, &sizes);
    for (child, slot) in parent.children.iter().zip(sizes.iter_mut()) {
        if !fill(child) {
            continue;
        }
        let mut own = [None; 2];
        let (_, ctx) = size::axis_rule(child, main, extent, own, siblings, env);
        let mut value = share.max(0.0);
        if size::has_max(child, Axis::X) {
            value = size::clamp_max(child, main, value, &ctx, extent);
        }
        own[main_index] = Some(value);
        let cross = other(main);
        let (resolved, ctx) = size::axis_rule(child, cross, extent, own, siblings, env);
        let cross_value = match resolved {
            crate::expr::Resolved::Pixels(value) => value,
            crate::expr::Resolved::Fill => 0.0,
        };
        slot[main_index] = value;
        slot[axis_index(cross)] = size::clamp(child, cross, cross_value, &ctx, extent);
    }
    sizes
}

/// Rects of a stack's children inside `stack`.
pub(super) fn stack_children<'a>(
    parent: &'a ResolvedControl,
    stack: Rect,
    env: &LayoutEnv,
) -> Vec<(&'a ResolvedControl, Rect)> {
    let sizes = measure::sizes(parent, [Some(stack.w), Some(stack.h)], env);
    let hidden = priority_hidden(parent, &sizes, [stack.w, stack.h]);
    let anchors = flag(parent, "use_child_anchors");
    let packs = |axis: Axis| main_axis(parent).is_none_or(|main| main == axis);
    let extent = [stack.w, stack.h];
    let mut previous: Option<([f64; 2], [f64; 2], bool)> = None;
    let mut placed = Vec::with_capacity(parent.children.len());
    for ((child, size), priority_hidden) in parent.children.iter().zip(sizes.iter()).zip(&hidden) {
        let from = place::anchor_from(child);
        let to = place::anchor_to(child);
        let mut at = [0.0; 2];
        for axis in [Axis::X, Axis::Y] {
            let index = axis_index(axis);
            let start = previous.map_or(0.0, |(position, extent, shown)| {
                position[index] + if shown { extent[index] } else { 0.0 }
            });
            at[index] = if packs(axis) {
                start
                    + if anchors {
                        size[index] * (from[index] - to[index])
                    } else {
                        0.0
                    }
            } else if anchors {
                extent[index] * from[index] - size[index] * to[index]
            } else {
                0.0
            };
        }
        let shown = super::visible(child) && !priority_hidden;
        if shown {
            previous = Some((at, *size, shown));
        }
        placed.push((
            child,
            Rect::new(stack.x + at[0], stack.y + at[1], size[0], size[1]),
        ));
    }
    placed
}

/// Per child of `control` laid out in `rect`, whether `use_priority` hides it.
pub(super) fn hidden_by_priority(
    control: &ResolvedControl,
    rect: Rect,
    env: &LayoutEnv,
) -> Vec<bool> {
    if orientation(control).is_none() || !flag(control, "use_priority") {
        return Vec::new();
    }
    let sizes = measure::sizes(control, [Some(rect.w), Some(rect.h)], env);
    priority_hidden(control, &sizes, [rect.w, rect.h])
}

/// Which children `use_priority` hides this layout: when the children's summed
/// main extent overflows the stack, the lowest `priority` values keep their
/// place while they fit (non-positive priorities always do); otherwise only the
/// `show_when_controls_are_hidden` children hide.
pub(super) fn priority_hidden(
    parent: &ResolvedControl,
    sizes: &[[f64; 2]],
    extent: [f64; 2],
) -> Vec<bool> {
    let mut hidden = vec![false; parent.children.len()];
    if !flag(parent, "use_priority") {
        return hidden;
    }
    let main = if orientation(parent) == Some(Orientation::Vertical) {
        1
    } else {
        0
    };
    let available = extent[main];
    if available <= 0.0 {
        return hidden;
    }
    let length = |index: usize| {
        if super::visible(&parent.children[index]) {
            sizes[index][main]
        } else {
            0.0
        }
    };
    let mut sum = 0.0;
    let overflows = parent.children.iter().enumerate().any(|(index, child)| {
        if shows_when_hidden(child) {
            return false;
        }
        sum += length(index);
        sum > available
    });
    if !overflows {
        for (slot, child) in hidden.iter_mut().zip(&parent.children) {
            *slot = shows_when_hidden(child);
        }
        return hidden;
    }
    let main_axis = if main == 0 { Axis::X } else { Axis::Y };
    let mut order: Vec<usize> = (0..parent.children.len())
        .filter(|index| !size::is_fill(&parent.children[*index], main_axis))
        .collect();
    order.sort_by_key(|index| priority(&parent.children[*index]));
    let mut remaining = available;
    for index in order {
        let length = length(index);
        if length <= remaining || priority(&parent.children[index]) < 1 {
            remaining -= length;
        } else {
            hidden[index] = true;
        }
    }
    hidden
}

/// A child's integer `priority`, else zero.
fn priority(control: &ResolvedControl) -> i64 {
    control
        .properties
        .get("priority")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn shows_when_hidden(control: &ResolvedControl) -> bool {
    control
        .properties
        .get("priority_rule")
        .and_then(Value::as_str)
        == Some("show_when_controls_are_hidden")
}
