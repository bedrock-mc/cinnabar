//! A control's input component as the 1.26.50 factory builds it: its button mappings, the
//! source-less hover mappings, the `any` remap, and the pointer/modal flags.

use serde_json::Value;

use crate::tree::ResolvedControl;
use crate::widgets::bound_bool;

/// When a mapping fires (`ButtonMappingType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingType {
    /// While the control is in the tree, wherever the pointer is.
    Global,
    DoublePressed,
    /// While the pointer is over the control.
    Pressed,
    /// While the control has focus.
    Focused,
}

/// Who receives the mapped event (`ScreenEventScope`, plus the `global` scope string).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MappingScope {
    #[default]
    Controller,
    View,
    Global,
}

/// `input_mode_condition`: the input modes a mapping is limited to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputModeCondition {
    #[default]
    Any,
    NotGamepad,
    Gamepad,
}

/// How the pointer or gamepad is being used, for [`InputModeCondition`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Mouse,
    Touch,
    Gamepad,
}

impl InputModeCondition {
    pub fn admits(self, mode: InputMode) -> bool {
        match self {
            InputModeCondition::Any => true,
            InputModeCondition::NotGamepad => mode != InputMode::Gamepad,
            InputModeCondition::Gamepad => mode == InputMode::Gamepad,
        }
    }
}

/// One `button_mappings` entry with the factory's defaults applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub from: String,
    pub to: String,
    pub kind: MappingType,
    pub scope: MappingScope,
    pub button_up_right_of_first_refusal: bool,
    pub handle_select: bool,
    pub handle_deselect: bool,
    pub alternate_input_scope: bool,
    pub consume_event: bool,
    pub input_mode_condition: InputModeCondition,
    pub ignore_input_scope: bool,
}

/// The input component of one control.
#[derive(Clone, Debug, PartialEq)]
pub struct InputComponent {
    pub mappings: Vec<Mapping>,
    /// Source-less `pressed` mappings: their target fires on hover.
    pub hover_mappings: Vec<(String, MappingScope)>,
    /// `from_button_id: "any"`: every button reaches this control, in this scope.
    pub any: Option<MappingScope>,
    pub modal: bool,
    pub inline_modal: bool,
    pub gamepad_deflection_mode: bool,
    pub always_listen_to_input: bool,
    pub always_handle_pointer: bool,
    pub always_handle_controller_direction: bool,
    pub hover_enabled: bool,
    pub consume_hover_events: bool,
    pub prevent_touch_input: bool,
}

impl Default for InputComponent {
    fn default() -> Self {
        Self {
            mappings: Vec::new(),
            hover_mappings: Vec::new(),
            any: None,
            modal: false,
            inline_modal: false,
            gamepad_deflection_mode: false,
            always_listen_to_input: false,
            always_handle_pointer: false,
            always_handle_controller_direction: false,
            hover_enabled: true,
            consume_hover_events: true,
            prevent_touch_input: false,
        }
    }
}

impl InputComponent {
    /// The component `control` carries; the factory reads the same keys for every type.
    pub(crate) fn read(control: &ResolvedControl) -> Self {
        super::cache::entry(&control.properties)
            .input
            .get_or_init(|| Self::parse(control))
            .clone()
    }

    /// Parse flags once for this version of the property map.
    fn parse(control: &ResolvedControl) -> Self {
        let flag = |key: &str, fallback: bool| bound_bool(control, key).unwrap_or(fallback);
        Self {
            modal: flag("modal", false),
            inline_modal: flag("inline_modal", false),
            gamepad_deflection_mode: flag("gamepad_deflection_mode", false),
            always_listen_to_input: flag("always_listen_to_input", false),
            always_handle_pointer: flag("always_handle_pointer", false),
            always_handle_controller_direction: flag("always_handle_controller_direction", false),
            hover_enabled: flag("hover_enabled", true),
            consume_hover_events: flag("consume_hover_events", true),
            prevent_touch_input: flag("prevent_touch_input", false),
            ..Self::read_mappings(control)
        }
    }

    /// Read routing declarations without the unrelated pointer and modal flags.
    fn read_mappings(control: &ResolvedControl) -> Self {
        super::cache::entry(&control.properties)
            .mappings
            .get_or_init(|| Self::parse_mappings(control))
            .clone()
    }

    /// Look up one global route without copying every mapping into a temporary component.
    pub(crate) fn global_target(control: &ResolvedControl, from: &str) -> Option<String> {
        let cached = super::cache::entry(&control.properties);
        cached
            .mappings
            .get_or_init(|| Self::parse_mappings(control))
            .mappings
            .iter()
            .find(|mapping| mapping.from == from && mapping.kind == MappingType::Global)
            .map(|mapping| mapping.to.clone())
    }

    /// Apply the factory's mapping defaults without reading pointer flags.
    fn parse_mappings(control: &ResolvedControl) -> Self {
        let mut component = Self::default();
        let items: &[Value] = match control.properties.get("button_mappings") {
            Some(Value::Array(items)) => items,
            _ => &[],
        };
        for item in items.iter().filter_map(Value::as_object) {
            component.add(item);
        }
        component
    }

