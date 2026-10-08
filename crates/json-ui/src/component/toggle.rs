//! The toggle component and the toggle manager, as the 1.26.50 factory reads them.

use serde_json::Value;

use super::text_of;
use crate::tree::ResolvedControl;
use crate::widgets::{bound_bool, bound_number};

/// What a toggle's component reads at creation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToggleMeta {
    pub name: Option<String>,
    pub radio: bool,
    pub default_state: bool,
    /// `toggle_group_forced_index`; negative reads the collection index.
    pub forced_index: i64,
    pub default_selected: i64,
    pub on_hover: bool,
    pub directional: bool,
    pub on_button: Option<String>,
    pub off_button: Option<String>,
    pub grid_collection: Option<String>,
    pub descriptive_text: Option<String>,
    pub tts_on: Option<String>,
    pub tts_off: Option<String>,
    /// The state the control was laid out with (bound `#toggle_state` or default).
    pub checked: bool,
}

impl ToggleMeta {
    pub(crate) fn read(control: &ResolvedControl) -> Self {
        let int = |key: &str, fallback: i64| {
            bound_number(control, key)
                .filter(|value| value.fract() == 0.0)
                .map_or(fallback, |value| value as i64)
        };
        ToggleMeta {
            name: text_of(control, "toggle_name"),
            radio: bound_bool(control, "radio_toggle_group").unwrap_or(false),
            default_state: bound_bool(control, "toggle_default_state").unwrap_or(false),
            forced_index: int("toggle_group_forced_index", -1),
            default_selected: int("toggle_group_default_selected", 0),
            // The factory writes the literal into `#toggle_on_hover`, which a binding may replace.
            on_hover: bound_bool(control, "#toggle_on_hover")
                .or_else(|| bound_bool(control, "toggle_on_hover"))
                .unwrap_or(false),
            directional: bound_bool(control, "enable_directional_toggling").unwrap_or(false),
            on_button: text_of(control, "toggle_on_button"),
            off_button: text_of(control, "toggle_off_button"),
            grid_collection: text_of(control, "toggle_grid_collection_name"),
            descriptive_text: text_of(control, "descriptive_text"),
            tts_on: text_of(control, "tts_toggle_on"),
            tts_off: text_of(control, "tts_toggle_off"),
            checked: crate::widgets::toggle_checked(control),
        }
    }

    /// The TTS value vanilla reads for the current state.
    pub fn tts_value(&self, checked: bool) -> Option<&str> {
        if checked {
            self.tts_on.as_deref()
        } else {
            self.tts_off.as_deref()
        }
    }
}

/// What a toggle does with a button event reaching it: the new state and whether a click set it, or
/// `None` when it ignores the event.
pub(crate) fn on_button(
    meta: &ToggleMeta,
    id: &str,
    interacted: bool,
    checked: bool,
) -> Option<(bool, bool)> {
    if !interacted {
        return None;
    }
    let on = meta.on_button.as_deref() == Some(id);
    let off = meta.off_button.as_deref() == Some(id);
    if on || off {
        if !meta.directional {
            return None;
        }
        return match (on, meta.radio) {
            (true, _) => (!checked).then_some((true, true)),
            (false, false) => checked.then_some((false, true)),
            (false, true) => checked.then_some((true, true)),
        };
    }
    if meta.radio {
        // A radio member only ever turns on; the group turns the others off.
        (!checked).then_some((true, true))
    } else {
        Some((!checked, true))
    }
}

/// The toggle's reaction to its hover turning on: `toggle_on_hover` checks a
/// radio member and flips a plain toggle, neither counting as a click.
pub(crate) fn on_hover(meta: &ToggleMeta, checked: bool) -> Option<bool> {
    if !meta.on_hover {
        return None;
    }
    if meta.radio {
        (!checked).then_some(true)
    } else {
        Some(!checked)
    }
}

