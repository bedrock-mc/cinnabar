//! `@button`: the button wrapper, and its states from named children or a
//! component set's `State` variants, overlaid in one panel.

use std::collections::BTreeMap;

use serde_json::json;

use super::names::{identifier, parse, with_words};
use super::{Body, Ctx, Natural, Place, Scope, entry, panel, type_of};
use crate::figma::{Constraint, Constraints, Node, Positioning};

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

/// States a button without them shows its default's look in.
const FALLBACKS: [&str; 2] = ["hover", "pressed"];

/// `node` with its state-named children moved into one `states` frame in the
/// default child's place, each overlaying it at the default's size, plus a copy
/// of the default for a missing hover or pressed state; and those states in
/// [`BUTTON_STATES`] order. Only the frame takes part in the button's layout.
/// `None` without a `default` child.
pub(super) fn state_frame(node: &Node) -> Option<(Node, Vec<&'static str>)> {
    let mut found: Vec<(&'static str, usize)> = Vec::new();
    for (index, child) in node.children.iter().enumerate() {
        let name = identifier(&parse(&child.name).words, "");
        let state = BUTTON_STATES
            .iter()
            .map(|(state, _)| *state)
            .find(|state| *state == name);
        if let Some(state) = state
            && !found.iter().any(|(known, _)| *known == state)
        {
            found.push((state, index));
        }
    }
    let at = |state: &str| {
        found
            .iter()
            .find(|(known, _)| *known == state)
            .map(|(_, at)| *at)
    };
    let default_at = at("default")?;
    let default = &node.children[default_at];
    let overlay = |mut child: Node, name: Option<&str>| {
        (child.x, child.y) = (0.0, 0.0);
        (child.width, child.height) = (default.width, default.height);
        child.constraints = Constraints {
            horizontal: Constraint::Stretch,
            vertical: Constraint::Stretch,
        };
        child.positioning = Positioning::Auto;
        if let Some(state) = name {
            child.name = with_words(&child.name, state);
        }
        child
    };
    let mut children = Vec::new();
    let mut states = Vec::new();
    for (state, _) in BUTTON_STATES {
        match at(state) {
            Some(index) => children.push(overlay(node.children[index].clone(), None)),
            None if FALLBACKS.contains(&state) => {
                children.push(overlay(default.clone(), Some(state)))
            }
            None => continue,
        }
        states.push(state);
    }
    let frame = Node {
        id: default.id.clone(),
        name: "states".into(),
        kind: "FRAME".into(),
        x: default.x,
        y: default.y,
        width: default.width,
        height: default.height,
        constraints: default.constraints,
        positioning: default.positioning,
        sizing: default.sizing,
        children,
        ..Node::default()
    };
    let mut framed = node.clone();
    framed.children = Vec::new();
    for (index, child) in node.children.iter().enumerate() {
        if index == default_at {
            framed.children.push(frame.clone());
        } else if !found.iter().any(|(_, at)| *at == index) {
            framed.children.push(child.clone());
        }
    }
    Some((framed, states))
}

/// Wraps `content` as a button pressed as `button.<id>`. With a `default`
/// among `states`, each names a descendant control the button switches;
/// otherwise the content sits outside any state control and always shows.
/// Content sized to itself on a `natural` axis keeps that size inside the button.
pub(super) fn button(content: Body, id: &str, states: &[&'static str], natural: &Natural) -> Body {
    let states = if states.contains(&"default") {
        states
    } else {
        &[]
    };
    let mut body = if !states.is_empty() && type_of(&content) == "panel" {
        content
    } else {
        let mut inner = content;
        let size = natural
            .clone()
            .map(|natural| natural.unwrap_or_else(|| json!("100%")));
        inner.insert("size".into(), json!(size));
        panel(vec![entry("content", inner)])
    };
    body.insert("type".into(), "button".into());
    for (state, property) in BUTTON_STATES {
        let target = if states.contains(&state) { state } else { "" };
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
    ) -> Option<(Body, Vec<&'static str>, [f64; 2])> {
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
        let mut bodies: BTreeMap<&str, Body> = BTreeMap::new();
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
            if bodies.contains_key(button_state) {
                let message = format!("another variant is already the {button_state} state");
                self.warn(variant, &path, message);
                continue;
            }
            if let Some((_, body)) = self.control(variant, Place::Fill, scope) {
                bodies.insert(button_state, body);
            }
        }
        // Every state overlays the others in one panel; a missing hover or
        // pressed state copies the default.
        let mut states = Vec::new();
        let mut controls = Vec::new();
        for (state, _) in BUTTON_STATES {
            let body = bodies.get(state).or_else(|| {
                FALLBACKS
                    .contains(&state)
                    .then(|| bodies.get("default"))
                    .flatten()
            });
            if let Some(body) = body {
                controls.push(entry(state, body.clone()));
                states.push(state);
            }
        }
        Some((panel(controls), states, [default.width, default.height]))
    }
}
