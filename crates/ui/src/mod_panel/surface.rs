//! Bounded extension-authored JSON-UI and its changing scalar bindings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
pub mod backdrop;
mod collection;
pub mod hud_card;
pub mod local_player_head;
mod paint_transform;
pub mod preview;
mod shadow;
mod static_templates;
mod transition;
pub use collection::{CollectionArt, CollectionDefinition, CollectionItem, SurfaceCollection};
use static_templates::{acyclic, authored, bound_expansion, local_reference, resolved_value};
pub use transition::{MAX_TRANSITION_SECONDS, SurfaceTransition, TransitionEasing};

/// Host-owned track action consumed without invoking an extension control.
pub const SCROLL_TRACK_ACTION: &str = "mod.scroll_track";

pub const MAX_SURFACE_BYTES: usize = 96 * 1024;
pub const MAX_SURFACE_NODES: usize = 4096;
pub const MAX_SURFACE_BINDINGS: usize = 128;
pub(crate) const MAX_BINDING_NAME_BYTES: usize = 96;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub screen: String,
    pub document: String,
    /// Independently bound, inert pack controls fitted inside authored preview nodes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub previews: BTreeMap<String, preview::ControlPreview>,
    #[serde(default)]
    pub bindings: BTreeMap<String, SurfaceValue>,
    /// Numeric bindings that ease toward each new value, keyed by binding name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transitions: BTreeMap<String, SurfaceTransition>,
    /// Bounded immutable tile data and the current filtered order of each native grid.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub collections: BTreeMap<String, SurfaceCollection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SurfaceValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Vector(Vec<f64>),
}

impl SurfaceValue {
    /// Rejects unbounded scalar values before a host retains extension bindings.
    pub fn validate(&self) -> Result<(), String> {
        let valid = match self {
            Self::Bool(_) => true,
            Self::Number(number) => bounded(number),
            Self::Text(text) => text.len() <= 256 && !text.chars().any(char::is_control),
            Self::Vector(numbers) => {
                (2..=4).contains(&numbers.len()) && numbers.iter().all(bounded)
            }
        };
        if valid {
            Ok(())
        } else {
            Err("surface binding value exceeds its bounds".into())
        }
    }

    /// The components a transition eases: one for a number, each element of a 2–4-number
    /// array, and none for other values.
    pub fn numbers(&self) -> Option<&[f64]> {
        match self {
            SurfaceValue::Number(number) => Some(std::slice::from_ref(number)),
            SurfaceValue::Vector(numbers) if (2..=4).contains(&numbers.len()) => Some(numbers),
            _ => None,
        }
    }
}

impl Surface {
    /// Rejects oversized trees and dynamic factories before the host retains them.
    pub fn validate(&self) -> Result<(), String> {
        preview::validate(&self.previews)?;
        let _ = self.walk()?;
        self.validate_bindings()
    }

    /// Validates only changing scalar values and their declared transitions, retaining the
    /// already checked document. A transition may name a binding a later update supplies.
    pub fn validate_bindings(&self) -> Result<(), String> {
        if self.bindings.len() > MAX_SURFACE_BINDINGS {
            return Err("surface has too many bindings".into());
        }
        for (name, value) in &self.bindings {
            if !binding_name(name) {
                return Err("surface binding name is invalid".into());
            }
            value.validate()?;
            backdrop::validate_world_binding(name, value)?;
        }
        if self.transitions.len() > MAX_SURFACE_BINDINGS {
            return Err("surface has too many transitions".into());
        }
        for (name, transition) in &self.transitions {
            if !binding_name(name) {
                return Err("surface transition name is invalid".into());
            }
            transition.validate()?;
        }
        for collection in self.collections.values() {
            collection.validate_order()?;
        }
        Ok(())
    }

    /// Returns authored pressed actions so the owning capability validates routes.
    pub fn actions(&self) -> Result<Vec<String>, String> {
        self.walk()
    }

