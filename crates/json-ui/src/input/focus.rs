//! A control's focus component and the focus containers enclosing it.

use serde_json::Value;

use crate::emit::RectOut;
use crate::tree::ResolvedControl;
use crate::widgets::{bound_bool, bound_number};

/// `ui::CardinalDirection`, less `None` (a sweep to a point).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Up,
    Down,
    Left,
    Right,
}

impl FocusDirection {
    pub const ALL: [FocusDirection; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    fn index(self) -> usize {
        self as usize
    }
}

/// The override that pins focus in place (`FOCUS_OVERRIDE_STOP`).
pub const FOCUS_OVERRIDE_STOP: &str = "FOCUS_OVERRIDE_STOP";

/// What focus navigation reads from one control.
#[derive(Clone, Debug, PartialEq)]
pub struct FocusMeta {
    /// `focus_enabled`, or its bound `#focus_enabled`.
    pub enabled: bool,
    pub precedence: i32,
    pub wrap: bool,
    pub magnet: bool,
    pub identifier: String,
    pub reset_on_focus_lost: bool,
    /// `focus_change_up/down/left/right`; empty means no override.
    pub change: [String; 4],
    /// `focus_mapping`: overrides for whichever control carries each identifier.
    pub mapping: Vec<(String, [String; 4])>,
    /// Enclosing focus containers, outermost first.
    pub containers: Vec<FocusContainer>,
}

impl FocusMeta {
    /// The focus component `control` carries, `None` for types without one.
    pub(crate) fn read(control: &ResolvedControl, containers: &[FocusContainer]) -> Option<Self> {
        let has_component = matches!(
            control.control_type.as_deref()?,
            "button"
                | "toggle"
                | "dropdown"
                | "slider"
                | "edit_box"
                | "input_panel"
                | "scroll_view"
                | "selection_wheel"
                | "custom"
        );
        if !has_component {
            return None;
        }
        let cached = super::cache::entry(&control.properties);
        let mut focus = cached.focus.get_or_init(|| Self::parse(control)).clone();
        focus.containers.extend_from_slice(containers);
        Some(focus)
    }

    /// Cache authored focus rules; ancestor geometry belongs to the current layout.
    fn parse(control: &ResolvedControl) -> Self {
        let flag = |key: &str, fallback: bool| bound_bool(control, key).unwrap_or(fallback);
        Self {
            enabled: bound_bool(control, "#focus_enabled").unwrap_or(flag("focus_enabled", false)),
            // A non-integer precedence reads as zero, as in vanilla.
            precedence: bound_number(control, "default_focus_precedence")
                .filter(|value| value.fract() == 0.0)
                .map_or(0, |value| value as i32),
            wrap: flag("focus_wrap_enabled", true),
            magnet: flag("focus_magnet_enabled", false),
            identifier: text(control.properties.get("focus_identifier")),
            reset_on_focus_lost: flag("reset_on_focus_lost", true),
            change: changes(|key| control.properties.get(key)),
            mapping: match control.properties.get("focus_mapping") {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(Value::as_object)
                    .map(|item| {
                        (
                            text(item.get("focus_identifier")),
                            changes(|key| item.get(key)),
                        )
                    })
                    .filter(|(identifier, _)| !identifier.is_empty())
                    .collect(),
                _ => Vec::new(),
            },
            containers: Vec::new(),
        }
    }

    /// This control's own override toward `direction`.
    pub fn change_toward(&self, direction: FocusDirection) -> &str {
        &self.change[direction.index()]
    }

    /// The innermost enclosing container.
    pub fn container(&self) -> Option<&FocusContainer> {
        self.containers.last()
    }
}

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_owned()
}

fn changes<'a>(get: impl Fn(&str) -> Option<&'a Value>) -> [String; 4] {
    ["up", "down", "left", "right"].map(|side| text(get(&format!("focus_change_{side}"))))
}

/// `FocusNavigationMode`: what leaving a container toward a side does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavigationMode {
    /// `""` or `"none"`: navigation leaves freely.
    #[default]
    Free,
    Contained,
    Stop,
    Custom,
}

/// One `focus_container_custom_*` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomRoute {
    pub container: String,
    /// `focus_id_inside`; empty enters the container by its own rules.
    pub inside: String,
}

/// A `focus_container` control as navigation needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct FocusContainer {
    pub key: String,
    pub name: String,
    pub rect: RectOut,
    pub use_last_focus: bool,
    pub wrap: bool,
    pub modes: [NavigationMode; 4],
    pub custom: [Vec<CustomRoute>; 4],
}

impl FocusContainer {
    /// The container `control` declares, `None` without `focus_container`.
    pub(crate) fn read(control: &ResolvedControl, key: &str, rect: RectOut) -> Option<Self> {
        if !bound_bool(control, "focus_container").unwrap_or(false) {
            return None;
        }
        let sides = ["up", "down", "left", "right"];
        Some(Self {
            key: key.to_owned(),
            name: control.name.clone(),
            rect,
            use_last_focus: bound_bool(control, "use_last_focus").unwrap_or(false),
            wrap: bound_bool(control, "focus_wrap_enabled").unwrap_or(true),
            modes: sides.map(|side| {
                match control
                    .properties
                    .get(&format!("focus_navigation_mode_{side}"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                {
                    "contained" => NavigationMode::Contained,
                    "stop" => NavigationMode::Stop,
                    "custom" => NavigationMode::Custom,
                    _ => NavigationMode::Free,
                }
            }),
            custom: sides.map(|side| {
                match control
                    .properties
                    .get(&format!("focus_container_custom_{side}"))
                {
                    Some(Value::Array(items)) => items
                        .iter()
                        .filter_map(Value::as_object)
                        .map(|item| CustomRoute {
                            container: text(item.get("other_focus_container_name")),
                            inside: text(item.get("focus_id_inside")),
                        })
                        .filter(|route| !route.container.is_empty())
                        .collect(),
                    _ => Vec::new(),
                }
            }),
        })
    }

    pub fn mode(&self, direction: FocusDirection) -> NavigationMode {
        self.modes[direction.index()]
    }

    pub fn routes(&self, direction: FocusDirection) -> &[CustomRoute] {
        &self.custom[direction.index()]
    }

    /// Whether the control at `key` sits inside this container.
    pub fn holds(&self, key: &str) -> bool {
        key.strip_prefix(self.key.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    }
}
