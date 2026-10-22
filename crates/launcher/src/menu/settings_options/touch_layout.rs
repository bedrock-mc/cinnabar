//! Saved touch-control positions, expressed as fractions of available safe-screen travel.
use super::SettingsOptions;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Fixed-point resolution of persisted normalized positions.
const UNITS: f32 = 10_000.;

/// Only explicit placements are stored; an empty map retains the authored touch layout.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TouchLayout(BTreeMap<String, [u16; 2]>);

impl TouchLayout {
    /// Reads a validated position as a fraction of travel after subtracting control size.
    pub fn position(&self, name: &str) -> Option<[f32; 2]> {
        self.0
            .get(name)
            .map(|position| position.map(|v| f32::from(v) / UNITS))
    }
    /// Stores a finite, bounded position. Control identities are checked when loading settings.
    pub fn set(&mut self, name: &str, position: [f32; 2]) {
        if position
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        {
            self.0.insert(
                name.to_owned(),
                position.map(|v| (v * UNITS).round() as u16),
            );
        }
    }
    /// Removes obsolete control identities and malformed fixed-point coordinates.
    pub(super) fn validate(&mut self) {
        let document: serde_json::Value =
            serde_json::from_slice(assets::TOUCH_LAYOUT).expect("built-in touch layout parses");
        let mut names = std::collections::BTreeSet::new();
        control_names(&document, &mut names);
        self.0.retain(|name, at| {
            name != "look"
                && names.contains(name.as_str())
                && at.iter().all(|v| f32::from(*v) <= UNITS)
        });
    }
}
/// Derives identities from the authored control actions rather than maintaining a second registry.
fn control_names<'a>(
    value: &'a serde_json::Value,
    names: &mut std::collections::BTreeSet<&'a str>,
) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(name) = text
                .strip_prefix("cinnabar.touch.")
                .filter(|name| !name.is_empty())
            {
                names.insert(name);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                control_names(value, names);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                control_names(value, names);
            }
        }
        _ => {}
    }
}

impl SettingsOptions {
    /// The saved layout used by both touch rendering and input regions.
    pub fn touch_layout(&self) -> &TouchLayout {
        &self.touch_layout
    }
    /// Replaces a completed layout draft; callers persist only after Save.
    pub fn set_touch_layout(&mut self, mut layout: TouchLayout) {
        layout.validate();
        self.touch_layout = layout;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positions_round_trip_and_reject_stale_or_invalid_controls() {
        let mut options = SettingsOptions::default();
        let mut layout = TouchLayout::default();
        layout.set("jump", [0.1234, 0.75]);
        layout.set("look", [0.5, 0.5]);
        layout.set("old-control", [0.5, 0.5]);
        layout.set("chat", [f32::NAN, 0.]);
        options.set_touch_layout(layout);
        let decoded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        assert_eq!(
            decoded.touch_layout().position("jump"),
            Some([0.1234, 0.75])
        );
        for name in ["look", "old-control", "chat"] {
            assert_eq!(decoded.touch_layout().position(name), None);
        }
        let old = SettingsOptions::decode(b"{}").unwrap();
        assert_eq!(old.touch_layout(), &TouchLayout::default());
        let corrupt = SettingsOptions::decode(br#"{"touch_layout":{"jump":[65535,4]}}"#).unwrap();
        assert_eq!(corrupt.touch_layout().position("jump"), None);
    }
}
