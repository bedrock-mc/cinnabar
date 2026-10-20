//! `@button`: the button wrapper, and its states from named children or a
//! component set's `State` variants.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{Body, Ctx, Node, Place, Scope, entry, panel, type_of};

/// The variant property a component set's button states vary.
const STATE_PROPERTY: &str = "state";
/// Button state children by name, and the property naming each.
const BUTTON_STATES: [(&str, &str); 4] = [
    ("default", "default_control"),
    ("hover", "hover_control"),
    ("pressed", "pressed_control"),
    ("locked", "locked_control"),
];

/// The button state a `State` variant value names.
fn button_state(value: &str) -> Option<&'static str> {
    let value = value.to_ascii_lowercase();
    if value == "disabled" {
        return Some("locked");
    }
    BUTTON_STATES
        .iter()
        .map(|(state, _)| *state)
        .find(|state| *state == value)
}

/// Wraps `content` as a button pressed as `button.<id>`. Children named for
/// button states become them; otherwise the content shows in every state.
pub(super) fn button(content: Body, id: &str) -> Body {
    let children: Vec<String> = match (type_of(&content), content.get("controls")) {
        ("panel", Some(Value::Array(controls))) => controls
            .iter()
            .filter_map(|control| control.as_object()?.keys().next().cloned())
            .collect(),
        _ => Vec::new(),
    };
    let has = |state: &str| children.iter().any(|name| name == state);
    let (mut body, states): (Body, Vec<&str>) = if has("default") {
        let states = BUTTON_STATES
            .iter()
            .map(|(state, _)| *state)
            .filter(|state| has(state))
            .collect();
        (content, states)
    } else {
        let mut inner = content;
        inner.insert("size".into(), json!(["100%", "100%"]));
        (panel(vec![entry("default", inner)]), vec!["default"])
    };
    body.insert("type".into(), "button".into());
    for (state, property) in BUTTON_STATES {
        if states.contains(&state) {
            body.insert(property.into(), state.into());
        } else if state != "locked" {
            body.insert(property.into(), "default".into());
        }
    }
    let target = format!("button.{id}");
    body.insert(
        "button_mappings".into(),
        json!([
            { "from_button_id": "button.menu_select", "to_button_id": target, "mapping_type": "pressed" },
            { "from_button_id": "button.menu_ok", "to_button_id": target, "mapping_type": "focused" },
        ]),
    );
    body.insert("sound_name".into(), "random.click".into());
    body
}

impl Ctx<'_> {
    /// A panel of a `@button` component set's or instance's variants, each
    /// named for the button state its `State` property selects, and the
    /// default variant's size. Only variants whose other properties match the
    /// instance's (or the set's first default variant's) take part. `None`
    /// when `node` has no default `State` variant.
    pub(super) fn state_variants(
        &mut self,
        node: &Node,
        scope: &Scope,
    ) -> Option<(Body, [f64; 2])> {
        let candidates = match node.kind.as_str() {
            "COMPONENT_SET" => &node.children,
            _ => &node.variants,
        };
        let state = |variant: &Node| {
            let value = variant
                .variant_properties
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(STATE_PROPERTY))?
                .1;
            Some(value.clone())
        };
        let others = |properties: &BTreeMap<String, String>| -> Vec<(String, String)> {
            properties
                .iter()
                .filter(|(key, _)| !key.eq_ignore_ascii_case(STATE_PROPERTY))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        };
        let wanted = (!node.variants.is_empty()).then(|| others(&node.variant_properties));
        let default = candidates.iter().find(|variant| {
            state(variant).is_some_and(|value| value.eq_ignore_ascii_case("default"))
                && wanted
                    .as_ref()
                    .is_none_or(|wanted| *wanted == others(&variant.variant_properties))
        })?;
        let reference = others(&default.variant_properties);
        let mut taken = Vec::new();
        let mut controls = Vec::new();
        for variant in candidates {
            let Some(value) = state(variant) else {
                continue;
            };
            if others(&variant.variant_properties) != reference {
                continue;
            }
            let path = scope.path_to(variant);
            let Some(button_state) = button_state(&value) else {
                let message = format!("variant state `{value}` is not a button state");
                self.warn(variant, &path, message);
                continue;
            };
            if taken.contains(&button_state) {
                let message = format!("another variant is already the {button_state} state");
                self.warn(variant, &path, message);
                continue;
            }
            if let Some((_, body)) = self.control(variant, Place::Fill, scope) {
                taken.push(button_state);
                controls.push(entry(button_state, body));
            }
        }
        Some((panel(controls), [default.width, default.height]))
    }
}
