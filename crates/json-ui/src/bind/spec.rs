//! A control's `bindings` array parsed as vanilla reads it: the binding type and condition, the
//! name expression, its target, collection fields and view scope.

use serde_json::{Map, Value};

use crate::env::Env;
use crate::predicate::{self, NoBindings, Scalar};
use crate::tree::ResolvedControl;

/// When a binding applies (`BindingCondition`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Condition {
    /// Every data refresh.
    None,
    /// The first application, retained after.
    Once,
    /// Every update.
    Always,
    /// Every update while the control is visible.
    AlwaysWhenVisible,
    /// Every data refresh while the control is visible.
    Visible,
    /// When the control's visibility differs from the last application's.
    VisibilityChanged,
}

/// Which control a view reads (`NameResolutionScope`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ViewScope {
    /// First match breadth-first from the root.
    Global,
    /// The control itself.
    Own,
    /// First match breadth-first from the parent.
    Sibling,
    /// Nearest ancestor so named.
    Ancestor,
}

/// A binding's name: one property, or an expression reading properties.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Source {
    Simple(String),
    Expression {
        text: String,
        properties: Vec<String>,
    },
}

impl Source {
    /// The `#name`s a binding queries, in order.
    pub(super) fn properties(&self) -> impl Iterator<Item = &str> {
        let names = match self {
            Source::Simple(name) => std::slice::from_ref(name),
            Source::Expression { properties, .. } => properties.as_slice(),
        };
        names.iter().map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    /// `rename` is the override: empty writes each property under its own name.
    Global {
        source: Source,
        rename: String,
    },
    Collection {
        source: Source,
        rename: String,
        collection: String,
    },
    Details {
        collection: String,
        prefix: String,
    },
    View {
        source: Source,
        target: String,
        control: String,
        scope: ViewScope,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Binding {
    pub(super) kind: Kind,
    pub(super) condition: Condition,
}

impl Binding {
    /// The collection name a `once` binding waits on an item index for.
    pub(super) fn collection(&self) -> Option<&str> {
        match &self.kind {
            Kind::Collection { collection, .. } | Kind::Details { collection, .. } => {
                Some(collection)
                    .filter(|name| !name.is_empty())
                    .map(String::as_str)
            }
            _ => None,
        }
    }
}

/// The bindings `control` declares, with diagnostics for the ones the client
/// rejects or corrects.
pub(super) fn parse(control: &ResolvedControl, diagnostics: &mut Vec<String>) -> Vec<Binding> {
    let mut bindings: Vec<Binding> = match control.properties.get("bindings") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_object)
            .filter(|binding| !ignored(binding.get("ignored")))
            .filter_map(|binding| parse_one(&control.name, binding, diagnostics))
            .collect(),
        None => Vec::new(),
        Some(_) => {
            diagnostics.push(format!("{}: bindings must be an array", control.name));
            Vec::new()
        }
    };
    // A grid's `grid_dimension_binding` asks the controller for its size
    // every refresh.
    if control.control_type.as_deref() == Some("grid")
        && let Some(name) = control
            .properties
            .get("grid_dimension_binding")
            .and_then(Value::as_str)
            .filter(|name| name.starts_with('#'))
    {
        bindings.push(Binding {
            kind: Kind::Global {
                source: Source::Simple(name.to_owned()),
                rename: String::new(),
            },
            condition: Condition::Always,
        });
    }
    bindings
}

/// A binding entry's `ignored`, as the client's resolved def drops one.
fn ignored(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(expression)) => predicate::eval(expression, &Env::new()) == Some(true),
        _ => false,
    }
}

/// A string field, evaluated first, so a constant
/// parenthesised expression reads as its result.
fn text(binding: &Map<String, Value>, key: &str) -> String {
    let raw = binding.get(key).and_then(Value::as_str).unwrap_or("");
    if raw.starts_with('(')
        && predicate::property_tokens(raw).is_empty()
        && let Some(Scalar::Text(result)) = predicate::eval_scalar(raw, &Env::new(), &NoBindings)
    {
        return result;
    }
    raw.to_owned()
}