/// A controller direction on a directional toggle: right turns it on, left off.
pub(crate) fn on_direction(meta: &ToggleMeta, x: f32, y: f32, checked: bool) -> Option<bool> {
    // The deflection must clear the dead zone and dominate the other axis.
    if !meta.directional || x.abs() <= DIRECTION_DEAD_ZONE || x.abs() <= y.abs() {
        return None;
    }
    match (x > 0.0, meta.radio) {
        (true, _) => (!checked).then_some(true),
        (false, false) => checked.then_some(false),
        (false, true) => None,
    }
}

/// Stick deflection a directional toggle ignores.
const DIRECTION_DEAD_ZONE: f32 = 0.8;

/// `toggle_manager_behavior` over `toggle_manage_groups`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToggleManager {
    pub behavior: ManagerBehavior,
    pub groups: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagerBehavior {
    Clear,
    Select,
    Gather,
    Default,
}

impl ToggleManager {
    pub(crate) fn read(control: &ResolvedControl) -> Option<Self> {
        let behavior = match control.properties.get("toggle_manager_behavior")? {
            Value::String(text) => match text.as_str() {
                "clear" => ManagerBehavior::Clear,
                "select" => ManagerBehavior::Select,
                "default" => ManagerBehavior::Default,
                // Unknown names assert in vanilla and fall back to gather.
                _ => ManagerBehavior::Gather,
            },
            _ => ManagerBehavior::Gather,
        };
        let groups = match control.properties.get("toggle_manage_groups") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        Some(ToggleManager { behavior, groups })
    }

    /// Whether a gather manager republishes `meta`'s state.
    pub(crate) fn gathers(&self, meta: &ToggleMeta) -> bool {
        self.behavior == ManagerBehavior::Gather
            && meta
                .name
                .as_ref()
                .is_some_and(|name| self.groups.contains(name))
    }

    /// The state a managed toggle takes when the manager acts, or `None` to keep it.
    pub(crate) fn state_for(&self, meta: &ToggleMeta, index: Option<usize>) -> Option<bool> {
        if !meta
            .name
            .as_ref()
            .is_some_and(|name| self.groups.contains(name))
        {
            return None;
        }
        match self.behavior {
            ManagerBehavior::Clear => Some(false),
            ManagerBehavior::Select => Some(true),
            ManagerBehavior::Default => Some(if meta.radio {
                index.map(|at| at as i64) == Some(meta.default_selected)
            } else {
                meta.default_state
            }),
            ManagerBehavior::Gather => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(radio: bool) -> ToggleMeta {
        ToggleMeta {
            radio,
            on_button: Some("toggle.toggle_on".into()),
            off_button: Some("toggle.toggle_off".into()),
            ..ToggleMeta::default()
        }
    }

    // A click flips a plain toggle; a radio member only turns on.
    #[test]
    fn clicks_flip_plain_toggles_and_select_radio_members() {
        assert_eq!(
            on_button(&meta(false), "button.menu_select", true, false),
            Some((true, true))
        );
        assert_eq!(
            on_button(&meta(false), "button.menu_select", true, true),
            Some((false, true))
        );
        assert_eq!(
            on_button(&meta(true), "button.menu_select", true, true),
            None
        );
        assert_eq!(
            on_button(&meta(false), "button.menu_select", false, false),
            None
        );
    }

    // On/off ids act only when directional toggling is enabled.
    #[test]
    fn on_off_buttons_need_directional_toggling() {
        let mut toggle = meta(false);
        assert_eq!(on_button(&toggle, "toggle.toggle_on", true, false), None);
        toggle.directional = true;
        assert_eq!(
            on_button(&toggle, "toggle.toggle_on", true, false),
            Some((true, true))
        );
        assert_eq!(
            on_button(&toggle, "toggle.toggle_off", true, true),
            Some((false, true))
        );
        assert_eq!(on_direction(&toggle, 0.9, 0.1, false), Some(true));
        assert_eq!(on_direction(&toggle, 0.2, 0.1, false), None);
    }
}
