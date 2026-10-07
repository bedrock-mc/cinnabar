//! A control's property bags at creation, as vanilla builds them: its own `property_bag` and `property_bag_for_children`, each
//! member evaluated, and the parent's children bag merged into both without
//! overwriting.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::env::Env;
use crate::predicate::{self, NoBindings, Scalar};
use crate::tree::ResolvedControl;

pub(super) type Bag = BTreeMap<String, Scalar>;

/// Evaluated literal bags belonging to one immutable control template.
pub(super) struct Bags {
    own: Bag,
    children: Arc<Bag>,
}

impl Bags {
    /// Evaluate template literals once; inherited bags are applied at creation.
    pub(super) fn new(control: &ResolvedControl) -> Self {
        Self {
            own: members(control.properties.get("property_bag")),
            children: Arc::new(members(control.properties.get("property_bag_for_children"))),
        }
    }

    /// The control's creation bag, with inherited values filling missing members.
    pub(super) fn own(&self, inherited: &Bag) -> Bag {
        let mut own = self.own.clone();
        for (name, value) in inherited {
            own.entry(name.clone()).or_insert_with(|| value.clone());
        }
        own
    }

    /// Share unchanged children bags; merge only when both scopes supply values.
    pub(super) fn children(&self, inherited: &Arc<Bag>) -> Arc<Bag> {
        if self.children.is_empty() {
            return Arc::clone(inherited);
        }
        if inherited.is_empty() {
            return Arc::clone(&self.children);
        }
        let mut children = (*self.children).clone();
        for (name, value) in inherited.iter() {
            children
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        Arc::new(children)
    }
}

/// A bag literal's members, each evaluated as a definition field: a
/// parenthesised expression that reads no property becomes its value.
fn members(value: Option<&Value>) -> Bag {
    let Some(Value::Object(members)) = value else {
        return Bag::new();
    };
    members
        .iter()
        .map(|(name, value)| (name.clone(), member(value)))
        .collect()
}

fn member(value: &Value) -> Scalar {
    if let Value::String(text) = value
        && text.trim_start().starts_with('(')
        && predicate::property_tokens(text).is_empty()
        && let Some(result) = predicate::eval_scalar(text, &Env::new(), &NoBindings)
    {
        return result;
    }
    Scalar::from_json(value)
}
