//! The resolved control tree consumed by later stages. Inheritance is flattened,
//! `$vars` and globals are substituted, and `ignored` controls are dropped; size,
//! offset and `view` expressions are intentionally left symbolic for T2.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod properties;
pub use properties::Properties;

/// A fully qualified `namespace.name` handle for a control definition.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ControlRef {
    pub namespace: String,
    pub name: String,
}

impl ControlRef {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Split a reference against a default namespace, dropping any leading `@`.
    /// `common.foo` keeps its namespace; `foo` and `.foo` adopt `default_ns`.
    /// A `name@ns.base` factory entry names its base after the `@`.
    pub fn parse(reference: &str, default_ns: &str) -> Self {
        let reference = reference
            .rsplit_once('@')
            .map_or(reference, |(_, base)| base);
        match reference.split_once('.') {
            Some(("", name)) => Self::new(default_ns, name),
            Some((namespace, name)) => Self::new(namespace, name),
            None => Self::new(default_ns, reference),
        }
    }
}

impl fmt::Display for ControlRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace, self.name)
    }
}

/// A recorded factory: the per-collection-index instantiation map that T3 uses.
/// Populated from a `factory` property or from a `type: "factory"` control's
/// `control_ids`; nothing is instantiated here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Factory {
    /// The factory `name` (the collection role), when present.
    pub name: Option<String>,
    /// Role key -> the control instantiated for that role.
    pub control_ids: BTreeMap<String, ControlRef>,
    /// A single-target factory (`control_name`), used by some radio groups.
    pub control_name: Option<ControlRef>,
    /// `max_children_size`: the host keeps at most this many children, evicting
    /// from the end opposite `insert_location`; zero declares no limit.
    #[serde(default)]
    pub max_children_size: Option<usize>,
    /// Instance names `control_ids` entries give (`instance@ns.control`).
    #[serde(default)]
    pub instance_names: BTreeMap<String, String>,
    /// `insert_location: "front"`: new controls go first.
    #[serde(default)]
    pub insert_front: bool,
    /// `factory_variables` captured where the factory was declared.
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
}

impl Factory {
    pub fn is_empty(&self) -> bool {
        self.control_ids.is_empty() && self.control_name.is_none()
    }

    /// The `$vars` a `control_ids` creation resolves with: its own, overridden
    /// by the captured `factory_variables`.
    pub fn creation_vars(&self, own: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
        let mut vars = own.clone();
        vars.extend(
            self.variables
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
        vars
    }

    /// `created` placed around the host's `literals` by `insert_location`, then
    /// capped at `max_children_size` by evicting from the opposite end.
    pub(crate) fn place<T>(&self, literals: Vec<T>, mut created: Vec<T>) -> Vec<T> {
        let mut children = if self.insert_front {
            created.reverse();
            created.extend(literals);
            created
        } else {
            let mut children = literals;
            children.extend(created);
            children
        };
        if let Some(max) = self.max_children_size
            && children.len() > max
        {
            if self.insert_front {
                children.truncate(max);
            } else {
                children.drain(..children.len() - max);
            }
        }
        children
    }
}

/// A resolved control node in document order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedControl {
    /// Local instance name (the part before `@` at the definition site).
    pub name: String,
    /// The `type` after substitution; `None` when no ancestor supplied one.
    pub control_type: Option<String>,
    /// The literal `@base` this node was derived from, for provenance.
    pub base: Option<ControlRef>,
    /// Set when a base reference could not be resolved (e.g. an unbound `$var`
    /// base); the node is still emitted so callers see the gap.
    pub unresolved_base: Option<String>,
    /// Remaining properties, `$var`/global substituted, size/`view` left symbolic.
    pub properties: Properties,
    /// Ordered children with `ignored` controls removed.
    pub children: Vec<ResolvedControl>,
    /// Recorded factory, if this control declares one.
    pub factory: Option<Factory>,
}

impl ResolvedControl {
    /// First direct child with the given instance name.
    pub fn child(&self, name: &str) -> Option<&ResolvedControl> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Depth-first search for the first descendant (or self) matching `predicate`.
    pub fn find(&self, predicate: &impl Fn(&ResolvedControl) -> bool) -> Option<&ResolvedControl> {
        if predicate(self) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(predicate))
    }
}
