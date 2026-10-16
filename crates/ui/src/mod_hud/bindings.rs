//! Bounded extension globals that resource-pack gameplay HUD controls may consume.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::mod_panel::{
    SurfaceValue,
    surface::{MAX_BINDING_NAME_BYTES, binding_name},
};

/// Maximum globals retained for one extension's gameplay HUD.
pub const MAX_HUD_BINDINGS: usize = 128;

/// Numeric and visibility globals scoped away from the native HUD controller's bindings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HudBindings(pub BTreeMap<String, SurfaceValue>);

impl HudBindings {
    /// Accepts bounded values only under the reserved extension HUD namespace.
    pub fn validate(&self) -> Result<(), String> {
        if self.0.len() > MAX_HUD_BINDINGS {
            return Err("HUD has too many extension bindings".into());
        }
        for (name, value) in &self.0 {
            if name.len() > MAX_BINDING_NAME_BYTES
                || !name
                    .strip_prefix("#mod_")
                    .is_some_and(|suffix| !suffix.is_empty())
                || !binding_name(name)
            {
                return Err("extension HUD binding name is invalid".into());
            }
            if matches!(value, SurfaceValue::Text(_)) {
                return Err("extension HUD bindings cannot contain text".into());
            }
            value.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validates one value under a conforming extension HUD name.
    fn accepts(value: SurfaceValue) -> bool {
        HudBindings(BTreeMap::from([("#mod_example".into(), value)]))
            .validate()
            .is_ok()
    }

    #[test]
    fn hud_bindings_are_a_transparent_bounded_scalar_map() {
        let bindings: HudBindings = serde_json::from_str(
            r##"{"#mod_visible":true,"#mod_scale":1.25,"#mod_position":[0.25,0.75]}"##,
        )
        .unwrap();
        assert!(bindings.validate().is_ok());
        assert_eq!(
            serde_json::to_value(&bindings).unwrap()["#mod_position"],
            serde_json::json!([0.25, 0.75])
        );
        for value in [
            SurfaceValue::Number(-1_000_000.0),
            SurfaceValue::Bool(false),
            SurfaceValue::Vector(vec![1.0, 2.0, 3.0, 4.0]),
        ] {
            assert!(accepts(value));
        }
        for value in [
            SurfaceValue::Number(f64::NAN),
            SurfaceValue::Number(f64::INFINITY),
            SurfaceValue::Number(1_000_001.0),
            SurfaceValue::Text("text".into()),
            SurfaceValue::Vector(vec![]),
            SurfaceValue::Vector(vec![0.0; 5]),
            SurfaceValue::Vector(vec![0.0, f64::NAN]),
        ] {
            assert!(!accepts(value));
        }
    }

    #[test]
    fn hud_bindings_cannot_shadow_native_globals_or_exceed_name_and_count_bounds() {
        for name in [
            "#title_text",
            "mod_visible",
            "#mod_",
            "#mod_bad name",
            "#mod_é",
            &format!("#mod_{}", "a".repeat(MAX_BINDING_NAME_BYTES)),
        ] {
            assert!(
                HudBindings(BTreeMap::from([(name.into(), SurfaceValue::Bool(true))]))
                    .validate()
                    .is_err(),
                "{name}"
            );
        }
        let mut bindings = HudBindings(
            (0..MAX_HUD_BINDINGS)
                .map(|i| (format!("#mod_{i}"), SurfaceValue::Number(0.0)))
                .collect(),
        );
        assert!(bindings.validate().is_ok());
        bindings
            .0
            .insert("#mod_extra".into(), SurfaceValue::Bool(true));
        assert!(bindings.validate().is_err());
    }
}
