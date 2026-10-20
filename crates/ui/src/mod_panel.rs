//! Bounded, host-rendered controls for a granted personal extension.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub use surface::hud_card;

mod axis;
mod control;
pub use control::{Control, Event, MAX_PANEL_VALUE_CHARS, TextEvent, validate_text_value};
pub(crate) mod surface;
mod update;
pub use axis::{AxisExtent, AxisRoute};
pub use surface::{
    CollectionArt, CollectionDefinition, CollectionItem, MAX_SURFACE_NODES, MAX_TRANSITION_SECONDS,
    Surface, SurfaceCollection, SurfaceTransition, SurfaceValue, TransitionEasing, backdrop,
    preview,
};
pub use update::{
    MAX_PANEL_BUNDLE_BYTES, MAX_PANEL_PATCH_BYTES, MAX_PANEL_SURFACES, PanelBundle, PanelPatch,
};

pub const MAX_PANEL_BYTES: usize = 128 * 1024;
pub const MAX_PANEL_CONTROLS: usize = 512;
/// Input batches remain bounded independently of the retained control catalog.
pub const MAX_PANEL_EVENTS: usize = 64;
pub const MAX_PANEL_TEXT_BYTES: usize = 96;
pub const MAX_PANEL_ID_BYTES: usize = 48;
pub const MAX_PANEL_CHOICES: usize = 8;
pub const MAX_PANEL_SECTIONS: usize = 32;
pub const MAX_PANEL_CATEGORIES: usize = 4;
pub const FONT_NAME: &str = "mod_panel";
/// A shipped, smoothly sampled sans face for authored native panels.
pub const SANS_FONT_NAME: &str = "mod_panel_sans";
/// Opens the selected personal panel when its authored hotkey is unavailable on a keyboard.
pub const FALLBACK_TOGGLE_KEY: &str = "F10";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Default,
    Monochrome,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Standard,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Icon {
    #[default]
    None,
    Pointer,
    Crosshair,
    Ruler,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    /// Optional extension-authored JSON-UI; omitted panels keep native controls.
    #[serde(default)]
    pub surface: Option<Surface>,
    /// Logical authored-surface dimensions to fit within smaller viewports.
    #[serde(default)]
    pub reference_size: Option<[f32; 2]>,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub style: Style,
    pub title: String,
    pub toggle_key: String,
    pub dark: bool,
    pub controls: Vec<Control>,
    /// Lets a binding control receive Escape instead of dismissing the panel.
    #[serde(default)]
    pub capture_key: bool,
    /// Keeps the resource-pack gameplay HUD visible behind this panel.
    /// Hide HUD and death screens still apply; focused panels always hide the crosshair.
    #[serde(default)]
    pub show_gameplay_hud: bool,
    /// Opts into native hover/toggle easing and smooth discrete wheel scrolling.
    #[serde(default)]
    pub motion: bool,
    #[serde(default)]
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    #[serde(default)]
    pub icon: Icon,
    pub id: String,
    pub label: String,
    pub category: String,
    #[serde(default)]
    pub toggle: Option<String>,
    pub controls: Vec<String>,
}