    /// Returns a canonical static document consumed by the native renderer.
    /// No host context can later select an authored branch that escaped validation.
    pub fn resolved_document(&self) -> Result<String, String> {
        let mut root = self.resolved_root()?;
        let namespace = self
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?
            .0;
        let name = self.screen.split_once('.').expect("qualified checked").1;
        let mut document = serde_json::Map::new();
        document.insert("namespace".into(), Value::String(namespace.into()));
        let mut grids = BTreeMap::new();
        for (name, collection) in &self.collections {
            collection.validate(name)?;
            let template: Value =
                serde_json::from_str(&collection.definition.template.resolved_document()?)
                    .map_err(|e| e.to_string())?;
            let source = collection
                .definition
                .template
                .screen
                .split_once('.')
                .expect("validated template")
                .1;
            let mut tile = template[source].clone();
            collection::scope(&mut tile, name);
            let template_name = format!("cinnabar_tile_{name}");
            if template_name == self.screen.split_once('.').expect("validated screen").1 {
                return Err("surface root collides with a native collection template".into());
            }
            let tile = serde_json::json!({"type":"panel","size":tile["size"],"clip_children":true,
                "bindings":[{"binding_type":"collection","binding_collection_name":name,
                    "binding_name":"#item_exists","binding_name_override":"#visible"}],
                "controls":[{"tile":tile}]});
            document.insert(template_name.clone(), tile);
            grids.insert(
                name.clone(),
                collection.grid(name, &format!("{namespace}.{template_name}")),
            );
        }
        collection::replace(&mut root, &mut grids, false)?;
        if !grids.is_empty() {
            return Err("surface collection is missing its placeholder".into());
        }
        document.insert(name.into(), root);
        Ok(Value::Object(document).to_string())
    }
    /// Collects capability-routed actions from the fully validated static tree.
    fn walk(&self) -> Result<Vec<String>, String> {
        let root = self.resolved_root()?;
        preview::node(&root, &self.previews)?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(&root, 0, &mut count, &mut actions, false, false)?;
        if self.collections.len() > 4 {
            return Err("surface has too many collections".into());
        }
        if serde_json::to_vec(&self.collections)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_SURFACE_BYTES * 4
        {
            return Err("surface collection metadata exceeds its byte budget".into());
        }
        let mut grids = BTreeMap::new();
        for (name, collection) in &self.collections {
            collection.validate(name)?;
            let template: Value =
                serde_json::from_str(&collection.definition.template.resolved_document()?)
                    .map_err(|e| e.to_string())?;
            let source = collection
                .definition
                .template
                .screen
                .split_once('.')
                .expect("validated template")
                .1;
            count += collection
                .definition
                .items
                .len()
                .div_ceil(collection.definition.columns)
                * collection.definition.columns
                + (collection::count(&template[source]) + 1) * (collection::MAX_VISIBLE_CELLS + 1)
                + 8;
            if count > MAX_SURFACE_NODES {
                return Err("surface collections exceed the live node budget".into());
            }
            for item in &collection.definition.items {
                actions.extend(item.controls.values().map(|i| format!("mod.control:{i}")));
            }
            grids.insert(name.clone(), collection.grid(name, "unused.template"));
        }
        let mut placeholders = root;
        collection::replace(&mut placeholders, &mut grids, false)?;
        if !grids.is_empty() {
            return Err("surface collection is missing its placeholder".into());
        }
        Ok(actions)
    }

