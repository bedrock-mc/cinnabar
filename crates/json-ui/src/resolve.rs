//! Stage C: turn raw controls into the resolved tree, in the vanilla
//! `UIResolvedDef` order: `ignored` evaluates in the enclosing scope, then the
//! control's `$` declarations and selected `variables` blocks form its frame, and
//! its properties, factory and children resolve in that scope. Base references on
//! child keys are resolved here because they may be `$var`s only the scope knows.

use serde_json::{Map, Value};

use crate::anim;
use crate::catalog::{Catalog, RawControl, child_controls};
use crate::env::{Env, evaluate, substitute};
use crate::merge::{Layering, flatten_def, flatten_properties, inherit};
use crate::tree::{ControlRef, Factory, ResolvedControl};

const MAX_DEPTH: usize = 256;
/// Property recording the `$vars` a factory's or grid's created controls see.
pub(crate) const FACTORY_SCOPE: &str = "factory_scope";
/// A digest of [`FACTORY_SCOPE`], so equal scopes are recognised without comparing.
pub(crate) const FACTORY_SCOPE_KEY: &str = "factory_scope_key";
pub(crate) const MAX_NODES: usize = 200_000;

/// Drives resolution over one [`Catalog`], accumulating diagnostics.
pub struct Resolver<'a> {
    catalog: &'a Catalog,
    diagnostics: Vec<String>,
    nodes: usize,
    /// The scope resolution started in; a factory records what it adds to it.
    root: Option<Env>,
}

