//! Keyboard/gamepad focus movement over hit regions, after the 1.26.50
//! `FocusManager`: default focus by precedence, identifier overrides, the
//! directional sweep (`_sweepForControlDirectional`), scroll sections, and
//! focus-container rules (`_handleFocusContainerLogic`).

use crate::emit::RectOut;
use crate::state::{FocusMemory, ViewState};

use super::focus::{FOCUS_OVERRIDE_STOP, FocusContainer, FocusDirection, NavigationMode};
use super::{HitKind, HitRegion, focus_order};

/// Minimum cosine a candidate needs toward the sweep direction.
const SWEEP_CONE: f64 = 0.02;
/// How far into its own edge the sweep starts (`_sweepToNextFocusObject`).
const EDGE_INSET: f64 = 2.0;
/// Distances at or below this aim straight ahead.
const DIRECTION_EPSILON: f64 = 1.192_092_9e-7;

/// The outcome of a directional press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusMove {
    Moved(String),
    Stayed,
    /// The focused control takes the direction itself (a controller-direction event).
    Claimed(String),
}

/// The default focus: highest `default_focus_precedence` (never below zero),
/// ties to the first in reading order.
pub fn default_focus(regions: &[HitRegion], screen_width: f64) -> Option<&HitRegion> {
    let candidates = focus_order(regions);
    let precedence = |region: &HitRegion| region.focus.as_ref().map_or(0, |f| f.precedence);
    let best = candidates.iter().map(|r| precedence(r)).fold(0, i32::max);
    candidates
        .into_iter()
        .filter(|region| precedence(region) == best)
        .min_by(|a, b| {
            let rank = |r: &HitRegion| r.rect.y * screen_width + r.rect.x;
            rank(a).total_cmp(&rank(b))
        })
}

/// The next (or previous) admitted region in document order, wrapping.
pub fn next_in_order<'a>(
    regions: &'a [HitRegion],
    focused: Option<&str>,
    backwards: bool,
) -> Option<&'a HitRegion> {
    let order = focus_order(regions);
    let count = order.len();
    let at = focused.and_then(|key| order.iter().position(|region| region.key == key));
    let next = match (at, backwards) {
        (None, false) => 0,
        (None, true) => count.checked_sub(1)?,
        (Some(at), false) => (at + 1) % count,
        (Some(at), true) => (at + count - 1) % count,
    };
    order.get(next).copied()
}

/// Whether `region` takes a controller direction instead of focus moving
/// (`always_handle_controller_direction`).
pub fn controller_direction_claimed(region: &HitRegion) -> bool {
    region.input.always_handle_controller_direction
}

/// Where focus goes from `state.focused` toward `direction` on a `screen`.
pub fn navigate(
    regions: &[HitRegion],
    state: &ViewState,
    direction: FocusDirection,
    screen: RectOut,
) -> FocusMove {
    let candidates = focus_order(regions);
    let Some(current) = state
        .focused
        .as_deref()
        .and_then(|key| candidates.iter().copied().find(|region| region.key == key))
    else {
        return moved(default_focus(regions, screen.w));
    };
    if controller_direction_claimed(current) {
        return FocusMove::Claimed(current.key.clone());
    }
    let over = override_toward(regions, current, direction);
    if over == FOCUS_OVERRIDE_STOP {
        return FocusMove::Stayed;
    }
    if !over.is_empty()
        && let Some(target) = candidates.iter().find(|region| identifier(region) == over)
    {
        return FocusMove::Moved(target.key.clone());
    }
    let target = sweep_from(regions, &candidates, current, direction, screen);
    contain(&candidates, current, target, direction, &state.focus_memory)
}

/// Record `key` as focused, keeping the containers' last focus and whether
/// the control focus left keeps its look (`reset_on_focus_lost`).
pub fn set_focus(state: &mut ViewState, regions: &[HitRegion], key: Option<String>) {
    if state.focused == key {
        return;
    }
    let find = |key: &str| regions.iter().find(|region| region.key == key);
    let left = state.focused.as_deref().and_then(find);
    state.focus_memory.held = left
        .filter(|region| {
            region
                .focus
                .as_ref()
                .is_some_and(|f| !f.reset_on_focus_lost)
        })
        .map(|region| region.key.clone());
    if let Some(focus) = key.as_deref().and_then(find).and_then(|r| r.focus.as_ref()) {
        for container in &focus.containers {
            state
                .focus_memory
                .last
                .insert(container.key.clone(), key.clone().unwrap_or_default());
        }
    }
    state.focused = key;
}

fn moved(region: Option<&HitRegion>) -> FocusMove {
    region.map_or(FocusMove::Stayed, |region| {
        FocusMove::Moved(region.key.clone())
    })
}

fn identifier(region: &HitRegion) -> &str {
    region.focus.as_ref().map_or("", |f| f.identifier.as_str())
}