fn parse_one(
    owner: &str,
    binding: &Map<String, Value>,
    diagnostics: &mut Vec<String>,
) -> Option<Binding> {
    let condition = condition(binding.get("binding_condition"), owner, diagnostics);
    let override_name = text(binding, "binding_name_override");
    let collection = text(binding, "binding_collection_name");
    let rename = override_name.clone();
    let kind = match binding_type(binding.get("binding_type"), owner, diagnostics)? {
        BindingType::Global => {
            let source = property_evaluation(binding.get("binding_name"))?;
            Kind::Global { source, rename }
        }
        BindingType::Collection => {
            let source = property_evaluation(binding.get("binding_name"))?;
            Kind::Collection {
                source,
                rename,
                collection,
            }
        }
        BindingType::Details => Kind::Details {
            collection,
            prefix: text(binding, "binding_collection_prefix"),
        },
        BindingType::View => {
            let source = property_evaluation(binding.get("source_property_name"))?;
            let flag = |key| binding.get(key).and_then(Value::as_bool) == Some(true);
            let (sibling, ancestor) = (
                flag("resolve_sibling_scope"),
                flag("resolve_ancestor_scope"),
            );
            if sibling && ancestor {
                diagnostics.push(format!(
                    "{owner}: resolve_ancestor_scope and resolve_sibling_scope cannot both be set"
                ));
            }
            let control = text(binding, "source_control_name");
            let scope = match (control.is_empty(), sibling, ancestor) {
                (true, ..) => ViewScope::Own,
                (false, true, _) => ViewScope::Sibling,
                (false, false, true) => ViewScope::Ancestor,
                (false, false, false) => ViewScope::Global,
            };
            let target = text(binding, "target_property_name");
            if target.is_empty() {
                diagnostics.push(format!("{owner}: view binding has no target property"));
            }
            Kind::View {
                source,
                target,
                control,
                scope,
            }
        }
    };
    Some(Binding { kind, condition })
}

enum BindingType {
    Global,
    Collection,
    Details,
    View,
}

/// Binding type: absent is global, `none` binds nothing,
/// an unknown name logs and falls back to global.
fn binding_type(
    value: Option<&Value>,
    owner: &str,
    diagnostics: &mut Vec<String>,
) -> Option<BindingType> {
    let Some(value) = value else {
        return Some(BindingType::Global);
    };
    Some(match value.as_str() {
        Some("global") => BindingType::Global,
        Some("collection") => BindingType::Collection,
        Some("collection_details") => BindingType::Details,
        Some("view") => BindingType::View,
        Some("none") => return None,
        other => {
            diagnostics.push(format!("{owner}: unknown binding type {other:?}"));
            BindingType::Global
        }
    })
}

/// Binding condition: an unknown name logs and reads `none`.
fn condition(value: Option<&Value>, owner: &str, diagnostics: &mut Vec<String>) -> Condition {
    let Some(value) = value else {
        return Condition::None;
    };
    match value.as_str() {
        Some("none") => Condition::None,
        Some("once") => Condition::Once,
        Some("always") => Condition::Always,
        Some("always_when_visible") => Condition::AlwaysWhenVisible,
        Some("visible") => Condition::Visible,
        Some("visibility_changed") => Condition::VisibilityChanged,
        other => {
            diagnostics.push(format!("{owner}: unknown binding condition {other:?}"));
            Condition::None
        }
    }
}

/// Levels of `$var`/constant indirection a name may take.
const MAX_INDIRECTION: usize = 8;

/// Property evaluation: a `#name` is one property; a
/// parenthesised expression reading properties is kept, a constant one is
/// evaluated and read again; anything else binds nothing.
fn property_evaluation(value: Option<&Value>) -> Option<Source> {
    let mut text = value?.as_str()?.to_owned();
    for _ in 0..MAX_INDIRECTION {
        if text.starts_with('#') {
            return Some(Source::Simple(text));
        }
        if !text.starts_with('(') {
            return None;
        }
        let properties = predicate::property_tokens(&text);
        if !properties.is_empty() {
            return Some(Source::Expression { text, properties });
        }
        match predicate::eval_scalar(&text, &Env::new(), &NoBindings)? {
            Scalar::Text(result) => text = result,
            _ => return None,
        }
    }
    None
}

/// Whether layout feedback can change this control's bound values.
pub(super) fn observes_scroll(control: &ResolvedControl, bindings: &[Binding]) -> bool {
    let is_scroll = |name: &str| super::state::SCROLL_PROPERTIES.contains(&name);
    control.properties.values().any(value_observes_scroll)
        || bindings.iter().any(|binding| match &binding.kind {
            Kind::Global { source, .. }
            | Kind::Collection { source, .. }
            | Kind::View { source, .. } => source.properties().any(is_scroll),
            Kind::Details { .. } => false,
        })
}

/// Keep feedback enabled for references anywhere in authored JSON expressions.
fn value_observes_scroll(value: &Value) -> bool {
    match value {
        Value::String(value) => super::state::SCROLL_PROPERTIES
            .iter()
            .any(|name| value.contains(name)),
        Value::Array(values) => values.iter().any(value_observes_scroll),
        Value::Object(values) => values.values().any(value_observes_scroll),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_feedback_expressions_are_observed() {
        for property in super::super::state::SCROLL_PROPERTIES {
            let value =
                serde_json::json!({"property_bag": {"#nested": [format!("(not {property})")]}});
            assert!(value_observes_scroll(&value));
        }
        assert!(!value_observes_scroll(
            &serde_json::json!({"text":"unrelated"})
        ));
    }
}
