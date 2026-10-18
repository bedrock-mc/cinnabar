//! Prevalidated local screen catalogs and bounded changes to retained panel values.

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;
use serde_json::Value;

use super::{Control, MAX_PANEL_CONTROLS, Panel, Surface, SurfaceValue, text};

/// Local catalogs are read once outside callbacks, independently of guest memory.
pub const MAX_PANEL_BUNDLE_BYTES: usize = 8 * 1024 * 1024;
/// Changing values never need to carry the retained document or control schema.
pub const MAX_PANEL_PATCH_BYTES: usize = 32 * 1024;
/// Maximum route documents admitted by one local catalog.
pub const MAX_PANEL_SURFACES: usize = 128;

/// An initial control schema with individually validated, reusable local routes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBundle {
    pub panel: Panel,
    pub surfaces: BTreeMap<String, Surface>,
    /// Optional controls owned by one route; absent routes use the initial schema.
    #[serde(default)]
    pub control_schemas: BTreeMap<String, Vec<Control>>,
}

impl PanelBundle {
    /// Checks the control identity and type facts each registered action was admitted against.
    pub fn matches_schema(&self, panel: &Panel) -> bool {
        self.matches_controls(&self.panel.controls, panel)
            || self.control_schemas.iter().any(|(id, controls)| {
                panel.surface.as_ref().is_some_and(|surface| {
                    self.surfaces
                        .get(id)
                        .is_some_and(|original| original.same_definition(surface))
                }) && self.matches_controls(controls, panel)
            })
    }

    /// Compares immutable action facts while allowing values and captions to change.
    fn matches_controls(&self, controls: &[Control], panel: &Panel) -> bool {
        controls.len() == panel.controls.len()
            && controls
                .iter()
                .zip(&panel.controls)
                .all(|(a, b)| a.same_schema(b))
    }

    /// Builds a route from its admitted static schema and initial values.
    pub fn route_panel(&self, id: &str) -> Option<Panel> {
        let mut panel = self.panel.clone();
        panel.surface = Some(self.surfaces.get(id)?.clone());
        if let Some(controls) = self.control_schemas.get(id) {
            panel.controls.clone_from(controls);
        }
        Some(panel)
    }

