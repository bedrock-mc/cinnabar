//! Chat layout is application-owned; other HUD controls retain pack priority.

use std::collections::{BTreeMap, BTreeSet};

use json_ui::{Catalog, ControlRef, RawControl, ResolvedControl};
use serde_json::Value;

const ROOTS: [&str; 4] = ["java_chat", "chat_panel", "chat_grid_item", "chat_label"];
/// Bounds factory templates followed from the HUD, as the runtime binder bounds its expansions.
const MAX_FACTORY_EXPANSIONS: usize = 64;

/// Restores built-in chat in `catalog`; returns the chat-screen controls that gained pack filters.
pub(super) fn prefer_builtin(catalog: &mut Catalog, builtin: &Catalog) -> Vec<RawControl> {
    let Some(screen) = builtin.lookup("hud", "hud_screen") else {
        return Vec::new();
    };
    let Some(overlay) = screen.props.get("$additional_screen_content").cloned() else {
        return Vec::new();
    };
    let Some(reference) = overlay.as_str() else {
        return Vec::new();
    };
    let overlay_ref = ControlRef::parse(reference, "hud");
    let empty = screen
        .props
        .get("$cinnabar_additional_content|default")
        .cloned()
        .unwrap_or(Value::Null);
    let mut chat = ROOTS
        .into_iter()
        .map(|name| ControlRef::new("hud", name))
        .collect::<BTreeSet<_>>();
    chat.insert(overlay_ref.clone());
    let mut derived = BTreeMap::<_, Vec<_>>::new();
    for control in catalog.controls() {
        if let Some(base) = &control.base {
            derived
                .entry(ControlRef::parse(base, &control.owner_ns))
                .or_default()
                .push(ControlRef::new(&control.owner_ns, &control.name));
        }
    }
    let mut pending = chat.iter().cloned().collect::<Vec<_>>();
    while let Some(base) = pending.pop() {
        for control in derived.get(&base).into_iter().flatten() {
            if chat.insert(control.clone()) {
                pending.push(control.clone());
            }
        }
    }
    let mut visibility = ROOTS
        .into_iter()
        .filter_map(|name| {
            let props = &catalog.lookup("hud", name)?.props;
            let rules = Rules::read(props.get("bindings"), props.get("property_bag"));
            Some((ControlRef::new("hud", name), rules))
        })
        .collect::<BTreeMap<_, _>>();
    let labels = derived_labels(&derived);
    collect_active_labels(
        catalog,
        &labels,
        visibility
            .entry(ControlRef::new("hud", "chat_label"))
            .or_default(),
    );
    for control in catalog.controls_mut() {
        suppress_chat(control, &chat);
    }
    let mut restore = BTreeSet::new();
    collect_definition(builtin, overlay_ref, &mut restore);
    for name in ROOTS {
        collect_definition(builtin, ControlRef::new("hud", name), &mut restore);
    }
    let mut global_names = BTreeSet::new();
    for reference in restore {
        if let Some(control) = builtin.lookup(&reference.namespace, &reference.name) {
            let mut control = control.clone();
            if let Some(rules) = visibility.get(&reference) {
                rules.merge_into(&mut control);
            }
            collect_globals(&control, &mut global_names);
            catalog.insert(control);
        }
    }
    let filtered = restore_chat_screen(catalog, builtin, &mut global_names);
    let globals = global_names
        .into_iter()
        .filter_map(|name| Some((format!("${name}"), builtin.global(&name)?.clone())))
        .collect::<serde_json::Map<_, _>>();
    catalog.overlay_globals_text(&Value::Object(globals).to_string());
    for screen in catalog
        .controls_mut()
        .filter(|control| control.owner_ns == "hud" && control.name == "hud_screen")
    {
        let additional = screen
            .props
            .get("$additional_screen_content")
            .filter(|value| **value != overlay)
            .cloned()
            .or_else(|| screen.props.get("$cinnabar_additional_content").cloned())
            .or_else(|| {
                screen
                    .props
                    .get("$cinnabar_additional_content|default")
                    .cloned()
            })
            .unwrap_or_else(|| empty.clone());
        screen
            .props
            .insert("$cinnabar_additional_content".to_owned(), additional);
        screen
            .props
            .insert("$additional_screen_content".to_owned(), overlay.clone());
    }
    filtered
}

/// Finds named templates inheriting the ordinary HUD chat label.
fn derived_labels(derived: &BTreeMap<ControlRef, Vec<ControlRef>>) -> BTreeSet<ControlRef> {
    let root = ControlRef::new("hud", "chat_label");
    let mut labels = BTreeSet::from([root.clone()]);
    let mut pending = vec![root];
    while let Some(base) = pending.pop() {
        for reference in derived.get(&base).into_iter().flatten() {
            if labels.insert(reference.clone()) {
                pending.push(reference.clone());
            }
        }
    }
    labels
}

