//! Native control value types and their declared editing bounds.

use serde::{Deserialize, Serialize};

/// Single-line editable values are bounded separately from short control labels.
pub const MAX_PANEL_VALUE_CHARS: usize = 256;

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
    Text {
        id: String,
        label: String,
        value: String,
        #[serde(default)]
        min_chars: usize,
        #[serde(default = "text_capacity")]
        max_chars: usize,
        #[serde(default)]
        live: bool,
        #[serde(default)]
        navigation_keys: bool,
    },
}

/// Supplies the default bounded capacity of a plain single-line field.
fn text_capacity() -> usize {
    MAX_PANEL_VALUE_CHARS
}

/// Checks both immutable field bounds and a complete Unicode text value.
pub fn validate_text_value(value: &str, min: usize, max: usize) -> Result<(), String> {
    let length = value.chars().count();
    if max == 0
        || max > MAX_PANEL_VALUE_CHARS
        || min > max
        || length < min
        || length > max
        || value.chars().any(char::is_control)
    {
        return Err("panel text value exceeds its single-line character bounds".into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub value: f32,
}

/// A committed native text edit carries its actual UTF-8 value independently of numeric events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEvent {
    pub id: String,
    pub value: String,
}

impl Control {
    pub fn id(&self) -> &str {
        match self {
            Self::Toggle { id, .. }
            | Self::Slider { id, .. }
            | Self::Button { id, .. }
            | Self::Keybind { id, .. }
            | Self::Choice { id, .. } => id,
            Self::Text { id, .. } => id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Toggle { label, .. }
            | Self::Slider { label, .. }
            | Self::Button { label, .. }
            | Self::Keybind { label, .. }
            | Self::Choice { label, .. } => label,
            Self::Text { label, .. } => label,
        }
    }
}

/// Modifier state sampled with a focused field key, before later releases in the same frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// An opted-in navigation key belongs to its focused field and retains its modifiers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldKey {
    pub id: String,
    pub key: String,
    pub modifiers: FieldModifiers,
}

/// Additive field input distinguishes live changes, submissions and guest navigation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FieldEvent {
    Change(TextEvent),
    Submit(TextEvent),
    Key(FieldKey),
}

/// Navigation keys shared by field routing and host event admission.
pub const FIELD_NAVIGATION_KEYS: &[&str] = &[
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Enter",
    "NumpadEnter",
];

/// Admits the navigation keys a focused native field may forward to its guest.
pub fn is_field_navigation_key(key: &str) -> bool {
    FIELD_NAVIGATION_KEYS.contains(&key)
}