    /// Parses and validates every route against its declared control schema.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > MAX_PANEL_BUNDLE_BYTES {
            return Err("panel bundle exceeds byte limit".into());
        }
        let bundle: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        bundle.validate()?;
        Ok(bundle)
    }

    /// Checks caller-constructed bundles before allocating prepared route state.
    pub fn validate(&self) -> Result<(), String> {
        self.panel.validate()?;
        if self.surfaces.len() > MAX_PANEL_SURFACES {
            return Err("panel bundle has too many surfaces".into());
        }
        if self
            .control_schemas
            .keys()
            .any(|id| !self.surfaces.contains_key(id))
        {
            return Err("panel control schema names an unknown surface".into());
        }
        let document_bytes = self
            .surfaces
            .values()
            .map(|surface| serde_json::to_vec(surface).map(|v| v.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .into_iter()
            .sum::<usize>()
            + self
                .panel
                .surface
                .as_ref()
                .map(serde_json::to_vec)
                .transpose()
                .map_err(|e| e.to_string())?
                .map_or(0, |bytes| bytes.len())
            + serde_json::to_vec(&self.control_schemas)
                .map_err(|error| error.to_string())?
                .len();
        if document_bytes > MAX_PANEL_BUNDLE_BYTES {
            return Err("panel bundle exceeds byte limit".into());
        }
        for id in self.surfaces.keys() {
            if id.is_empty()
                || id.len() > 96
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
            {
                return Err("panel surface route is invalid".into());
            }
            let panel = self.route_panel(id).expect("route comes from this bundle");
            panel.validate()?;
        }
        Ok(())
    }
}

impl Control {
    /// Compares action identity, kind and editing bounds, excluding mutable text and values.
    pub fn same_schema(&self, other: &Self) -> bool {
        self.id() == other.id()
            && match self {
                Self::Toggle { .. } => matches!(other, Self::Toggle { .. }),
                Self::Button { .. } => matches!(other, Self::Button { .. }),
                Self::Keybind { .. } => matches!(other, Self::Keybind { .. }),
                Self::Slider { min, max, step, .. } => {
                    matches!(other, Self::Slider {min:b_min,max:b_max,step:b_step,..} if (min,max,step)==(b_min,b_max,b_step))
                }
                Self::Choice { options, .. } => {
                    matches!(other, Self::Choice {options:b,..} if options==b)
                }
                Self::Text {
                    min_chars,
                    max_chars,
                    ..
                } => {
                    matches!(other,Self::Text {min_chars:b_min,max_chars:b_max,..} if (min_chars,max_chars)==(b_min,b_max))
                }
            }
    }
}

/// A sparse control change and optional replacement of a route's scalar bindings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPatch {
    #[serde(default)]
    controls: Vec<ControlPatch>,
    /// Indexed scalar edits use the selected route's immutable control order.
    #[serde(default)]
    control_values: Vec<(usize, Value)>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    bindings: Option<BTreeMap<String, SurfaceValue>>,
    /// Sparse scalar edits preserve bindings belonging to other retained routes.
    #[serde(default)]
    binding_changes: Option<BTreeMap<String, SurfaceValue>>,
    /// Filters or sorts admitted tile data without resending its immutable metadata.
    #[serde(default)]
    collection_order: BTreeMap<String, Vec<usize>>,
    #[serde(default)]
    capture_key: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlPatch {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    capturing: Option<bool>,
    #[serde(default)]
    index: Option<u32>,
}

impl PanelPatch {
    /// Selects admitted controls without resetting panel metadata, then applies sparse changes.
    pub fn apply_bundle(self, current: &Panel, bundle: &PanelBundle) -> Result<Panel, String> {
        let Some(id) = &self.surface else {
            return self.apply(current, &bundle.surfaces);
        };
        if !bundle.surfaces.contains_key(id) {
            return Err("panel update names an unknown surface".into());
        }
        let mut selected = current.clone();
        let controls = bundle
            .control_schemas
            .get(id)
            .unwrap_or(&bundle.panel.controls);
        if !bundle.matches_controls(controls, current) {
            selected.controls.clone_from(controls);
        }
        self.apply(&selected, &bundle.surfaces)
    }

    /// Reads a bounded patch without resolving any retained screen definition.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > MAX_PANEL_PATCH_BYTES {
            return Err("panel update exceeds byte limit".into());
        }
        let patch: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if patch.controls.len() + patch.control_values.len() > MAX_PANEL_CONTROLS {
            return Err("panel update has too many controls".into());
        }
        Ok(patch)
    }

    /// Validates all changes before returning a replacement; errors leave the original intact.
    pub fn apply(
        self,
        current: &Panel,
        routes: &BTreeMap<String, Surface>,
    ) -> Result<Panel, String> {
        let mut panel = current.clone();
        let mut seen = HashSet::new();
        for patch in self.controls {
            if !seen.insert(patch.id.clone()) {
                return Err("panel update repeats a control".into());
            }
            let control = panel
                .controls
                .iter_mut()
                .find(|control| control.id() == patch.id)
                .ok_or("panel update names an unknown control")?;
            patch.apply(control)?;
        }
        for (index, value) in self.control_values {
            let control = panel
                .controls
                .get_mut(index)
                .ok_or("panel update names an unknown control index")?;
            if !seen.insert(control.id().to_owned()) {
                return Err("panel update repeats a control".into());
            }
            let mut patch = ControlPatch {
                id: control.id().to_owned(),
                label: None,
                value: Some(value),
                key: None,
                capturing: None,
                index: None,
            };
            if matches!(control, Control::Choice { .. }) {
                let index = patch
                    .value
                    .take()
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or("panel choice update requires an unsigned integer")?;
                patch.index = Some(index);
            }
            patch.apply(control)?;
        }
        if let Some(id) = self.surface {
            let mut route = routes
                .get(&id)
                .ok_or("panel update names an unknown surface")?
                .clone();
            if let Some(current) = &panel.surface {
                if current.same_definition(&route) {
                    route.collections.clone_from(&current.collections);
                }
                route.bindings.clone_from(&current.bindings);
            }
            panel.surface = Some(route);
        }
        if let Some(bindings) = self.bindings {
            let surface = panel
                .surface
                .as_mut()
                .ok_or("panel bindings require a retained surface")?;
            surface.bindings = bindings;
            surface.validate_bindings()?;
        }
        if let Some(changes) = self.binding_changes {
            let surface = panel
                .surface
                .as_mut()
                .ok_or("panel bindings require a retained surface")?;
            surface.bindings.extend(changes);
            surface.validate_bindings()?;
        }
        if let Some(capture) = self.capture_key {
            panel.capture_key = capture;
        }
        for (name, order) in self.collection_order {
            let collection = panel
                .surface
                .as_mut()
                .and_then(|surface| surface.collections.get_mut(&name))
                .ok_or("panel update names an unknown collection")?;
            collection.order = order;
            collection.validate_order()?;
        }
        Ok(panel)
    }
}

