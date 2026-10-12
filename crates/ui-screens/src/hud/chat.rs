//! Chat layout is application-owned; other HUD controls retain pack priority.

use std::collections::{BTreeMap, BTreeSet};

use json_ui::{Catalog, ControlRef, RawControl};
use serde_json::Value;

const ROOTS: [&str; 4] = ["java_chat", "chat_panel", "chat_grid_item", "chat_label"];

pub(super) fn prefer_builtin(catalog: &mut Catalog, builtin: &Catalog) {
    let Some(screen) = builtin.lookup("hud", "hud_screen") else {
        return;
    };
    let Some(overlay) = screen.props.get("$additional_screen_content").cloned() else {
        return;
    };
    let Some(reference) = overlay.as_str() else {
        return;
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
    let visibility = ROOTS
        .into_iter()
        .filter_map(|name| {
            let reference = ControlRef::new("hud", name);
            let bindings = visibility_bindings(catalog.lookup("hud", name)?);
            Some((reference, bindings))
        })
        .collect::<BTreeMap<_, _>>();
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
            if let Some(bindings) = visibility.get(&reference)
                && let Some(target) = control
                    .props
                    .entry("bindings".to_owned())
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
            {
                for binding in bindings {
                    if !target.contains(binding) {
                        target.push(binding.clone());
                    }
                }
            }
            collect_globals(&control, &mut global_names);
            catalog.insert(control);
        }
    }
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
}

fn visibility_bindings(control: &RawControl) -> Vec<Value> {
    control
        .props
        .get("bindings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|binding| {
            let target = binding
                .get("target_property_name")
                .or_else(|| binding.get("binding_name_override"))
                .or_else(|| binding.get("binding_name"));
            target.and_then(Value::as_str) == Some("#visible")
        })
        .cloned()
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