/// Retains visibility rules from the ordinary chat labels the pack HUD instantiates.
fn collect_active_labels(catalog: &Catalog, labels: &BTreeSet<ControlRef>, rules: &mut Rules) {
    let context = crate::ui_runtime::presentation::forms::menu_screens::retail_context();
    let Some(hud) = json_ui::resolve(catalog, json_ui::HUD_SCREEN, &context).control else {
        return;
    };
    let mut templates = Vec::new();
    visit_active(&hud, false, labels, &context, rules, &mut templates);
    let mut visited = BTreeSet::new();
    let mut expansions = 0;
    while let Some((template, scope)) = templates.pop() {
        let key = (
            template.clone(),
            serde_json::to_string(scope.vars()).unwrap_or_default(),
        );
        if !visited.insert(key) {
            continue;
        }
        if expansions == MAX_FACTORY_EXPANSIONS {
            bevy::log::warn!(
                %template,
                limit = MAX_FACTORY_EXPANSIONS,
                "chat filter scan skipped factory templates beyond the expansion limit"
            );
            break;
        }
        expansions += 1;
        if let Some(instance) = json_ui::resolve(catalog, &template.to_string(), &scope).control {
            let is_label = labels.contains(&template);
            visit_active(&instance, is_label, labels, &scope, rules, &mut templates);
        }
    }
}

/// Collects label rules in one resolved tree and queues its factory templates with their variables.
fn visit_active(
    control: &ResolvedControl,
    is_label: bool,
    labels: &BTreeSet<ControlRef>,
    scope: &json_ui::Context,
    rules: &mut Rules,
    templates: &mut Vec<(ControlRef, json_ui::Context)>,
) {
    if is_label
        || control
            .base
            .as_ref()
            .is_some_and(|base| labels.contains(base))
    {
        rules.extend(Rules::read(
            control.properties.get("bindings"),
            control.properties.get("property_bag"),
        ));
    }
    if let Some(factory) = &control.factory {
        let captured = factory
            .variables
            .iter()
            .fold(scope.clone(), |scope, (name, value)| {
                scope.with_var(name, value.clone())
            });
        for template in factory
            .control_name
            .iter()
            .chain(factory.control_ids.values())
        {
            templates.push((template.clone(), captured.clone()));
        }
    }
    for child in &control.children {
        visit_active(child, false, labels, scope, rules, templates);
    }
}

/// Restores the built-in chat screen with pack visibility rules; returns the controls that gained any.
fn restore_chat_screen(
    catalog: &mut Catalog,
    builtin: &Catalog,
    globals: &mut BTreeSet<String>,
) -> Vec<RawControl> {
    let context = crate::ui_runtime::presentation::forms::menu_screens::retail_context();
    let controls = builtin
        .controls()
        .filter(|control| control.owner_ns == "chat")
        .map(|control| {
            let resolved =
                json_ui::resolve(catalog, &format!("chat.{}", control.name), &context).control;
            (control.clone(), resolved)
        })
        .collect::<Vec<_>>();
    let mut filtered = Vec::new();
    for (mut control, resolved) in controls {
        if resolved.is_some_and(|resolved| retain_visibility(&mut control, &resolved)) {
            filtered.push(control.clone());
        }
        collect_globals(&control, globals);
        catalog.insert(control);
    }
    filtered
}

/// Copies resolved visibility rules onto the same named control and its children.
fn retain_visibility(target: &mut RawControl, source: &ResolvedControl) -> bool {
    let mut changed = Rules::read(
        source.properties.get("bindings"),
        source.properties.get("property_bag"),
    )
    .merge_into(target);
    for child in &mut target.children {
        if let Some(source) = source
            .children
            .iter()
            .find(|source| source.name == child.name)
        {
            changed |= retain_visibility(child, source);
        }
    }
    changed
}

/// Visibility bindings with the bindings and property-bag values that feed the properties they read.
#[derive(Debug, Default)]
struct Rules {
    bindings: Vec<Value>,
    bag: serde_json::Map<String, Value>,
}

