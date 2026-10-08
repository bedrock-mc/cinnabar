//! Which state controls a stateful control shows, as vanilla's components
//! write them: each named target is the first
//! descendant with that name, and a shown target shows even if it is itself
//! authored `visible: false`.

use std::collections::VecDeque;

use super::{bound_bool, enabled, prop_str, toggle_checked};
use crate::state::ViewState;
use crate::tree::ResolvedControl;

const BUTTON: [&str; 4] = [
    "locked_control",
    "pressed_control",
    "hover_control",
    "default_control",
];
/// A toggle's state targets by `checked + 4·hover + 8·locked`; unused slots are empty.
const TOGGLE: [&str; 16] = [
    "unchecked_control",
    "checked_control",
    "",
    "",
    "unchecked_hover_control",
    "checked_hover_control",
    "",
    "",
    "unchecked_locked_control",
    "checked_locked_control",
    "",
    "",
    "unchecked_locked_hover_control",
    "checked_locked_hover_control",
    "",
    "",
];
const SLIDER_BOX: [&str; 4] = [
    "default_control",
    "hover_control",
    "locked_control",
    "indent_control",
];
/// Paired normal/hover targets a slider swaps on hover.
const SLIDER_PAIRS: [(&str, &str); 3] = [
    ("default_control", "hover_control"),
    ("background_control", "background_hover_control"),
    ("progress_control", "progress_hover_control"),
];

/// A state control and the interaction states (a mask over [`state_index`]) it shows under.
pub(crate) struct StateTarget<'a> {
    pub control: &'a ResolvedControl,
    pub mask: u8,
}

/// The bit a control's interaction state takes in a [`StateTarget`] mask.
pub(crate) fn state_index(state: &ViewState, key: &str) -> u8 {
    u8::from(state.is_hovered(key))
        | (u8::from(state.is_pressed(key)) << 1)
        | (u8::from(state.is_focused(key)) << 2)
}

/// Whether this control has component-managed visual states.
pub(crate) fn has_state_targets(control: &ResolvedControl) -> bool {
    matches!(
        control.control_type.as_deref(),
        Some("button" | "edit_box" | "toggle" | "dropdown" | "slider" | "selection_wheel")
    )
}

/// Named visual targets and the interaction states under which each shows.
pub(crate) fn state_targets(
    control: &ResolvedControl,
    ancestor_locked: bool,
) -> Vec<StateTarget<'_>> {
    if !has_state_targets(control) {
        return Vec::new();
    }
    if control.control_type.as_deref() == Some("selection_wheel") {
        return crate::component::wheel_visibility(control)
            .into_iter()
            .map(|(control, shown)| StateTarget {
                control,
                mask: if shown { u8::MAX } else { 0 },
            })
            .collect();
    }
    let mut targets: Vec<StateTarget<'_>> = Vec::new();
    let mut resolved = Vec::new();
    for index in 0..8u8 {
        for (target, shown) in writes(control, index, ancestor_locked, &mut resolved) {
            match targets
                .iter_mut()
                .find(|known| std::ptr::eq(known.control, target))
            {
                Some(known) => {
                    known.mask = set(known.mask, index, shown);
                }
                None => targets.push(StateTarget {
                    control: target,
                    mask: set(0, index, shown),
                }),
            }
        }
    }
    targets
}

/// Names of direct children the control hides at rest (no hover, press or
/// focus): they add nothing to its `%c`/`%cm`.
pub(crate) fn rest_hidden_children(control: &ResolvedControl) -> Vec<&str> {
    writes(control, 0, false, &mut Vec::new())
        .into_iter()
        .filter(|(target, shown)| {
            !shown
                && control
                    .children
                    .iter()
                    .any(|child| std::ptr::eq(child, *target))
        })
        .map(|(target, _)| target.name.as_str())
        .collect()
}

fn set(mask: u8, index: u8, shown: bool) -> u8 {
    if shown {
        mask | (1 << index)
    } else {
        mask & !(1 << index)
    }
}