impl Panel {
    /// Accepts the authored binding or the native keyboard-independent fallback.
    pub fn is_toggle_key(&self, key: &str) -> bool {
        key == self.toggle_key || key == FALLBACK_TOGGLE_KEY
    }
    /// Validates before retaining any guest-controlled text or input geometry.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_inner(true)
    }

    /// Checks changing values after the caller proves the schema and document are unchanged.
    pub fn validate_values(&self) -> Result<(), String> {
        self.validate_inner(false)
    }

    /// Keeps static action checks separate from bounded value-only updates.
    fn validate_inner(&self, document: bool) -> Result<(), String> {
        text(&self.title, "title")?;
        if let Some(size) = self.reference_size
            && (self.surface.is_none()
                || !size
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0 && *value <= 1_000_000.0))
        {
            return Err(
                "panel reference size requires a surface and bounded positive dimensions".into(),
            );
        }
        if let Some(surface) = &self.surface {
            if document {
                surface.validate()?;
            } else {
                surface.validate_bindings()?;
            }
            for action in if document {
                surface.actions()?
            } else {
                Vec::new()
            } {
                if action == "mod.close" {
                    continue;
                }
                let valid = if let Some(index) = action.strip_prefix("mod.control:") {
                    index
                        .parse::<usize>()
                        .ok()
                        .is_some_and(|index| index < self.controls.len())
                } else if let Some(axis) = AxisRoute::parse(&action) {
                    matches!(
                        self.controls.get(axis.control),
                        Some(Control::Slider { .. })
                    ) && axis
                        .extent
                        .is_none_or(|extent| extent.resolve(&surface.bindings).is_some())
                } else if let Some(indices) = action
                    .strip_prefix("mod.vector:")
                    .or_else(|| action.strip_prefix("mod.drag:"))
                {
                    indices.split_once(':').is_some_and(|(x, y)| {
                        let index = |s: &str| {
                            s.parse::<usize>().ok().filter(|i| {
                                matches!(self.controls.get(*i), Some(Control::Slider { .. }))
                            })
                        };
                        match (index(x), index(y)) {
                            (Some(x), Some(y)) => x != y,
                            _ => false,
                        }
                    })
                } else if let Some(index) = action.strip_prefix("mod.edit:") {
                    index.parse::<usize>().ok().is_some_and(|index| {
                        matches!(
                            self.controls.get(index),
                            Some(Control::Slider { .. } | Control::Text { .. })
                        )
                    })
                } else {
                    false
                };
                if !valid {
                    return Err("panel surface action must reference a declared control".into());
                }
            }
        }
        if self.toggle_key.is_empty()
            || self.toggle_key.len() > MAX_PANEL_ID_BYTES
            || !self
                .toggle_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("panel toggle key must be a bounded physical key name".into());
        }
        if self.controls.len() > MAX_PANEL_CONTROLS {
            return Err("panel has too many controls".into());
        }
        let mut ids = HashSet::with_capacity(self.controls.len());
        for control in &self.controls {
            let id = control.id();
            if id.is_empty()
                || id.len() > MAX_PANEL_ID_BYTES
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                || !ids.insert(id)
            {
                return Err("panel control IDs must be bounded, unique identifiers".into());
            }
            text(control.label(), "control label")?;
            match control {
                Control::Text {
                    value,
                    min_chars,
                    max_chars,
                    ..
                } => validate_text_value(value, *min_chars, *max_chars)?,
                Control::Keybind { key, .. } => {
                    if key.len() > MAX_PANEL_ID_BYTES || key.chars().any(char::is_control) {
                        return Err("panel keybind requires a bounded key label".into());
                    }
                }
                Control::Slider {
                    value,
                    min,
                    max,
                    step,
                    ..
                } => {
                    if ![value, min, max, step]
                        .iter()
                        .all(|number| number.is_finite())
                        || min >= max
                        || value < min
                        || value > max
                        || *step <= 0.0
                        || step > &(max - min)
                        || !(max - min).is_finite()
                        || ((max - min) / step) > 1_000_000.0
                    {
                        return Err(
                            "panel slider requires finite, ordered bounds and a positive step"
                                .into(),
                        );
                    }
                }
                Control::Choice { index, options, .. } => {
                    if options.is_empty()
                        || options.len() > MAX_PANEL_CHOICES
                        || *index as usize >= options.len()
                    {
                        return Err(
                            "panel choice requires bounded options and a valid index".into()
                        );
                    }
                    for option in options {
                        text(option, "choice label")?;
                    }
                }
                _ => {}
            }
        }
        self.validate_sections(&ids)
    }

    fn validate_sections(&self, ids: &HashSet<&str>) -> Result<(), String> {
        if self.sections.is_empty() {
            return Ok(());
        }
        if self.sections.len() > MAX_PANEL_SECTIONS {
            return Err("panel has too many sections".into());
        }
        let mut sections = HashSet::new();
        let mut categories = HashSet::new();
        let mut assigned = HashSet::new();
        for section in &self.sections {
            if section.id.is_empty()
                || section.id.len() > MAX_PANEL_ID_BYTES
                || !section
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                || !sections.insert(&section.id)
            {
                return Err("panel section IDs must be bounded, unique identifiers".into());
            }
            text(&section.label, "section label")?;
            text(&section.category, "category label")?;
            if section.category.len() > 24 {
                return Err("panel category label is too long".into());
            }
            categories.insert(&section.category);
            if let Some(toggle) = &section.toggle
                && !self.controls.iter().any(|control| {
                    control.id() == toggle && matches!(control, Control::Toggle { .. })
                })
            {
                return Err("panel section toggle must name a toggle control".into());
            }
            for id in section.toggle.iter().chain(&section.controls) {
                if !ids.contains(id.as_str()) || !assigned.insert(id.as_str()) {
                    return Err(
                        "panel sections must assign each existing control exactly once".into(),
                    );
                }
            }
        }
        if categories.len() > MAX_PANEL_CATEGORIES || assigned.len() != ids.len() {
            return Err(
                "panel sections require bounded categories and complete control assignments".into(),
            );
        }
        Ok(())
    }
}

fn text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_PANEL_TEXT_BYTES || value.chars().any(char::is_control)
    {
        Err(format!("panel {field} must be bounded, single-line text"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