impl Rules {
    /// Selects the visibility rules of one control's bindings and property bag.
    fn read(bindings: Option<&Value>, bag: Option<&Value>) -> Self {
        let bindings = bindings
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let mut wanted = vec![false; bindings.len()];
        let mut read = BTreeSet::from(["#visible".to_owned()]);
        loop {
            let mut grew = false;
            for (index, binding) in bindings.iter().enumerate() {
                if !wanted[index]
                    && binding_target(binding).is_some_and(|target| read.contains(target))
                {
                    wanted[index] = true;
                    grew = true;
                    read.extend(properties_read(binding));
                }
            }
            if !grew {
                break;
            }
        }
        let bindings = bindings
            .iter()
            .zip(&wanted)
            .filter_map(|(binding, wanted)| wanted.then(|| binding.clone()))
            .collect::<Vec<_>>();
        let bag = if bindings.is_empty() {
            serde_json::Map::new()
        } else {
            bag.and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter(|(name, _)| read.contains(name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        };
        Self { bindings, bag }
    }

    /// Adds rules not already present; the first value for a property-bag entry wins.
    fn extend(&mut self, other: Self) {
        for binding in other.bindings {
            if !self.bindings.contains(&binding) {
                self.bindings.push(binding);
            }
        }
        for (name, value) in other.bag {
            self.bag.entry(name).or_insert(value);
        }
    }

    /// Appends these rules; properties the control already binds or holds keep their own source.
    fn merge_into(&self, control: &mut RawControl) -> bool {
        if self.bindings.is_empty() {
            return false;
        }
        let mut changed = false;
        if let Some(bindings) = control
            .props
            .entry("bindings".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
        {
            let supplied = bindings
                .iter()
                .filter_map(binding_target)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            for rule in &self.bindings {
                let target = binding_target(rule);
                if (target == Some("#visible")
                    || !target.is_some_and(|target| supplied.contains(target)))
                    && !bindings.contains(rule)
                {
                    bindings.push(rule.clone());
                    changed = true;
                }
            }
        }
        if !self.bag.is_empty()
            && let Some(bag) = control
                .props
                .entry("property_bag".to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()
        {
            for (name, value) in &self.bag {
                if !bag.contains_key(name) {
                    bag.insert(name.clone(), value.clone());
                    changed = true;
                }
            }
        }
        changed
    }
}

/// The property a binding writes.
fn binding_target(binding: &Value) -> Option<&str> {
    binding
        .get("target_property_name")
        .or_else(|| binding.get("binding_name_override"))
        .or_else(|| binding.get("binding_name"))
        .and_then(Value::as_str)
}

/// Every `#property` a binding's source or name expression reads.
fn properties_read(binding: &Value) -> Vec<String> {
    ["source_property_name", "binding_name"]
        .into_iter()
        .filter_map(|key| binding.get(key).and_then(Value::as_str))
        .flat_map(|text| {
            text.match_indices('#').map(move |(start, _)| {
                let name = text[start + 1..]
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default();
                format!("#{name}")
            })
        })
        .filter(|name| name.len() > 1)
        .collect()
}

fn collect_globals(control: &RawControl, names: &mut BTreeSet<String>) {
    fn visit(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::String(text) if text.starts_with('$') => {
                names.insert(text[1..].to_owned());
            }
            Value::Array(values) => values.iter().for_each(|value| visit(value, names)),
            Value::Object(values) => values.values().for_each(|value| visit(value, names)),
            _ => {}
        }
    }
    for value in control.props.values() {
        visit(value, names);
    }
    for child in &control.children {
        collect_globals(child, names);
    }
}

fn suppress_chat(control: &mut RawControl, chat: &BTreeSet<ControlRef>) {
    let inherited = control
        .base
        .as_deref()
        .is_some_and(|base| chat.contains(&ControlRef::parse(base, &control.owner_ns)));
    let own_factory = control.props.get("type").and_then(Value::as_str) == Some("factory")
        && (control.props.contains_key("control_name")
            || control.props.contains_key("control_ids"));
    let fields = if own_factory {
        Some(&mut control.props)
    } else {
        control
            .props
            .get_mut("factory")
            .and_then(Value::as_object_mut)
    };
    let is_chat = |target: &str| chat.contains(&ControlRef::parse(target, &control.owner_ns));
    let ordinary_chat = fields.is_some_and(|fields| {
        if let Some(target) = fields
            .get("control_name")
            .and_then(Value::as_str)
            .filter(|target| !target.is_empty())
        {
            return is_chat(target);
        }
        if let Some(ids) = fields.get_mut("control_ids").and_then(Value::as_object_mut) {
            ids.retain(|_, target| !target.as_str().is_some_and(is_chat));
        }
        false
    });
    if inherited || ordinary_chat {
        control
            .props
            .insert("ignored".to_owned(), Value::Bool(true));
    }
    for child in &mut control.children {
        suppress_chat(child, chat);
    }
}

fn collect_definition(catalog: &Catalog, reference: ControlRef, seen: &mut BTreeSet<ControlRef>) {
    // Top chat measures these pack-owned controls through invisible padding.
    // Restoring chat must preserve their server textures, bindings and offsets.
    if reference.namespace != "hud"
        || matches!(
            reference.name.as_str(),
            "player_position" | "number_of_days_played"
        )
        || !seen.insert(reference.clone())
    {
        return;
    }
    let Some(control) = catalog.lookup(&reference.namespace, &reference.name) else {
        return;
    };
    collect_control(catalog, control, seen);
}

fn collect_control(catalog: &Catalog, control: &RawControl, seen: &mut BTreeSet<ControlRef>) {
    if let Some(base) = &control.base {
        collect_definition(catalog, ControlRef::parse(base, &control.owner_ns), seen);
    }
    for value in control.props.values() {
        collect_value(catalog, value, &control.owner_ns, seen);
    }
    for child in &control.children {
        collect_control(catalog, child, seen);
    }
}

fn collect_value(
    catalog: &Catalog,
    value: &Value,
    namespace: &str,
    seen: &mut BTreeSet<ControlRef>,
) {
    match value {
        Value::String(text) if text.starts_with('@') || text.contains('@') => {
            collect_definition(catalog, ControlRef::parse(text, namespace), seen);
        }
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_value(catalog, value, namespace, seen)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_value(catalog, value, namespace, seen)),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