    /// Compares immutable route artwork and data while excluding filtered item order.
    pub fn same_definition(&self, other: &Self) -> bool {
        self.screen == other.screen
            && self.document == other.document
            && self.previews == other.previews
            && self.collections.len() == other.collections.len()
            && self.collections.iter().all(|(name, collection)| {
                other.collections.get(name).is_some_and(|other| {
                    Arc::ptr_eq(&collection.definition, &other.definition)
                        || collection.definition == other.definition
                })
            })
    }
    /// Resolves only static local templates before validating the emitted tree.
    fn resolved_root(&self) -> Result<Value, String> {
        if self.document.len() > MAX_SURFACE_BYTES {
            return Err("surface document exceeds byte limit".into());
        }
        let Some((namespace, name)) = self.screen.split_once('.') else {
            return Err("surface screen must be qualified".into());
        };
        if !identifier(namespace) || !identifier(name) {
            return Err("surface screen is invalid".into());
        }
        if matches!(namespace, "cinnabar_personal" | "cinnabar_hud_editor") {
            return Err("surface namespace is reserved by the host".into());
        }
        let document: Value = serde_json::from_str(&self.document)
            .map_err(|error| format!("invalid surface JSON: {error}"))?;
        let object = document
            .as_object()
            .ok_or("surface document must be an object")?;
        if object.get("namespace").and_then(Value::as_str) != Some(namespace) {
            return Err("surface document must contain its own namespace".into());
        }
        let mut definitions = BTreeMap::new();
        for (key, body) in object.iter().filter(|(key, _)| *key != "namespace") {
            let (local, base) = local_reference(key, namespace)?;
            if definitions.insert(local, (body, base)).is_some() {
                return Err("duplicate surface definition".into());
            }
        }
        if !definitions.contains_key(name) {
            return Err("surface screen is missing".into());
        }
        let mut graph = BTreeMap::new();
        let mut authored_nodes = 0;
        for (key, (body, base)) in &definitions {
            let mut references = Vec::new();
            if let Some(base) = base {
                references.push(base.clone());
            }
            authored(body, namespace, 0, &mut authored_nodes, &mut references)?;
            graph.insert(key.clone(), references);
        }
        for key in graph.keys() {
            acyclic(
                key,
                &graph,
                &mut Vec::new(),
                &mut std::collections::BTreeSet::new(),
            )?;
        }
        bound_expansion(name, namespace, &definitions)?;
        let files = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
        let catalog = json_ui::Catalog::from_files([
            ("ui/_global_variables.json", &b"{}"[..]),
            ("ui/_ui_defs.json", &files[..]),
            ("ui/extension_surface.json", self.document.as_bytes()),
        ])
        .map_err(|error| error.to_string())?
        .with_paint_transforms();
        if !catalog.diagnostics().is_empty() {
            return Err(catalog.diagnostics().join("; "));
        }
        let resolved = json_ui::resolve_with_limits(
            &catalog,
            &self.screen,
            &json_ui::Context::default(),
            33,
            MAX_SURFACE_NODES,
        );
        if !resolved.diagnostics.is_empty() {
            return Err(resolved.diagnostics.join("; "));
        }
        let root = resolved
            .control
            .as_ref()
            .ok_or("surface root cannot be resolved")?;
        let root = resolved_value(root)?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(&root, 0, &mut count, &mut actions, false, false)?;
        Ok(root)
    }
}

/// A finite binding number within the surface value bound.
fn bounded(number: &f64) -> bool {
    number.is_finite() && number.abs() <= 1_000_000.0
}

/// Matches the names accepted by a local scalar data source.
pub(crate) fn binding_name(name: &str) -> bool {
    name.strip_prefix('#').is_some_and(identifier)
}

/// Restricts names to an unambiguous local identifier.
pub(crate) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_BINDING_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