impl<'a> Resolver<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self {
            catalog,
            diagnostics: Vec::new(),
            nodes: 0,
            root: None,
        }
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// Record a caller-level diagnostic.
    pub fn note(&mut self, message: String) {
        self.diagnostics.push(message);
    }

    pub fn into_diagnostics(self) -> Vec<String> {
        self.diagnostics
    }

    /// Resolve `namespace.name` under `root_env` (globals plus context flags).
    pub fn resolve(
        &mut self,
        namespace: &str,
        name: &str,
        root_env: &Env,
    ) -> Option<ResolvedControl> {
        if self.root.is_none() {
            self.root = Some(root_env.clone());
        }
        let (control, provenance) =
            flatten_def(self.catalog, namespace, name, &mut self.diagnostics)?;
        // An ignored definition creates nothing, whether a screen or a
        // factory's instance (a pack's title overlay gated on one title).
        if self.is_ignored(&control, root_env) {
            return None;
        }
        let env = self.build_env(root_env, &control.props);
        Some(self.resolve_with_env(&control, provenance, None, &env, 0))
    }

    /// The root's `type` and substituted properties without resolving its
    /// children; `None` when the reference is unknown or ignored.
    pub(crate) fn resolve_root_properties(
        &mut self,
        namespace: &str,
        name: &str,
        root_env: &Env,
    ) -> Option<(Option<String>, std::collections::BTreeMap<String, Value>)> {
        let control = flatten_properties(self.catalog, namespace, name, &mut self.diagnostics)?;
        // As in `resolve_root`, `ignored` reads the scope before the control's own `$` values.
        if self.is_ignored(&control, root_env) {
            return None;
        }
        let env = self.build_env(root_env, &control.props);
        let mut missing = Vec::new();
        let control_type = control
            .props
            .get("type")
            .map(|value| substitute(value, &env, &mut missing))
            .and_then(value_string);
        let properties = build_properties(&control, &env, false, &mut missing);
        Some((control_type, properties))
    }

    fn resolve_with_env(
        &mut self,
        control: &RawControl,
        provenance: Option<ControlRef>,
        unresolved_base: Option<String>,
        env: &Env,
        depth: usize,
    ) -> ResolvedControl {
        self.nodes += 1;
        let mut missing = Vec::new();
        let control_type = control
            .props
            .get("type")
            .map(|value| evaluate(value, env))
            .and_then(value_string);
        let (factory, control_ids_consumed) =
            self.extract_factory(control, control_type.as_deref(), env);
        let mut properties = build_properties(control, env, control_ids_consumed, &mut missing);
        self.resolve_anims(&mut properties, env, &control.owner_ns);
        // A grid's items and a `control_name` template resolve in this scope;
        // `control_ids` creations see only their own and captured variables.
        let template = factory
            .as_ref()
            .is_some_and(|factory| factory.control_name.is_some());
        if template || properties.contains_key("grid_item_template") {
            let scope = self.local_scope(env);
            let key = scope_key(&scope);
            properties.insert(FACTORY_SCOPE.to_owned(), scope);
            properties.insert(FACTORY_SCOPE_KEY.to_owned(), Value::String(key));
        }
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            self.diagnostics.push(format!(
                "{}.{}: unresolved $vars: {}",
                control.owner_ns,
                control.name,
                missing.join(", ")
            ));
        }
        let children = self.resolve_children(control, env, depth);
        ResolvedControl {
            name: instance_name(&control.name, env),
            control_type,
            base: provenance,
            unresolved_base,
            properties: properties.into(),
            children,
            factory,
        }
    }

    /// The variables `env` holds beyond the root scope, which the controls a
    /// factory or grid creates resolve with, as they would inside it.
    fn local_scope(&self, env: &Env) -> Value {
        let root = self.root.clone().unwrap_or_default();
        Value::Object(env.above(&root).into_iter().collect())
    }

    /// Link the control's animation references (named or inline, in the
    /// animated properties and `anims`) into its graph; a referencing property
    /// takes the animation's initial value, as the factory sets it.
    fn resolve_anims(
        &self,
        properties: &mut std::collections::BTreeMap<String, Value>,
        env: &Env,
        owner_ns: &str,
    ) {
        properties.remove(anim::GRAPH_KEY);
        let catalog = self.catalog;
        let mut load = |target: &ControlRef| {
            let (def, _) = flatten_def(catalog, &target.namespace, &target.name, &mut Vec::new())?;
            // A referenced animation's own `$` declarations scope its values.
            let scope = Resolver::new(catalog).build_env(env, &def.props);
            match substitute(&Value::Object(def.props), &scope, &mut Vec::new()) {
                Value::Object(props) => Some((props, def.owner_ns)),
                _ => None,
            }
        };
        let mut builder = anim::GraphBuilder::new(&mut load);
        for key in anim::ANIMATED_PROPERTIES {
            let Some(value) = properties.get(key).cloned() else {
                continue;
            };
            match builder.add_head(&value, owner_ns) {
                Some(Some(initial)) => {
                    properties.insert(key.to_owned(), initial);
                }
                Some(None) => {
                    properties.remove(key);
                }
                // An unknown reference leaves the property at its default.
                None if value.as_str().is_some_and(|text| text.starts_with('@')) => {
                    properties.remove(key);
                }
                None => {}
            }
        }
        let anims = match properties.remove("anims") {
            Some(Value::Array(items)) => items,
            Some(single @ (Value::String(_) | Value::Object(_))) => vec![single],
            _ => Vec::new(),
        };
        for item in &anims {
            builder.add_head(item, owner_ns);
        }
        if let Some(graph) = builder.finish()
            && let Ok(value) = serde_json::to_value(graph)
        {
            properties.insert(anim::GRAPH_KEY.to_owned(), value);
        }
    }

    fn resolve_children(
        &mut self,
        control: &RawControl,
        env: &Env,
        depth: usize,
    ) -> Vec<ResolvedControl> {
        if depth >= MAX_DEPTH || self.nodes >= MAX_NODES {
            if depth >= MAX_DEPTH {
                self.diagnostics.push(format!(
                    "{}.{}: max depth reached",
                    control.owner_ns, control.name
                ));
            }
            return Vec::new();
        }
        // A `$var` child list (`"controls": "$button_contents"`) is read here, its
        // entries left for each child's own scope; it replaces the static list
        // even when it resolves to nothing.
        let dynamic = match control.props.get("controls") {
            Some(reference @ Value::String(_)) => Some(match evaluate(reference, env) {
                value @ Value::Array(_) => {
                    child_controls(&control.owner_ns, &value, &mut self.diagnostics)
                }
                _ => Vec::new(),
            }),
            _ => None,
        };
        let children = dynamic.as_ref().unwrap_or(&control.children);
        let mut resolved = Vec::new();
        for child in children {
            if self.nodes >= MAX_NODES {
                self.diagnostics.push(format!(
                    "{}.{}: node budget exhausted",
                    control.owner_ns, control.name
                ));
                break;
            }
            let (mut working, provenance, unresolved) = self.resolve_child_base(child, env);
            if child.base.is_none() && !child.name.starts_with('$') {
                working.name = crate::catalog::unqualified(&child.name).to_owned();
            }
            // `ignored` reads the enclosing scope only: the vanilla client
            // evaluates it before the control's own `$` declarations apply.
            if self.is_ignored(&working, env) {
                continue;
            }
            let child_env = self.build_env(env, &working.props);
            resolved.push(self.resolve_with_env(
                &working,
                provenance,
                unresolved,
                &child_env,
                depth + 1,
            ));
        }
        resolved
    }

    /// `ignored`, evaluated in the enclosing scope: a bool or integer decides; a
    /// string (an unset `$var`, literal text) keeps the control, as does an
    /// expression that needs runtime bindings.
    fn is_ignored(&mut self, control: &RawControl, env: &Env) -> bool {
        let Some(raw) = control.props.get("ignored") else {
            return false;
        };
        match evaluate(raw, env) {
            Value::Bool(flag) => flag,
            Value::Number(number) => {
                number.as_i64().is_some_and(|value| value != 0)
                    || number.as_u64().is_some_and(|value| value != 0)
            }
            Value::String(expression) if expression.starts_with('(') => {
                self.diagnostics.push(format!(
                    "{}.{}: undecidable `ignored` `{}` ({} bytes); keeping",
                    control.owner_ns,
                    control.name,
                    clipped(&expression),
                    expression.len()
                ));
                false
            }
            _ => false,
        }
    }

    /// Resolve a child's `@base` (literal or `$var`) and merge it under the child.
    fn resolve_child_base(
        &mut self,
        child: &RawControl,
        env: &Env,
    ) -> (RawControl, Option<ControlRef>, Option<String>) {
        // A variable child key may supply both its instance name and inherited template.
        if child.base.is_none()
            && let Some(Value::String(text)) = child
                .name
                .strip_prefix('$')
                .and_then(|variable| env.get(variable))
            && let Some((name, reference)) = text.split_once('@')
        {
            let mut named = child.clone();
            named.name = if name.is_empty() {
                ControlRef::parse(reference, &child.owner_ns).name
            } else {
                name.to_owned()
            };
            named.base = Some(reference.to_owned());
            return self.resolve_child_base(&named, env);
        }
        let Some(base) = &child.base else {
            return (child.clone(), None, None);
        };
        let reference = match base.strip_prefix('$') {
            Some(variable) => match env.get(variable) {
                Some(Value::String(text)) => text.clone(),
                _ => {
                    let mut cleared = child.clone();
                    cleared.base = None;
                    return (cleared, None, Some(base.clone()));
                }
            },
            None => base.clone(),
        };
        let base_ref = ControlRef::parse(&reference, &child.owner_ns);
        match flatten_def(
            self.catalog,
            &base_ref.namespace,
            &base_ref.name,
            &mut self.diagnostics,
        ) {
            Some((base_control, _)) => {
                let mut working = inherit(&base_control, child, Layering::Inline);
                working.base = None;
                (working, Some(base_ref), None)
            }
            None => {
                self.diagnostics.push(format!(
                    "{}.{}: base {base_ref} not found",
                    child.owner_ns, child.name
                ));
                let mut cleared = child.clone();
                cleared.base = None;
                (cleared, Some(base_ref), Some(reference))
            }
        }
    }

    /// The control's frame over `parent`: its `$` declarations in name order,
    /// each evaluated as it is declared, then its selected `variables` blocks.
    fn build_env(&mut self, parent: &Env, props: &Map<String, Value>) -> Env {
        let mut env = parent.child();
        for (key, value) in props {
            if let Some(name) = key.strip_prefix('$') {
                let value = evaluate(value, &env);
                env.set(name, value);
            }
        }
        if let Some(blocks) = props.get("variables") {
            match evaluate(blocks, &env) {
                Value::Array(blocks) => {
                    for block in blocks.iter().filter_map(Value::as_object) {
                        self.apply_block(block, &mut env);
                    }
                }
                Value::Object(block) => self.apply_block(&block, &mut env),
                _ => {}
            }
        }
        env.settle()
    }

    /// One `variables` block: `requires` selects it by the vanilla typed rules
    /// (a nonzero number, a nonempty string, a nonempty array or object; missing
    /// or null never), and its `$` members then follow `$` references.
    fn apply_block(&mut self, block: &Map<String, Value>, env: &mut Env) {
        let raw = block.get("requires").unwrap_or(&Value::Null);
        let selected = match evaluate(raw, env) {
            Value::Null => false,
            Value::Bool(flag) => flag,
            Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
            // Lenient: a condition that needs runtime bindings selects nothing.
            Value::String(text) if text.starts_with('(') => false,
            Value::String(text) => !text.is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
        };
        if !selected {
            return;
        }
        for (key, value) in block {
            let Some(name) = key.strip_prefix('$') else {
                continue;
            };
            let mut value = value.clone();
            for _ in 0..MAX_BLOCK_HOPS {
                if !value.as_str().is_some_and(|text| text.starts_with('$')) {
                    break;
                }
                let next = evaluate(&value, env);
                if next == value {
                    break;
                }
                value = next;
            }
            env.set(name, value);
        }
    }

    /// The factory a control declares: a `type: "factory"` control naming a
    /// `control_name` or `control_ids` is its own factory, else an object-valued
    /// `factory` (possibly a `$var`) is. `true` when the control's own
    /// `control_ids` were consumed.
    fn extract_factory(
        &mut self,
        control: &RawControl,
        control_type: Option<&str>,
        env: &Env,
    ) -> (Option<Factory>, bool) {
        let owner = &control.owner_ns;
        if control_type == Some("factory") {
            let names = |key: &str| {
                control
                    .props
                    .get(key)
                    .is_some_and(|value| !evaluate(value, env).is_null())
            };
            if names("control_name") || names("control_ids") {
                let name = instance_name(&control.name, env);
                let factory = self.factory_from(&control.props, Some(name), owner, env);
                return (Some(factory), true);
            }
        }
        if let Some(spec) = control.props.get("factory")
            && let Value::Object(spec) = evaluate(spec, env)
        {
            return (Some(self.factory_from(&spec, None, owner, env)), false);
        }
        (None, false)
    }

    /// A factory's declaration, each field evaluated in the declaring scope.
    fn factory_from(
        &mut self,
        fields: &Map<String, Value>,
        name: Option<String>,
        owner: &str,
        env: &Env,
    ) -> Factory {
        let field = |key: &str| fields.get(key).map(|value| evaluate(value, env));
        let name = name.or_else(|| field("name").and_then(value_string));
        if name.as_deref().is_none_or(str::is_empty) {
            self.diagnostics
                .push(format!("{owner}: factory name should not be empty"));
        }
        let mut factory = Factory {
            name: name.filter(|name| !name.is_empty()),
            ..Factory::default()
        };
        // A `control_name` template takes priority; `control_ids` then go unread.
        match field("control_name").and_then(value_string) {
            Some(reference) if !reference.is_empty() => {
                factory.control_name = Some(ControlRef::parse(&reference, owner));
            }
            _ => {
                if let Some(Value::Object(entries)) = field("control_ids") {
                    for (role, reference) in entries {
                        let Some(reference) = value_string(evaluate(&reference, env)) else {
                            continue;
                        };
                        if let Some((instance, _)) = reference.split_once('@')
                            && !instance.is_empty()
                        {
                            factory
                                .instance_names
                                .insert(role.clone(), instance.to_owned());
                        }
                        factory
                            .control_ids
                            .insert(role, ControlRef::parse(&reference, owner));
                    }
                }
            }
        }
        if let Some(Value::Array(names)) = field("factory_variables") {
            for name in names.iter().filter_map(Value::as_str) {
                let key = name.strip_prefix('$').unwrap_or(name).to_owned();
                factory
                    .variables
                    .insert(key, evaluate(&Value::from(name), env));
            }
        }
        match field("max_children_size").as_ref().and_then(Value::as_i64) {
            Some(max) if max < 0 => self
                .diagnostics
                .push(format!("{owner}: negative factory max_children_size {max}")),
            // Zero means unlimited.
            Some(max) if max > 0 => factory.max_children_size = usize::try_from(max).ok(),
            _ => {}
        }
        factory.insert_front =
            field("insert_location").and_then(value_string).as_deref() == Some("front");
        factory
    }
}