/// `current`'s own override, else a `focus_mapping` entry for its identifier.
fn override_toward<'a>(
    regions: &'a [HitRegion],
    current: &'a HitRegion,
    direction: FocusDirection,
) -> &'a str {
    let Some(focus) = current.focus.as_ref() else {
        return "";
    };
    let own = focus.change_toward(direction);
    if !own.is_empty() || focus.identifier.is_empty() {
        return own;
    }
    let side = FocusDirection::ALL
        .iter()
        .position(|side| *side == direction)
        .unwrap_or(0);
    regions
        .iter()
        .filter_map(|region| region.focus.as_ref())
        .flat_map(|f| f.mapping.iter())
        .find(|(id, _)| *id == focus.identifier)
        .map_or("", |(_, change)| change[side].as_str())
}

/// Scroll sections first, innermost out, then the whole screen.
fn sweep_from<'a>(
    regions: &'a [HitRegion],
    candidates: &[&'a HitRegion],
    current: &HitRegion,
    direction: FocusDirection,
    screen: RectOut,
) -> Option<&'a HitRegion> {
    let mut sections: Vec<&HitRegion> = regions
        .iter()
        .filter(|view| view.kind == HitKind::ScrollView && under(&current.key, &view.key))
        .collect();
    sections.sort_by_key(|view| std::cmp::Reverse(view.key.len()));
    for view in &sections {
        let inside: Vec<&HitRegion> = candidates
            .iter()
            .copied()
            .filter(|region| under(&region.key, &view.key))
            .collect();
        let bounds = intersect(view.rect, view.clip);
        if let Some(found) = sweep(&inside, current, direction, bounds, false, false) {
            return Some(found);
        }
    }
    // A control in a scroll section never wraps across the screen.
    let wrap = sections.is_empty() && current.focus.as_ref().is_some_and(|f| f.wrap);
    let outside: Vec<&HitRegion> = candidates
        .iter()
        .copied()
        .filter(|region| {
            sections
                .first()
                .is_none_or(|view| !under(&region.key, &view.key))
        })
        .collect();
    sweep(&outside, current, direction, screen, wrap, true)
}

/// The container rules for leaving `current`'s containers toward `target`,
/// then entering `target`'s.
fn contain<'a>(
    candidates: &[&'a HitRegion],
    current: &HitRegion,
    target: Option<&'a HitRegion>,
    direction: FocusDirection,
    memory: &FocusMemory,
) -> FocusMove {
    let containers = current.focus.as_ref().map_or(&[][..], |f| &f.containers);
    for container in containers.iter().rev() {
        if target.is_some_and(|target| container.holds(&target.key)) {
            break;
        }
        match container.mode(direction) {
            NavigationMode::Free => continue,
            NavigationMode::Stop => return FocusMove::Stayed,
            NavigationMode::Contained => {
                let inside: Vec<&HitRegion> = candidates
                    .iter()
                    .copied()
                    .filter(|region| container.holds(&region.key))
                    .collect();
                return moved(sweep(
                    &inside,
                    current,
                    direction,
                    container.rect,
                    container.wrap,
                    true,
                ));
            }
            NavigationMode::Custom => {
                return moved(route(candidates, current, container, direction, memory));
            }
        }
    }
    let Some(target) = target else {
        return FocusMove::Stayed;
    };
    let entered = target
        .focus
        .as_ref()
        .and_then(|f| f.containers.iter().find(|c| !c.holds(&current.key)));
    if let Some(container) = entered.filter(|c| c.use_last_focus)
        && let Some(last) = memory.last.get(&container.key)
        && candidates.iter().any(|region| region.key == *last)
    {
        return FocusMove::Moved(last.clone());
    }
    FocusMove::Moved(target.key.clone())
}

/// A `custom` container side: the first route whose container can take focus.
fn route<'a>(
    candidates: &[&'a HitRegion],
    current: &HitRegion,
    from: &FocusContainer,
    direction: FocusDirection,
    memory: &FocusMemory,
) -> Option<&'a HitRegion> {
    for route in from.routes(direction) {
        let Some(container) = candidates
            .iter()
            .filter_map(|region| region.focus.as_ref())
            .flat_map(|f| f.containers.iter())
            .find(|c| c.name == route.container)
        else {
            continue;
        };
        let inside = || {
            candidates
                .iter()
                .copied()
                .filter(|region| container.holds(&region.key))
        };
        if !route.inside.is_empty() {
            if let Some(found) = inside().find(|region| identifier(region) == route.inside) {
                return Some(found);
            }
            continue;
        }
        let nested = from.holds(&container.key) || container.holds(&from.key);
        if container.use_last_focus
            && !nested
            && let Some(last) = memory.last.get(&container.key)
            && let Some(found) = inside().find(|region| region.key == *last)
        {
            return Some(found);
        }
        let origin = center(&current.rect);
        if let Some(found) = inside().min_by(|a, b| {
            distance2(center(&a.rect), origin).total_cmp(&distance2(center(&b.rect), origin))
        }) {
            return Some(found);
        }
    }
    None
}

