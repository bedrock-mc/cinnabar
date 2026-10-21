//! Finite tile collections with immutable artwork and independently filtered order.

use super::{Surface, SurfaceValue, identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A collection can represent a catalogue without expanding every tile's descendants.
pub const MAX_ITEMS: usize = 256;
/// Includes a four-column, three-row viewport and its bounded virtualization margin.
pub const MAX_VISIBLE_CELLS: usize = 26;

/// A reusable native grid with a small mutable list of immutable item indexes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceCollection {
    pub definition: Arc<CollectionDefinition>,
    pub order: Vec<usize>,
}

impl PartialEq for SurfaceCollection {
    /// Admitted immutable metadata can compare by identity during ordinary frame publication.
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
            && (Arc::ptr_eq(&self.definition, &other.definition)
                || self.definition == other.definition)
    }
}

/// Geometry and tile facts admitted once when loading the local component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDefinition {
    pub template: Surface,
    pub columns: usize,
    pub viewport: [f64; 2],
    pub items: Vec<CollectionItem>,
}

/// Plain scalar fields, native controls and optional vector artwork for one tile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionItem {
    #[serde(default)]
    pub values: BTreeMap<String, SurfaceValue>,
    #[serde(default)]
    pub controls: BTreeMap<String, usize>,
    #[serde(default)]
    pub art: Option<CollectionArt>,
}

/// A finite vector path supplied to the tile's ordinary binding bags.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionArt {
    pub view_box: [f32; 4],
    pub paths: Vec<Vec<[f32; 2]>>,
    pub stroke_width: f32,
    pub closed: bool,
}

impl SurfaceCollection {
    /// The maximum initial window admitted by the fixed native viewport geometry.
    pub const INITIAL_WINDOW: usize = MAX_VISIBLE_CELLS;
    /// Checks a complete, unique filtered order without altering its original catalogue.
    pub fn validate_order(&self) -> Result<(), String> {
        let mut seen = [false; MAX_ITEMS];
        if self.definition.items.len() > MAX_ITEMS
            || self.order.len() > self.definition.items.len()
            || self.order.iter().any(|i| {
                *i >= self.definition.items.len() || std::mem::replace(&mut seen[*i], true)
            })
        {
            return Err("surface collection order repeats or names an unknown item".into());
        }
        Ok(())
    }

    /// Admits fixed viewport geometry, finite tile data and a small static template.
    pub(super) fn validate(&self, name: &str) -> Result<(), String> {
        if !identifier(name) || name.contains('.') || self.definition.items.len() > MAX_ITEMS {
            return Err("surface collection exceeds its identity or item bounds".into());
        }
        self.validate_order()?;
        let d = &self.definition;
        if !d.template.collections.is_empty()
            || !d.template.transitions.is_empty()
            || !d.template.bindings.is_empty()
        {
            return Err("surface collection templates cannot contain live surface state".into());
        }
        d.template.validate()?;
        let document: Value =
            serde_json::from_str(&d.template.resolved_document()?).map_err(|e| e.to_string())?;
        let local = d
            .template
            .screen
            .split_once('.')
            .expect("validated screen")
            .1;
        let root = &document[local];
        bounded_clipping(root)?;
        let cell =
            pixel_size(root.get("size")).ok_or("collection template requires a fixed size")?;
        if d.columns == 0
            || d.columns > 8
            || pixel_size(Some(&json!(d.viewport))).is_none()
            || d.viewport[0] - 8.0 < cell[0] * d.columns as f64
        {
            return Err("surface collection viewport or column geometry is invalid".into());
        }
        let visible = ((d.viewport[1] / cell[1]).ceil() as usize + 1) * d.columns;
        if visible + 2 * json_ui::BindState::grid_window_margin(visible) > MAX_VISIBLE_CELLS
            || count(root) > 32
        {
            return Err("surface collection exceeds its visible tile budget".into());
        }
        let mut roles = BTreeSet::new();
        for action in d.template.actions()? {
            let role = action
                .strip_prefix("mod.item:")
                .filter(|role| identifier(role) && !role.contains('.'))
                .ok_or("collection template actions must use a declared item role")?;
            roles.insert(role.to_owned());
        }
        for item in &d.items {
            if item.values.len() > 16 || item.controls.len() > 8 {
                return Err("surface collection item has too many fields".into());
            }
            let values = Surface {
                previews: Default::default(),
                collections: Default::default(),
                screen: String::new(),
                document: String::new(),
                bindings: item.values.clone(),
                transitions: Default::default(),
            };
            values.validate_bindings()?;
            if item.values.keys().any(|key| {
                !key.starts_with("#item_") || key.starts_with("#item_art_") || key == "#item_exists"
            }) || item
                .controls
                .keys()
                .any(|role| !identifier(role) || role.contains('.'))
                || roles.iter().any(|role| !item.controls.contains_key(role))
            {
                return Err("surface collection item fields or action roles are invalid".into());
            }
            for role in item.controls.keys() {
                if item
                    .values
                    .keys()
                    .any(|key| key.starts_with(&format!("#item_{role}_")))
                {
                    return Err("surface collection value shadows a native control field".into());
                }
            }
            if let Some(art) = &item.art {
                let fields = serde_json::to_value(art).map_err(|e| e.to_string())?;
                let fields = fields
                    .as_object()
                    .expect("art is an object")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if json_ui::vector_path::parse(&fields).is_none() {
                    return Err("surface collection artwork is invalid".into());
                }
            }
        }
        Ok(())
    }