    fn add(&mut self, item: &serde_json::Map<String, Value>) {
        if ignored(item.get("ignored")) {
            return;
        }
        let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("");
        let flag = |key: &str, fallback: bool| match item.get(key) {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::String(text)) if text == "true" => true,
            Some(Value::String(text)) if text == "false" => false,
            _ => fallback,
        };
        // An unknown or missing type is logged by vanilla and read as `pressed`.
        let kind = match text("mapping_type") {
            "global" => MappingType::Global,
            "double_pressed" => MappingType::DoublePressed,
            "focused" => MappingType::Focused,
            _ => MappingType::Pressed,
        };
        let scope = match text("scope") {
            "view" => MappingScope::View,
            "global" => MappingScope::Global,
            _ => MappingScope::Controller,
        };
        let (from, to) = (text("from_button_id"), text("to_button_id"));
        if from == "any" {
            self.any = Some(scope);
            return;
        }
        if from.is_empty() {
            if kind == MappingType::Pressed && !to.is_empty() {
                self.hover_mappings.push((to.to_owned(), scope));
            }
            return;
        }
        if to.is_empty() {
            return;
        }
        self.mappings.push(Mapping {
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            scope,
            button_up_right_of_first_refusal: flag("button_up_right_of_first_refusal", false),
            handle_select: flag("handle_select", true),
            handle_deselect: flag("handle_deselect", true),
            alternate_input_scope: flag("alternate_input_scope", false),
            consume_event: flag("consume_event", true),
            input_mode_condition: match text("input_mode_condition") {
                "gamepad" => InputModeCondition::Gamepad,
                "not_gamepad" => InputModeCondition::NotGamepad,
                _ => InputModeCondition::Any,
            },
            ignore_input_scope: flag("ignore_input_scope", false),
        });
    }

    /// Where a `pressed` mapping from `from` routes, the first declared winning.
    pub fn pressed_target(&self, from: &str) -> Option<&str> {
        self.mappings
            .iter()
            .find(|mapping| mapping.from == from && mapping.kind == MappingType::Pressed)
            .map(|mapping| mapping.to.as_str())
    }

    /// Whether anything here reacts to input at all.
    pub fn listens(&self) -> bool {
        !self.mappings.is_empty()
            || !self.hover_mappings.is_empty()
            || self.any.is_some()
            || self.modal
            || self.inline_modal
            || self.always_listen_to_input
            || self.always_handle_pointer
    }
}

/// A nested `ignored` keeps its substituted text (`(not false)`); fold it here.
fn ignored(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(expression)) => {
            crate::predicate::eval(expression, &crate::env::Env::new()) == Some(true)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn component(mappings: Value) -> InputComponent {
        let control = ResolvedControl {
            name: "c".to_owned(),
            control_type: Some("button".to_owned()),
            base: None,
            unresolved_base: None,
            properties: [("button_mappings".to_owned(), mappings)]
                .into_iter()
                .collect(),
            children: Vec::new(),
            factory: None,
        };
        InputComponent::read(&control)
    }

    // A missing or unknown mapping_type falls back to pressed, as the factory logs.
    #[test]
    fn unknown_mapping_type_reads_as_pressed() {
        let read = component(json!([
            { "from_button_id": "button.a", "to_button_id": "button.b" },
            { "from_button_id": "button.a", "to_button_id": "button.c", "mapping_type": "unknown" }
        ]));
        assert!(
            read.mappings
                .iter()
                .all(|mapping| mapping.kind == MappingType::Pressed)
        );
    }

    // Source-less pressed mappings become hover mappings; "any" sets the remap scope.
    #[test]
    fn hover_and_any_mappings_register_separately() {
        let read = component(json!([
            { "to_button_id": "button.hovered", "mapping_type": "pressed" },
            { "to_button_id": "button.never", "mapping_type": "global" },
            { "from_button_id": "any", "mapping_type": "global", "scope": "view" }
        ]));
        assert!(read.mappings.is_empty());
        assert_eq!(
            read.hover_mappings,
            [("button.hovered".to_owned(), MappingScope::Controller)]
        );
        assert_eq!(read.any, Some(MappingScope::View));
    }

    // Every per-mapping field keeps the factory's defaults unless set.
    #[test]
    fn mapping_fields_default_like_the_factory() {
        let read = component(json!([
            { "from_button_id": "button.menu_select", "to_button_id": "button.test", "mapping_type": "pressed" },
            {
                "from_button_id": "button.menu_select", "to_button_id": "button.test", "mapping_type": "focused",
                "scope": "view", "input_mode_condition": "gamepad", "button_up_right_of_first_refusal": true,
                "handle_select": false, "handle_deselect": false, "alternate_input_scope": true,
                "consume_event": false, "ignore_input_scope": true
            }
        ]));
        let plain = &read.mappings[0];
        assert!(plain.handle_select && plain.handle_deselect && plain.consume_event);
        assert_eq!(plain.scope, MappingScope::Controller);
        let full = &read.mappings[1];
        assert_eq!(full.kind, MappingType::Focused);
        assert_eq!(full.scope, MappingScope::View);
        assert_eq!(full.input_mode_condition, InputModeCondition::Gamepad);
        assert!(full.button_up_right_of_first_refusal && full.alternate_input_scope);
        assert!(!full.handle_select && !full.handle_deselect && !full.consume_event);
        assert!(full.ignore_input_scope);
    }
}
