//! Sizing: a control's `[w, h]` from its `size`, `min_size` and `max_size`
//! rules, solved in the order their own-axis references (`%x`, `%y`) need and
//! clamped before anything reads them, as the vanilla client does.

use serde_json::Value;

use super::{Axis, LayoutEnv, ResolvedControl, axis_index, measure, other};
use crate::expr::{self, AxisContext, Length, Resolved, Unit};
use crate::widgets;

/// The client's bounds when a control has no `min_size`/`max_size` rule.
const NO_MIN: f64 = -32768.0;
const NO_MAX: f64 = 32767.0;

/// Memo slots of the parsed lengths: size, then max, then min, per axis.
const SIZE_SLOT: u8 = 0;
const MAX_SLOT: u8 = 2;
const MIN_SLOT: u8 = 4;

/// Resolve `control`'s `[w, h]` under a parent whose axes may be unknown (a
/// parent sizing to its children); `siblings` is the largest sibling per axis,
/// which `%sm` reads.
pub(super) fn resolve_size(
    control: &ResolvedControl,
    parent: [Option<f64>; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> [f64; 2] {
    let first = if height_first(control) {
        Axis::Y
    } else {
        Axis::X
    };
    let mut own = [None; 2];
    for axis in [first, other(first)] {
        let size = match axis_rule(control, axis, parent, own, siblings, env) {
            (Resolved::Pixels(value), ctx) => clamp(control, axis, value, &ctx, parent),
            // `fill` off a stack's main axis is an empty rule: zero, then bounds.
            (Resolved::Fill, ctx) => clamp(control, axis, 0.0, &ctx, parent),
        };
        own[axis_index(axis)] = Some(size);
    }
    [own[0].unwrap_or(0.0), own[1].unwrap_or(0.0)]
}

/// The size rule on `axis`, unclamped, with the context it evaluated in.
pub(super) fn axis_rule(
    control: &ResolvedControl,
    axis: Axis,
    parent: [Option<f64>; 2],
    own: [Option<f64>; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> (Resolved, AxisContext) {
    let ctx = context(control, axis, parent, own, siblings, env);
    if let Some(cells) = super::grid::own_size(control, axis, own, env) {
        return (Resolved::Pixels(cells), ctx);
    }
    let resolved = memo_length(
        control,
        SIZE_SLOT + axis_index(axis) as u8,
        || Some(length(control, axis)),
        |length| length.map_or(Resolved::Pixels(0.0), |length| length.eval(&ctx)),
    );
    (resolved, ctx)
}

/// `value` within the control's bounds on `axis`: an over-large value takes the
/// maximum even when the minimum exceeds it. A parent-relative bound under an
/// unknown parent axis does not constrain.
pub(super) fn clamp(
    control: &ResolvedControl,
    axis: Axis,
    value: f64,
    ctx: &AxisContext,
    parent: [Option<f64>; 2],
) -> f64 {
    let index = axis_index(axis);
    let known = parent[index].is_some();
    let max = bound(control, MAX_SLOT, axis, ctx, known, f64::INFINITY).unwrap_or(NO_MAX);
    let min = bound(control, MIN_SLOT, axis, ctx, known, 0.0).unwrap_or(NO_MIN);
    if max < value {
        max
    } else if value <= min {
        min
    } else {
        value
    }
}

/// `value` under only the maximum on `axis` (a stack `fill` child's minimum is
/// forced to zero).
pub(super) fn clamp_max(
    control: &ResolvedControl,
    axis: Axis,
    value: f64,
    ctx: &AxisContext,
    parent: [Option<f64>; 2],
) -> f64 {
    let known = parent[axis_index(axis)].is_some();
    match bound(control, MAX_SLOT, axis, ctx, known, f64::INFINITY) {
        Some(max) if max < value => max,
        _ => value,
    }
}

/// A bound rule's pixels, or `None` without one (`default`, `fill`, no terms).
fn bound(
    control: &ResolvedControl,
    slot: u8,
    axis: Axis,
    ctx: &AxisContext,
    parent_known: bool,
    unknown_parent: f64,
) -> Option<f64> {
    let index = axis_index(axis);
    let key = if slot == MAX_SLOT {
        "max_size"
    } else {
        "min_size"
    };
    memo_length(
        control,
        slot + index as u8,
        || bound_length(control, key, index),
        |length| {
            length.map(|length| {
                if !parent_known && length.uses(Unit::Percent) {
                    let ctx = AxisContext {
                        parent: unknown_parent,
                        ..*ctx
                    };
                    return length.eval_pixels(&ctx);
                }
                length.eval_pixels(ctx)
            })
        },
    )
}

/// Whether a `max_size` rule exists on `axis`.
pub(super) fn has_max(control: &ResolvedControl, axis: Axis) -> bool {
    let index = axis_index(axis);
    memo_length(
        control,
        MAX_SLOT + index as u8,
        || bound_length(control, "max_size", index),
        |length| length.is_some(),
    )
}

/// What `axis`'s rules evaluate against. Own sizes feed content measurement
/// only where they resolved against a known parent axis.
fn context(
    control: &ResolvedControl,
    axis: Axis,
    parent: [Option<f64>; 2],
    own: [Option<f64>; 2],
    siblings: [f64; 2],
    env: &LayoutEnv,
) -> AxisContext {
    let index = axis_index(axis);
    let known = [
        own[0].filter(|_| parent[0].is_some()),
        own[1].filter(|_| parent[1].is_some()),
    ];
    let flags = flags(control);
    let children = if flags.children[index] {
        measure::children(control, env, known)
    } else {
        measure::Children::default()
    };
    AxisContext {
        parent: parent[index].unwrap_or(0.0),
        own_width: own[0],
        own_height: own[1],
        children: Some(children.content[index]),
        children_max: Some(children.maximum[index]),
        sibling_max: Some(siblings[index]),
        natural: flags.natural[index]
            .then(|| natural(control, axis, own, known[0], env))
            .flatten(),
    }
}

/// Rule properties read on every solve, derived once per layout.
#[derive(Clone, Copy)]
pub(super) struct Flags {
    pub(super) height_first: bool,
    pub(super) reads_sibling_max: bool,
    /// `inherit_max_sibling_width`, `inherit_max_sibling_height`.
    pub(super) inherits: [bool; 2],
    children: [bool; 2],
    natural: [bool; 2],
}

pub(super) fn flags(control: &ResolvedControl) -> Flags {
    let address = std::ptr::from_ref(control).addr();
    if let Some(flags) = measure::FLAGS.with(|memo| memo.borrow().get(&address).copied()) {
        return flags;
    }
    let [width, height] = [Axis::X, Axis::Y].map(|axis| rule_flags(control, axis));
    // An omitted panel height already has a parent-relative rule. It can
    // satisfy width's own-Y bounds just like an explicit `100%` height.
    // Natural label height and ratio-image height still need resolved width.
    // Vanilla counts min/max dependencies before clamping the ordinary size rule.
    let independent_height = height.terms
        || (height.default && !crate::label::is_label(control) && !scales_to_ratio(control));
    let flags = Flags {
        height_first: ((width.cross || width.children)
            && independent_height
            && !height.cross
            && !height.children)
            || (scales_to_ratio(control) && width.default && !height.default),
        children: [width.children, height.children],
        natural: [width.default, height.default],
        reads_sibling_max: width.sibling || height.sibling,
        inherits: ["inherit_max_sibling_width", "inherit_max_sibling_height"]
            .map(|key| control.properties.get(key) == Some(&Value::Bool(true))),
    };
    measure::FLAGS.with(|memo| memo.borrow_mut().insert(address, flags));
    flags
}

/// Whether the height resolves before the width.
pub(super) fn height_first(control: &ResolvedControl) -> bool {
    flags(control).height_first
}

/// Dependencies gathered from one axis's size, minimum and maximum rules.
#[derive(Default)]
struct RuleFlags {
    default: bool,
    terms: bool,
    children: bool,
    cross: bool,
    sibling: bool,
}

/// Inspect each parsed rule once instead of querying each dependency separately.
fn rule_flags(control: &ResolvedControl, axis: Axis) -> RuleFlags {
    let index = axis_index(axis) as u8;
    let mut flags = RuleFlags::default();
    for slot in [SIZE_SLOT, MAX_SLOT, MIN_SLOT] {
        memo_length(
            control,
            slot + index,
            || match slot {
                SIZE_SLOT => Some(length(control, axis)),
                MAX_SLOT => bound_length(control, "max_size", index as usize),
                _ => bound_length(control, "min_size", index as usize),
            },
            |length| {
                if slot == SIZE_SLOT {
                    flags.default = matches!(length, Some(Length::Default));
                    flags.terms = matches!(length, Some(Length::Terms(_)));
                }
                if let Some(length) = length {
                    flags.children |=
                        length.uses(Unit::PercentChildren) || length.uses(Unit::PercentChildrenMax);
                    flags.cross |= length.uses(if axis == Axis::X {
                        Unit::PercentY
                    } else {
                        Unit::PercentX
                    });
                    flags.sibling |= length.uses(Unit::PercentSiblingMax);
                }
            },
        );
    }
    flags
}

/// `read` over the parsed size rule on `axis`, without cloning it.
pub(super) fn with_size<R>(
    control: &ResolvedControl,
    axis: Axis,
    read: impl FnOnce(Option<&Length>) -> R,
) -> R {
    memo_length(
        control,
        SIZE_SLOT + axis_index(axis) as u8,
        || Some(length(control, axis)),
        read,
    )
}

/// Whether the size on `axis` is `fill`.
pub(super) fn is_fill(control: &ResolvedControl, axis: Axis) -> bool {
    with_size(control, axis, |length| matches!(length, Some(Length::Fill)))
}

fn memo_length<R>(
    control: &ResolvedControl,
    slot: u8,
    read: impl FnOnce() -> Option<Length>,
    eval: impl FnOnce(Option<&Length>) -> R,
) -> R {
    let key = (std::ptr::from_ref(control).addr(), slot);
    measure::LENGTHS.with(|memo| {
        let mut memo = memo.borrow_mut();
        let length = memo.entry(key).or_insert_with(read);
        eval(length.as_ref())
    })
}

/// A control's size on `axis`. A missing or non-scalar element is `default`:
/// a label's text size, a ratio-scaled image's texture size, a stack panel's
/// summed children along its axis, else the parent's full extent.
fn length(control: &ResolvedControl, axis: Axis) -> Length {
    let explicit = control.properties.get("size").map(|size| match size {
        Value::Array(items) => items
            .get(axis_index(axis))
            .map_or(Length::Default, expr::length_from_value),
        _ => Length::Default,
    });
    match explicit {
        Some(Length::Default) | None if super::stack_axis(control) == Some(axis) => {
            expr::parse_length("100%c")
        }
        Some(length) => length,
        None => Length::Default,
    }
}

/// A bound rule on `index`, present only as an expression that builds a term: a
/// `%c` term builds one per child, so a childless control's `%c` builds none.
fn bound_length(control: &ResolvedControl, key: &str, index: usize) -> Option<Length> {
    let Value::Array(items) = control.properties.get(key)? else {
        return None;
    };
    let Length::Terms(terms) = expr::length_from_value(items.get(index)?) else {
        return None;
    };
    let sums_children = !control.children.is_empty() && !super::grid::has_template(control);
    let builds = |term: &expr::Term| {
        term.coeff != 0.0 && (term.unit != Unit::PercentChildren || sums_children)
    };
    terms.iter().any(builds).then_some(Length::Terms(terms))
}

/// The natural extent on `axis`: a label's text (wrapped at a known width), or a
/// ratio-scaled image's texture size, one default axis following the other.
fn natural(
    control: &ResolvedControl,
    axis: Axis,
    own: [Option<f64>; 2],
    width: Option<f64>,
    env: &LayoutEnv,
) -> Option<f64> {
    match control.control_type.as_deref() {
        _ if crate::label::is_label(control) => {
            Some(label_extent(control, width, env)[axis_index(axis)])
        }
        Some("image") if scales_to_ratio(control) => {
            let [tw, th] = texture_size(control, env)?;
            let ratio = |numerator: f64, denominator: f64| {
                if denominator == 0.0 {
                    0.0
                } else {
                    numerator / denominator
                }
            };
            let other_default = with_size(control, other(axis), |length| {
                matches!(length, Some(Length::Default))
            });
            Some(match (axis, other_default) {
                (Axis::X, true) => tw,
                (Axis::Y, true) => th,
                (Axis::X, false) => ratio(own[1].unwrap_or(0.0), th) * tw,
                (Axis::Y, false) => ratio(own[0].unwrap_or(0.0), tw) * th,
            })
        }
        _ => None,
    }
}

fn scales_to_ratio(control: &ResolvedControl) -> bool {
    control.control_type.as_deref() == Some("image")
        && widgets::bound_bool(control, "default_size_scales_to_ratio") == Some(true)
}

fn texture_size(control: &ResolvedControl, env: &LayoutEnv) -> Option<[f64; 2]> {
    measure::natural(control, None, || {
        let path = control.properties.get("texture")?.as_str()?;
        env.textures.texture(path).map(|meta| meta.pixels)
    })
}

/// A label's text extent, wrapped at `width` when known.
fn label_extent(control: &ResolvedControl, width: Option<f64>, env: &LayoutEnv) -> [f64; 2] {
    measure::natural(control, width, || {
        Some(crate::label::natural(control, env, width))
    })
    .unwrap_or_default()
}