    /// Maps a hit through the exact item order used for its rendered frame.
    pub fn control(&self, index: usize, role: &str) -> Option<usize> {
        self.definition
            .items
            .get(*self.order.get(index)?)?
            .controls
            .get(role)
            .copied()
    }

    /// Emits one fixed native viewport, grid and scrollbar, with no authored dynamic factories.
    pub(super) fn grid(&self, name: &str, template: &str) -> Value {
        let d = &self.definition;
        let [width, height] = d.viewport;
        json!({"type":"scroll_view","size":[width,height],"clip_children":true,
            "anchor_from":"top_left","anchor_to":"top_left",
            "scroll_speed":24,"scroll_content":"content","scroll_view_port":"viewport",
            "scrollbar_track":"track","scrollbar_box":"thumb","scroll_box_and_track_panel":"bar",
            "scrollbar_track_button":super::SCROLL_TRACK_ACTION,
            "controls":[{"viewport":{"type":"panel","size":[width-8.0,height],"clip_children":true,
                "anchor_from":"top_left","anchor_to":"top_left",
                "controls":[{"content":{"type":"grid","size":[width-8.0,"default"],
                    "anchor_from":"top_left","anchor_to":"top_left",
                    "grid_dimensions":[d.columns,0],"grid_dimension_binding":"#grid_dimensions",
                    "collection_name":name,"grid_item_template":template,
                    "bindings":[{"binding_name":format!("#collection_{name}_dimensions"),
                        "binding_name_override":"#grid_dimensions"}]}}]}},
                {"bar":{"type":"panel","size":[4,height],"anchor_from":"top_right","anchor_to":"top_right",
                    "controls":[{"track":{"type":"scroll_track","size":[4,height],
                    "anchor_from":"top_left","anchor_to":"top_left",
                    "button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":super::SCROLL_TRACK_ACTION}],
                    "controls":[]}}, {"thumb":{"type":"scrollbar_box","size":[4,20],"draggable":"vertical",
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "controls":[{"fill":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                            "size":["100%","100%"],"radius":1,"color":[0.6,0.62,0.6,0.5]}}]}}]}}]})
    }
}

/// Tile descendants must respect their native cell clip so offscreen cells cannot paint into view.
fn bounded_clipping(value: &Value) -> Result<(), String> {
    if let Some(object) = value.as_object() {
        if object
            .get("allow_clipping")
            .is_some_and(|v| v != &Value::Bool(true))
        {
            return Err("collection tiles cannot escape their native clipping bounds".into());
        }
        if let Some(children) = object.get("controls").and_then(Value::as_array) {
            for child in children {
                for body in child
                    .as_object()
                    .ok_or("named tile child required")?
                    .values()
                {
                    bounded_clipping(body)?;
                }
            }
        }
    }
    Ok(())
}

/// Reads literal positive logical dimensions, excluding bindings and relative size expressions.
fn pixel_size(value: Option<&Value>) -> Option<[f64; 2]> {
    let v = value?.as_array()?;
    if v.len() != 2 {
        return None;
    }
    let pair = [v[0].as_f64()?, v[1].as_f64()?];
    pair.iter()
        .all(|v| v.is_finite() && *v >= 1.0 && *v <= 4096.0)
        .then_some(pair)
}

