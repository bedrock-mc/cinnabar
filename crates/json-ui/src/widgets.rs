//! Engine-driven control behaviour that the templates only name: which state
//! child of a button/toggle/edit box/slider shows, where a slider's box sits and
//! how much of its progress bar is revealed. The templates supply the child
//! names (`default_control`, `checked_hover_control`, …); the state comes from
//! the bound `#` values and the caller's [`ViewState`].

use serde_json::Value;

use crate::layout::Rect;
use crate::tree::ResolvedControl;

mod scroll;
mod scroll_motion;

pub use scroll::Draggable;
pub(crate) use scroll::OVERSCROLL;
pub use scroll_motion::ScrollMotion;

mod states;

pub(crate) use states::{has_state_targets, rest_hidden_children, state_index, state_targets};

/// The slider bag value holding its box's selected (indent) state.
pub(crate) use crate::component::SLIDER_BOX_SELECTED;

pub(crate) fn prop_str<'a>(control: &'a ResolvedControl, key: &str) -> Option<&'a str> {
    control
        .properties
        .get(key)
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

/// A bound boolean: literal bool, `"true"`/`"false"`, or `None`.
pub(crate) fn bound_bool(control: &ResolvedControl, key: &str) -> Option<bool> {
    match control.properties.get(key)? {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) if text == "true" => Some(true),
        Value::String(text) if text == "false" => Some(false),
        _ => None,
    }
}

/// Reads a finite numeric property, including a numeric string.
pub(crate) fn bound_number(control: &ResolvedControl, key: &str) -> Option<f64> {
    match control.properties.get(key)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

/// Whether the control accepts input: `enabled`/`#enabled` false locks it.
pub(crate) fn enabled(control: &ResolvedControl) -> bool {
    bound_bool(control, "#enabled")
        .or_else(|| bound_bool(control, "enabled"))
        .unwrap_or(true)
}

/// An `edit_box`'s `place_holder_control` to hide: vanilla shows it only while
/// its `text_control` is empty.
pub(crate) fn hidden_placeholder(control: &ResolvedControl) -> Option<&str> {
    if control.control_type.as_deref() != Some("edit_box") {
        return None;
    }
    let placeholder = prop_str(control, "place_holder_control")?;
    let text_control = prop_str(control, "text_control")?;
    let text = control
        .find(&|child| child.name == text_control)?
        .properties
        .get("text")?
        .as_str()?;
    (!text.is_empty()).then_some(placeholder)
}

/// A toggle's checked state: the bound `#toggle_state`, else its default.
pub(crate) fn toggle_checked(control: &ResolvedControl) -> bool {
    bound_bool(control, "#toggle_state")
        .or_else(|| bound_bool(control, "toggle_default_state"))
        .unwrap_or(false)
}

/// A slider's normalized position `0..=1`: a step slider's `#slider_value` is the
/// step index over `#slider_steps`, a continuous slider's value is already a
/// fraction.
fn slider_fraction(control: &ResolvedControl) -> Option<f64> {
    if control.control_type.as_deref() != Some("slider") {
        return None;
    }
    let value = bound_number(control, "#slider_value").unwrap_or(0.0);
    let steps = bound_number(control, "#slider_steps")
        .or_else(|| bound_number(control, "slider_steps"))
        .unwrap_or(1.0);
    let fraction = if steps > 1.0 {
        value / (steps - 1.0)
    } else {
        value
    };
    let fraction = fraction.clamp(0.0, 1.0);
    Some(if bound_bool(control, "slider_inverted") == Some(true) {
        1.0 - fraction
    } else {
        fraction
    })
}

/// A slider being laid out: where its box travels and which children it clips.
pub(crate) struct SliderFrame {
    pub fraction: f64,
    /// `slider_box_control`, `progress_control`, `progress_hover_control`.
    pub names: [Option<String>; 3],
    pub rect: Rect,
    pub vertical: bool,
}

impl SliderFrame {
    pub fn open(control: &ResolvedControl, rect: Rect) -> Option<Self> {
        Some(Self {
            fraction: slider_fraction(control)?,
            names: [
                prop_str(control, "slider_box_control").map(str::to_owned),
                prop_str(control, "progress_control").map(str::to_owned),
                prop_str(control, "progress_hover_control").map(str::to_owned),
            ],
            rect,
            vertical: prop_str(control, "slider_direction") == Some("vertical"),
        })
    }

    /// The box's rect: its centre travels the full slider along its axis.
    pub fn place_box(&self, box_rect: Rect) -> Rect {
        let track = self.rect;
        if self.vertical {
            return Rect::new(
                box_rect.x,
                track.y + track.h * self.fraction - box_rect.h * 0.5,
                box_rect.w,
                box_rect.h,
            );
        }
        Rect::new(
            track.x + track.w * self.fraction - box_rect.w * 0.5,
            box_rect.y,
            box_rect.w,
            box_rect.h,
        )
    }
}

/// A panel holding a `dropdown`: the dropdown's name, its `dropdown_area` and
/// its content sibling's name (`DropdownComponent`).
pub(crate) fn dropdown_area(control: &ResolvedControl) -> Option<(String, String, String)> {
    control.children.iter().find_map(|child| {
        if child.control_type.as_deref() != Some("dropdown") {
            return None;
        }
        let area = prop_str(child, "dropdown_area")?;
        let content = prop_str(child, "dropdown_content_control").unwrap_or("dropdown_content");
        Some((child.name.clone(), area.to_owned(), content.to_owned()))
    })
}

/// The content's top as `DropdownComponent::_positionContent` places it:
/// level with the dropdown, raised to end inside the area, never above it,
/// and centred on the area when taller than it.
pub(crate) fn dropdown_content_top(dropdown: Rect, area: Rect, content_height: f64) -> f64 {
    if area.h <= content_height {
        return area.y + area.h * 0.5 - content_height * 0.5;
    }
    let raised = if area.h + area.y < content_height + dropdown.y {
        area.h + area.y - content_height
    } else {
        dropdown.y
    };
    raised.max(area.y)
}