/// `_sweepForControlDirectional`: the nearest candidate ahead of `current`'s
/// leading edge within the cone, retrying from the far side of `bounds` when
/// `wrap`; `clipped` drops candidates with no corner inside their clip.
fn sweep<'a>(
    candidates: &[&'a HitRegion],
    current: &HitRegion,
    direction: FocusDirection,
    bounds: RectOut,
    wrap: bool,
    clipped: bool,
) -> Option<&'a HitRegion> {
    let rect = current.rect;
    let (cx, cy) = (rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    let (hw, hh) = (rect.w * 0.5, rect.h * 0.5);
    let origin = [cx, cy];
    let start = match direction {
        FocusDirection::Up => [cx, cy - (hh - EDGE_INSET)],
        FocusDirection::Down => [cx, cy + hh - EDGE_INSET],
        FocusDirection::Left => [cx - (hw - EDGE_INSET), cy],
        FocusDirection::Right => [cx + hw - EDGE_INSET, cy],
    };
    let found = sweep_at(candidates, current, direction, origin, start, clipped);
    if found.is_some() || !wrap {
        return found;
    }
    let from = match direction {
        FocusDirection::Up => [cx, bounds.y + bounds.h],
        FocusDirection::Down => [cx, bounds.y],
        FocusDirection::Left => [bounds.x + bounds.w, cy],
        FocusDirection::Right => [bounds.x, cy],
    };
    sweep_at(candidates, current, direction, from, from, clipped)
}

fn sweep_at<'a>(
    candidates: &[&'a HitRegion],
    current: &HitRegion,
    direction: FocusDirection,
    origin: [f64; 2],
    start: [f64; 2],
    clipped: bool,
) -> Option<&'a HitRegion> {
    let axis = match direction {
        FocusDirection::Up => [0.0, -1.0],
        FocusDirection::Down => [0.0, 1.0],
        FocusDirection::Left => [-1.0, 0.0],
        FocusDirection::Right => [1.0, 0.0],
    };
    // The cone narrows to the current control's own corner when that is flatter.
    let rect = current.rect;
    let corner = match direction {
        FocusDirection::Up | FocusDirection::Left => [rect.x, rect.y],
        FocusDirection::Down | FocusDirection::Right => [rect.x + rect.w, rect.y + rect.h],
    };
    let to_corner = [corner[0] - origin[0], corner[1] - origin[1]];
    let length = distance2(corner, origin).sqrt();
    let reach = if length > 1.0e-4 {
        (to_corner[0] * axis[0] + to_corner[1] * axis[1]) / length
    } else {
        axis[0] * to_corner[0] + axis[1] * to_corner[1]
    };
    let cone = reach.min(SWEEP_CONE);
    let mut best: Option<(&HitRegion, f64)> = None;
    let mut ordered = candidates.to_vec();
    ordered.sort_by_key(|region| region.order);
    for candidate in ordered {
        if candidate.key == current.key || (clipped && !any_corner_inside(candidate)) {
            continue;
        }
        let r = candidate.rect;
        let (x0, x1, y0, y1) = (r.x, r.x + r.w, r.y, r.y + r.h);
        let point = match direction {
            FocusDirection::Up => [start[0].clamp(x0, x1.max(x0)), y1],
            FocusDirection::Down => [start[0].clamp(x0, x1.max(x0)), y0],
            FocusDirection::Left => [x1, start[1].clamp(y0, y1.max(y0))],
            FocusDirection::Right => [x0, start[1].clamp(y0, y1.max(y0))],
        };
        let delta = [point[0] - start[0], point[1] - start[1]];
        let distance = distance2(point, start).sqrt();
        let cosine = if distance > DIRECTION_EPSILON {
            (delta[0] * axis[0] + delta[1] * axis[1]) / distance
        } else {
            1.0
        };
        if cosine < cone {
            continue;
        }
        if best.is_none_or(|(_, nearest)| distance < nearest) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(region, _)| region)
}

fn any_corner_inside(region: &HitRegion) -> bool {
    let (r, c) = (region.rect, region.clip);
    [
        [r.x, r.y],
        [r.x + r.w, r.y],
        [r.x, r.y + r.h],
        [r.x + r.w, r.y + r.h],
    ]
    .iter()
    .any(|p| p[0] >= c.x && p[0] <= c.x + c.w && p[1] >= c.y && p[1] <= c.y + c.h)
}

fn under(key: &str, ancestor: &str) -> bool {
    key.strip_prefix(ancestor)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn intersect(a: RectOut, b: RectOut) -> RectOut {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    RectOut {
        x: x0,
        y: y0,
        w: ((a.x + a.w).min(b.x + b.w) - x0).max(0.0),
        h: ((a.y + a.h).min(b.y + b.h) - y0).max(0.0),
    }
}

fn center(rect: &RectOut) -> [f64; 2] {
    [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5]
}

fn distance2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
