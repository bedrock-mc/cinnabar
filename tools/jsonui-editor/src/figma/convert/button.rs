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

/// The button states `node`'s own Figma children are named for, before any
/// layout wrapping nests them.
pub(super) fn named_states(node: &Node) -> Vec<&'static str> {
    let names: Vec<String> = node
        .children
        .iter()
        .map(|child| super::names::identifier(&super::names::parse(&child.name).words, ""))
        .collect();
    BUTTON_STATES
        .iter()
        .map(|(state, _)| *state)
        .filter(|state| names.iter().any(|name| name == state))
        .collect()
}

/// The button states among a variant panel's direct children.
pub(super) fn direct_states(body: &Body) -> Vec<&'static str> {
    let Some(Value::Array(controls)) = body.get("controls") else {
        return Vec::new();
    };
    BUTTON_STATES
        .iter()
        .map(|(state, _)| *state)
        .filter(|state| controls.iter().any(|control| control.get(*state).is_some()))
        .collect()
}

/// Wraps `content` as a button pressed as `button.<id>`. With a `default`
/// among `states` (descendants of `content` named for them), each state gets
/// its own control, a missing hover or pressed one copying the default's;
/// otherwise the content sits outside the state-switched controls and shows
/// in every state.
pub(super) fn button(content: Body, id: &str, states: &[&'static str]) -> Body {
    let wrap = |mut inner: Body| {
        inner.insert("size".into(), json!(["100%", "100%"]));
        panel(vec![entry("content", inner)])
    };
    let mut body = if !states.contains(&"default") {
        wrap(content)
    } else if type_of(&content) == "panel" {
        content
    } else {
        wrap(content)
    };
    let mut present = states.to_vec();
    if present.contains(&"default") {
        let missing: Vec<&str> = ["hover", "pressed"]
            .into_iter()
            .filter(|state| !present.contains(state))
            .collect();
        if copy_state(&mut body, "default", &missing) {
            present.extend(missing);
        }
    }
    body.insert("type".into(), "button".into());
    for (state, property) in BUTTON_STATES {
        let target = if present.contains(&state) { state } else { "" };
        body.insert(property.into(), target.into());
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

/// Inserts copies of the first control named `from` (breadth first, as state
/// targets resolve) right after it under each of `names`; `false` if absent.
fn copy_state(body: &mut Body, from: &str, names: &[&str]) -> bool {
    let mut queue: std::collections::VecDeque<&mut Body> = std::collections::VecDeque::from([body]);
    while let Some(next) = queue.pop_front() {
        let Some(Value::Array(controls)) = next.get_mut("controls") else {
            continue;
        };
        let found = controls.iter().position(|control| {
            control
                .as_object()
                .is_some_and(|entry| entry.keys().next().is_some_and(|key| key == from))
        });
        if let Some(at) = found {
            let original = controls[at][from].clone();
            for (offset, name) in names.iter().enumerate() {
                controls.insert(at + 1 + offset, json!({ *name: original.clone() }));
            }
            return true;
        }
        for control in controls.iter_mut() {
            if let Some(child) = control
                .as_object_mut()
                .and_then(|entry| entry.values_mut().next())
                .and_then(Value::as_object_mut)
            {
                queue.push_back(child);
            }
        }
    }
    false
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