/// Counts static tile descendants before the grid may replicate their property payloads.
pub(super) fn count(value: &Value) -> usize {
    1 + value
        .get("controls")
        .and_then(Value::as_array)
        .map_or(0, |children| {
            children
                .iter()
                .map(|entry| {
                    entry
                        .as_object()
                        .map_or(0, |entry| entry.values().map(count).sum::<usize>())
                })
                .sum::<usize>()
        })
}

/// Replaces reserved item fields with this native grid's scoped collection bindings.
pub(super) fn scope(value: &mut Value, name: &str) {
    if let Some(object) = value.as_object_mut() {
        if let Some(bindings) = object.get_mut("bindings").and_then(Value::as_array_mut) {
            for binding in bindings {
                if binding
                    .get("binding_name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        let properties = json_ui::expression_properties(name);
                        !properties.is_empty()
                            && properties.iter().all(|key| key.starts_with("#item_"))
                    })
                {
                    binding["binding_type"] = json!("collection");
                    binding["binding_collection_name"] = json!(name);
                }
            }
        }
        if let Some(children) = object.get_mut("controls").and_then(Value::as_array_mut) {
            for entry in children {
                if let Some(entry) = entry.as_object_mut() {
                    for child in entry.values_mut() {
                        scope(child, name);
                    }
                }
            }
        }
    }
}

/// Installs native grids and checks their clipping controls under inherited rotation.
pub(super) fn replace(
    value: &mut Value,
    grids: &mut BTreeMap<String, Value>,
    rotated: bool,
) -> Result<(), String> {
    if let Some(object) = value.as_object_mut() {
        if let Some(name) = object.remove("cinnabar_collection") {
            let name = name
                .as_str()
                .ok_or("collection placeholder must name a collection")?;
            let grid = grids
                .remove(name)
                .ok_or("unknown or repeated collection placeholder")?;
            if object.get("type").and_then(Value::as_str) != Some("panel")
                || object.contains_key("controls")
                || object.contains_key("bindings")
            {
                return Err("collection placeholder must be an empty static panel".into());
            }
            for (key, value) in grid.as_object().expect("native grid").iter() {
                object.insert(key.clone(), value.clone());
            }
        }
        let rotated = super::paint_transform::control(object, rotated)?;
        if let Some(children) = object.get_mut("controls").and_then(Value::as_array_mut) {
            for entry in children {
                if let Some(entry) = entry.as_object_mut() {
                    for child in entry.values_mut() {
                        replace(child, grids, rotated)?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Item-only expressions read the current card, while host globals retain their scope.
    #[test]
    fn preview_visibility_expressions_follow_their_collection_item() {
        let mut node = json!({"type":"custom","bindings":[
            {"binding_name":"(#item_kind = 'armor')","binding_name_override":"#visible"},
            {"binding_name":"#item_name","binding_name_override":"#text"},
            {"binding_name":"(#category = 'all')","binding_name_override":"#visible"},
            {"binding_name":"(#item_kind = #category)","binding_name_override":"#visible"},
            {"binding_name":"((#item_kind = '#category') and #item_exists)","binding_name_override":"#visible"},
            {"binding_name":"('#item_kind' = 'armor')","binding_name_override":"#visible"},
            {"binding_name":"true","binding_name_override":"#visible"}
        ]});
        scope(&mut node, "modules");
        for i in [0, 1, 4] {
            assert_eq!(node["bindings"][i]["binding_type"], "collection");
            assert_eq!(node["bindings"][i]["binding_collection_name"], "modules");
        }
        for i in [2, 3, 5, 6] {
            assert!(node["bindings"][i].get("binding_type").is_none());
        }
    }

    /// Admits the four-column browser while retaining a finite window for oversized viewports.
    #[test]
    fn four_column_browser_keeps_a_bounded_retained_window() {
        let mut collection = SurfaceCollection {
            definition: Arc::new(CollectionDefinition {
                template: Surface {
                    screen: "cards.tile".into(),
                    document: json!({"namespace":"cards","tile":{"type":"panel","size":[111,97]}})
                        .to_string(),
                    bindings: Default::default(),
                    transitions: Default::default(),
                    collections: Default::default(),
                    previews: Default::default(),
                },
                columns: 4,
                viewport: [452., 289.],
                items: vec![CollectionItem::default(); 66],
            }),
            order: (0..66).collect(),
        };
        collection.validate("modules").unwrap();
        Arc::make_mut(&mut collection.definition).viewport[1] = 486.;
        assert!(collection.validate("modules").is_err());
    }
}