impl ControlPatch {
    /// Applies only fields belonging to this existing control's declared kind.
    fn apply(self, control: &mut Control) -> Result<(), String> {
        let Self {
            label,
            value,
            key,
            capturing,
            index,
            ..
        } = self;
        let invalid = || "panel update fields do not match the declared control kind".to_owned();
        match control {
            Control::Text {
                value: current,
                min_chars,
                max_chars,
                ..
            } => {
                if key.is_some() || capturing.is_some() || index.is_some() {
                    return Err(invalid());
                }
                if let Some(value) = value {
                    let value = value.as_str().ok_or_else(invalid)?;
                    super::validate_text_value(value, *min_chars, *max_chars)?;
                    *current = value.to_owned();
                }
            }
            Control::Toggle { value: current, .. } => {
                if key.is_some() || capturing.is_some() || index.is_some() {
                    return Err(invalid());
                }
                if let Some(value) = value {
                    *current = value.as_bool().ok_or_else(invalid)?;
                }
            }
            Control::Slider {
                value: current,
                min,
                max,
                ..
            } => {
                if key.is_some() || capturing.is_some() || index.is_some() {
                    return Err(invalid());
                }
                if let Some(value) = value {
                    let number = value.as_f64().ok_or_else(invalid)? as f32;
                    if !number.is_finite() || number < *min || number > *max {
                        return Err(
                            "panel update slider value is outside its declared bounds".into()
                        );
                    }
                    *current = number;
                }
            }
            Control::Keybind {
                key: current,
                capturing: active,
                ..
            } => {
                if value.is_some() || index.is_some() {
                    return Err(invalid());
                }
                if let Some(key) = key {
                    if key.len() > super::MAX_PANEL_ID_BYTES || key.chars().any(char::is_control) {
                        return Err("panel update keybind exceeds its text bound".into());
                    }
                    *current = key;
                }
                if let Some(capturing) = capturing {
                    *active = capturing;
                }
            }
            Control::Choice {
                index: current,
                options,
                ..
            } => {
                if value.is_some() || key.is_some() || capturing.is_some() {
                    return Err(invalid());
                }
                if let Some(index) = index {
                    if index as usize >= options.len() {
                        return Err("panel update choice index is invalid".into());
                    }
                    *current = index;
                }
            }
            Control::Button { .. } => {
                if value.is_some() || key.is_some() || capturing.is_some() || index.is_some() {
                    return Err(invalid());
                }
            }
        }
        if let Some(label) = label {
            text(&label, "control label")?;
            let current = match control {
                Control::Toggle { label, .. }
                | Control::Slider { label, .. }
                | Control::Keybind { label, .. }
                | Control::Choice { label, .. }
                | Control::Text { label, .. }
                | Control::Button { label, .. } => label,
            };
            *current = label;
        }
        Ok(())
    }
}
