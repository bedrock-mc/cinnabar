//! Bounded, host-rendered controls for a granted personal extension.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const MAX_PANEL_BYTES: usize = 16 * 1024;
pub const MAX_PANEL_CONTROLS: usize = 64;
pub const MAX_PANEL_TEXT_BYTES: usize = 96;
pub const MAX_PANEL_ID_BYTES: usize = 48;
pub const MAX_PANEL_CHOICES: usize = 8;
pub const MAX_PANEL_SECTIONS: usize = 12;
pub const MAX_PANEL_CATEGORIES: usize = 4;
pub const FONT_NAME: &str = "mod_panel";

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Toggle {
        id: String,
        label: String,
        value: bool,
    },
    Slider {
        id: String,
        label: String,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    Button {
        id: String,
        label: String,
    },
    Keybind {
        id: String,
        label: String,
        key: String,
        #[serde(default)]
        capturing: bool,
    },
    Choice {
        id: String,
        label: String,
        index: u32,
        options: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub value: f32,
}

impl Control {
    pub fn id(&self) -> &str {
        match self {
            Self::Toggle { id, .. }
            | Self::Slider { id, .. }
            | Self::Button { id, .. }
            | Self::Keybind { id, .. }
            | Self::Choice { id, .. } => id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Toggle { label, .. }
            | Self::Slider { label, .. }
            | Self::Button { label, .. }
            | Self::Keybind { label, .. }
            | Self::Choice { label, .. } => label,
        }
    }
}

impl Panel {
    /// Validates before retaining any guest-controlled text or input geometry.
    pub fn validate(&self) -> Result<(), String> {
        text(&self.title, "title")?;
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