/// Most times a `variables` block value may follow a `$` reference.
const MAX_BLOCK_HOPS: usize = 8;

/// A stable digest of a scope's serialized vars.
fn scope_key(scope: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(scope)
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn build_properties(
    control: &RawControl,
    env: &Env,
    control_ids_consumed: bool,
    missing: &mut Vec<String>,
) -> std::collections::BTreeMap<String, Value> {
    let mut properties = std::collections::BTreeMap::new();
    for (key, value) in &control.props {
        if key.starts_with('$') || is_reserved(key) {
            continue;
        }
        if control_ids_consumed && key == "control_ids" {
            continue;
        }
        properties.insert(key.clone(), substitute(value, env, missing));
    }
    properties
}

fn is_reserved(key: &str) -> bool {
    matches!(
        key,
        "type" | "ignored" | "variables" | "factory" | "controls"
    )
}

/// A diagnostic-sized prefix of server-supplied text.
fn clipped(text: &str) -> &str {
    let mut end = text.len().min(80);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn value_string(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

/// An instance name written as a `$var` (`"$tab_view_binding_name@common.toggle"`)
/// takes the variable's string value; view bindings find the control by it.
fn instance_name(name: &str, env: &Env) -> String {
    match name
        .strip_prefix('$')
        .and_then(|variable| env.get(variable))
    {
        Some(Value::String(value)) => value.clone(),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{Catalog, Context, resolve};

    #[test]
    fn variable_child_key_keeps_named_inheritance_and_inline_overrides() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/a.json",
            r##"{ "namespace": "a",
                "button": { "type": "button", "size": [36, 36],
                    "$caption|default": "Base",
                    "controls": [
                        { "border": { "type": "image", "texture": "blue" } },
                        { "label": { "type": "label", "text": "$caption" } } ] },
                "root": { "type": "panel", "$button_control": "chosen@a.button",
                    "controls": [
                        { "$button_control": { "size": [50, 60], "$caption": "Join" } } ] } }"##,
        );
        let resolved = resolve(&catalog, "a.root", &Context::empty());
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let root = resolved.control.unwrap();
        let button = root.child("chosen").expect("the authored instance name");
        assert_eq!(button.control_type.as_deref(), Some("button"));
        assert_eq!(button.properties["size"], serde_json::json!([50, 60]));
        assert_eq!(
            button.child("border").unwrap().properties["texture"],
            "blue"
        );
        assert_eq!(button.child("label").unwrap().properties["text"], "Join");
    }

    // A control's own `$` declarations do not decide its `ignored`.
    #[test]
    fn ignored_reads_the_enclosing_scope() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/a.json",
            r#"{ "namespace": "n", "root": { "type": "panel", "$touch_mode|default": false,
                "controls": [
                    { "touch": { "type": "panel", "ignored": "(not $touch_mode)", "$touch_mode": true } },
                    { "mouse": { "type": "panel", "ignored": "$touch_mode" } } ] } }"#,
        );
        let root = resolve(&catalog, "n.root", &Context::empty())
            .control
            .unwrap();
        let names: Vec<_> = root
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(names, ["mouse"]);
    }

    fn names(catalog: &Catalog, reference: &str) -> Vec<String> {
        let root = resolve(catalog, reference, &Context::empty())
            .control
            .unwrap();
        root.children
            .iter()
            .map(|child| child.name.clone())
            .collect()
    }

    // A derived `controls`, even an empty dynamic one, replaces the base's children.
    #[test]
    fn derived_controls_replace_the_base_children() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/a.json",
            r#"{ "namespace": "a",
                "base": { "type": "panel", "controls": [ { "old": { "type": "panel" } } ] },
                "derived@a.base": { "controls": [ { "new": { "type": "panel" } } ] },
                "dynamic@a.base": { "$children": [], "controls": "$children" },
                "inline": { "type": "panel", "controls": [ { "x@a.base": { "controls": [] } } ] } }"#,
        );
        assert_eq!(names(&catalog, "a.derived"), ["new"]);
        assert!(names(&catalog, "a.dynamic").is_empty());
        let inline = resolve(&catalog, "a.inline", &Context::empty())
            .control
            .unwrap();
        assert!(inline.children[0].children.is_empty());
    }
    #[test]
    fn wide_sibling_lists_obey_the_shared_node_budget() {
        use super::{Env, MAX_NODES, RawControl, Resolver};
        let catalog = Catalog::default();
        let mut resolver = Resolver::new(&catalog);
        let leaf = RawControl {
            owner_ns: "a".into(),
            name: "leaf".into(),
            base: None,
            props: serde_json::Map::new(),
            children: Vec::new(),
            has_controls: false,
        };
        let root = RawControl {
            name: "root".into(),
            children: vec![leaf.clone(); MAX_NODES + 1],
            ..leaf
        };
        let resolved = resolver.resolve_with_env(&root, None, None, &Env::new(), 0);
        assert_eq!(resolved.children.len(), MAX_NODES - 1);
        assert!(
            resolver
                .diagnostics()
                .iter()
                .any(|note| note.contains("node budget"))
        );
    }
}