/// The ordered visibility writes for interaction `bits`, resolved so the
/// last write to a shared target wins, as in vanilla.
fn writes<'a>(
    control: &'a ResolvedControl,
    bits: u8,
    ancestor_locked: bool,
    cache: &mut Targets<'a>,
) -> Vec<(&'a ResolvedControl, bool)> {
    if control.control_type.as_deref() == Some("selection_wheel") {
        return crate::component::wheel_visibility(control);
    }
    if !matches!(
        control.control_type.as_deref(),
        Some("button" | "edit_box" | "toggle" | "dropdown" | "slider")
    ) {
        return Vec::new();
    }
    let (hovered, pressed, focused) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
    let locked = ancestor_locked || !enabled(control);
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    match control.control_type.as_deref().unwrap_or("") {
        "button" | "edit_box" => {
            let selected = bound_bool(control, "#text_edit_selected") == Some(true)
                || (control.control_type.as_deref() == Some("edit_box") && focused);
            if locked {
                let has_locked = prop_str(control, "locked_control").is_some();
                out.extend([
                    ("locked_control", true),
                    ("default_control", !has_locked),
                    ("pressed_control", false),
                    ("hover_control", false),
                ]);
            } else {
                let shown = if pressed || selected {
                    "pressed_control"
                } else if hovered {
                    "hover_control"
                } else {
                    "default_control"
                };
                out.extend(BUTTON.map(|name| (name, name == shown)));
            }
        }
        "toggle" | "dropdown" => {
            let index = usize::from(toggle_checked(control))
                + 4 * usize::from(hovered)
                + 8 * usize::from(locked);
            out.extend(
                TOGGLE
                    .iter()
                    .filter(|name| !name.is_empty())
                    .map(|name| (*name, false)),
            );
            out.push((TOGGLE[index], true));
        }
        "slider" => {
            let hover = hovered || focused;
            for (normal, over) in SLIDER_PAIRS {
                let paired = prop_str(control, over).is_some();
                if paired || normal == "default_control" {
                    out.push((normal, !hover || !paired));
                }
                out.push((over, hover));
            }
            let mut resolved = resolve(control, &out, cache);
            if let Some(box_control) = target(control, "slider_box_control", cache) {
                let selected = bound_bool(control, super::SLIDER_BOX_SELECTED) == Some(true);
                let shown = if locked {
                    "locked_control"
                } else if selected {
                    "indent_control"
                } else if hover {
                    "hover_control"
                } else {
                    "default_control"
                };
                let box_writes: Vec<(&'static str, bool)> = SLIDER_BOX
                    .iter()
                    .map(|name| (*name, *name == shown))
                    .collect();
                resolved.extend(resolve(box_control, &box_writes, cache));
            }
            return resolved;
        }
        _ => return Vec::new(),
    }
    resolve(control, &out, cache)
}

/// Property-named writes as target controls, last write per target winning.
fn resolve<'a>(
    control: &'a ResolvedControl,
    writes: &[(&'static str, bool)],
    cache: &mut Targets<'a>,
) -> Vec<(&'a ResolvedControl, bool)> {
    let mut out: Vec<(&ResolvedControl, bool)> = Vec::new();
    for (property, shown) in writes {
        let Some(target) = target(control, property, cache) else {
            continue;
        };
        match out
            .iter_mut()
            .find(|(known, _)| std::ptr::eq(*known, target))
        {
            Some(entry) => entry.1 = *shown,
            None => out.push((target, *shown)),
        }
    }
    out
}

/// The first descendant of `control` named `name`, breadth first.
fn descendant<'a>(control: &'a ResolvedControl, name: &str) -> Option<&'a ResolvedControl> {
    let mut queue: VecDeque<&ResolvedControl> = control.children.iter().collect();
    while let Some(next) = queue.pop_front() {
        if next.name == name {
            return Some(next);
        }
        queue.extend(next.children.iter());
    }
    None
}

/// Targets resolved once while computing all eight interaction masks.
type Targets<'a> = Vec<(usize, &'static str, Option<&'a ResolvedControl>)>;

/// Reuse a property’s descendant search across interaction states.
fn target<'a>(
    control: &'a ResolvedControl,
    property: &'static str,
    cache: &mut Targets<'a>,
) -> Option<&'a ResolvedControl> {
    let address = std::ptr::from_ref(control).addr();
    if let Some((_, _, target)) = cache
        .iter()
        .find(|(at, name, _)| *at == address && *name == property)
    {
        return *target;
    }
    let found = prop_str(control, property).and_then(|name| descendant(control, name));
    cache.push((address, property, found));
    found
}
