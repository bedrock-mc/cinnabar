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
}

impl PanelBundle {
    /// Checks the control identity and type facts each registered action was admitted against.
    pub fn matches_schema(&self, panel: &Panel) -> bool {
        self.panel.controls.len() == panel.controls.len()
            && self
                .panel
                .controls
                .iter()
                .zip(&panel.controls)
                .all(|(original, current)| {
                    original.id() == current.id()
                        && match (original, current) {
                            (Control::Toggle { .. }, Control::Toggle { .. })
                            | (Control::Keybind { .. }, Control::Keybind { .. })
                            | (Control::Button { .. }, Control::Button { .. }) => true,
                            (
                                Control::Slider {
                                    min: a_min,
                                    max: a_max,
                                    step: a_step,
                                    ..
                                },
                                Control::Slider {
                                    min: b_min,
                                    max: b_max,
                                    step: b_step,
                                    ..
                                },
                            ) => (a_min, a_max, a_step) == (b_min, b_max, b_step),
                            (
                                Control::Choice { options: a, .. },
                                Control::Choice { options: b, .. },
                            ) => a == b,
                            _ => false,
                        }
                })
    }

    /// Parses and validates every route against the single declared control schema.
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
        let document_bytes = self
            .surfaces
            .values()
            .map(|surface| surface.document.len())
            .sum::<usize>()
            + self
                .panel
                .surface
                .as_ref()
                .map_or(0, |surface| surface.document.len());
        if document_bytes > MAX_PANEL_BUNDLE_BYTES {
            return Err("panel bundle exceeds byte limit".into());
        }
        for (id, surface) in &self.surfaces {
            if id.is_empty()
                || id.len() > 96
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
            {
                return Err("panel surface route is invalid".into());
            }
            let mut panel = self.panel.clone();
            panel.surface = Some(surface.clone());
            panel.validate()?;
        }
        Ok(())
    }
}

/// A sparse control change and optional replacement of a route's scalar bindings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPatch {
    #[serde(default)]
    controls: Vec<ControlPatch>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    bindings: Option<BTreeMap<String, SurfaceValue>>,
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
    /// Reads a bounded patch without resolving any retained screen definition.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > MAX_PANEL_PATCH_BYTES {
            return Err("panel update exceeds byte limit".into());
        }
        let patch: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if patch.controls.len() > MAX_PANEL_CONTROLS {
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
        if let Some(id) = self.surface {
            let mut route = routes
                .get(&id)
                .ok_or("panel update names an unknown surface")?
                .clone();
            if let Some(current) = &panel.surface {
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
        if let Some(capture) = self.capture_key {
            panel.capture_key = capture;
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
                | Control::Button { label, .. } => label,
            };
            *current = label;
        }
        Ok(())
    }
}