/// Checks a finite authored tree without resolving factories or inheritance; `rotated`
/// reports an ancestor that may rotate its descendants and `filtered` one that filters them.
fn node(
    value: &Value,
    depth: usize,
    count: &mut usize,
    actions: &mut Vec<String>,
    rotated: bool,
    filtered: bool,
) -> Result<(), String> {
    *count += 1;
    if depth > 32 || *count > MAX_SURFACE_NODES {
        return Err("surface tree exceeds structural limits".into());
    }
    let object = value
        .as_object()
        .ok_or("surface control must be an object")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("surface control type is missing")?;
    if !matches!(
        kind,
        "screen"
            | "panel"
            | "input_panel"
            | "button"
            | "label"
            | "image"
            | "custom"
            | "stack_panel"
            | "scroll_view"
            | "scroll_track"
            | "scrollbar_box"
    ) || (kind == "screen" && depth != 0)
    {
        return Err("surface control type is unsupported".into());
    }
    if kind == "image" {
        local_player_head::validate(object)?;
    }
    for (name, value) in object {
        if name.starts_with('$') {
            return Err("surface variable declarations are unsupported".into());
        }
        if name != "controls" {
            property(value, 0)?;
        }
        if matches!(
            name.as_str(),
            "alpha" | "uv" | "color" | "clip_ratio" | "size" | "offset"
        ) && value.as_str().is_some_and(|text| text.starts_with('@'))
        {
            return Err("surface animation references are unsupported".into());
        }
    }
    for name in [
        "factory",
        "variables",
        "anims",
        "animations",
        "collection_name",
        "grid_dimensions",
        "property_bag",
        "property_bag_for_children",
    ] {
        if object.contains_key(name) {
            return Err("surface dynamic expansion is unsupported".into());
        }
    }
    let rotated = paint_transform::control(object, rotated)?;
    let filtered = backdrop::filter(object, filtered)?;
    if let Some(bindings) = object.get("bindings") {
        for binding in bindings
            .as_array()
            .ok_or("surface bindings must be an array")?
        {
            let binding = binding
                .as_object()
                .ok_or("surface binding must be an object")?;
            if binding
                .get("binding_type")
                .is_some_and(|kind| kind.as_str() != Some("global"))
            {
                return Err("surface bindings must use the typed global data source".into());
            }
        }
    }
    if kind == "custom"
        && !object
            .get("renderer")
            .and_then(Value::as_str)
            .is_some_and(|name| {
                matches!(
                    name,
                    json_ui::rounded::RENDERER
                        | backdrop::GLASS_RENDERER
                        | "cinnabar_vector_icon"
                        | "cinnabar_vector_path"
                        | "gradient_renderer"
                        | preview::RENDERER
                        | hud_card::RENDERER
                )
            })
    {
        return Err("surface custom renderer is unsupported".into());
    }
    if object.get("renderer").and_then(Value::as_str) == Some(backdrop::GLASS_RENDERER) {
        backdrop::validate_glass(object)?;
    }
    if object.get("renderer").and_then(Value::as_str) == Some(hud_card::RENDERER) {
        hud_card::validate(object)?;
    }
    if object.get("renderer").and_then(Value::as_str) == Some(json_ui::rounded::RENDERER) {
        shadow::validate(object)?;
    }
    if let Some(mappings) = object.get("button_mappings") {
        let mappings = mappings
            .as_array()
            .ok_or("surface mappings must be an array")?;
        if mappings.len() > 4 {
            return Err("surface has too many button mappings".into());
        }
        for mapping in mappings {
            if mapping.get("from_button_id").and_then(Value::as_str) != Some("button.menu_select")
                || mapping.get("mapping_type").and_then(Value::as_str) != Some("pressed")
            {
                return Err("surface buttons require pressed menu-select mappings".into());
            }
            let action = mapping
                .get("to_button_id")
                .and_then(Value::as_str)
                .ok_or("surface action is missing")?;
            if action.len() > 96 {
                return Err("surface action exceeds its bound".into());
            }
            if kind != "scroll_track" || action != SCROLL_TRACK_ACTION {
                actions.push(action.to_owned());
            }
        }
    }
    if let Some(children) = object.get("controls") {
        for entry in children
            .as_array()
            .ok_or("surface controls must be an array")?
        {
            let entry = entry.as_object().ok_or("surface child must be named")?;
            if entry.len() != 1 {
                return Err("surface child must have one name".into());
            }
            let (name, body) = entry.iter().next().expect("one member checked");
            if !identifier(name) || name.contains('@') {
                return Err("surface child inheritance is unsupported".into());
            }
            node(body, depth + 1, count, actions, rotated, filtered)?;
        }
    }
    Ok(())
}

/// Rejects structured aliases and inline animation graphs before JSON-UI substitution.
fn property(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("surface property exceeds structural limits".into());
    }
    match value {
        Value::String(text) if text.starts_with('$') => {
            return Err("surface variable references are unsupported".into());
        }
        Value::Array(items) => {
            for item in items {
                property(item, depth + 1)?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                if name.starts_with('$') || name == "anim_type" {
                    return Err(
                        "surface variable declarations and inline animations are unsupported"
                            .into(),
                    );
                }
                property(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
