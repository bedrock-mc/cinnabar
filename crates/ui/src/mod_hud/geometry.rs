//! Bounded named-root geometry shared by native HUD layout editors.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::mod_panel::{SurfaceValue, surface::identifier};

/// Maximum named gameplay HUD roots requested by one extension.
pub const MAX_HUD_GEOMETRY_REQUESTS: usize = 128;

/// Names of inert input-panel roots whose last painted geometry an extension may observe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HudGeometryRequests(pub Vec<String>);

impl HudGeometryRequests {
    /// Restricts selectors to unique, bounded control instance names.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = HashSet::new();
        if self.0.len() > MAX_HUD_GEOMETRY_REQUESTS
            || self
                .0
                .iter()
                .any(|name| !identifier(name) || !seen.insert(name))
        {
            return Err("HUD geometry requests must contain unique bounded names".into());
        }
        Ok(())
    }
}

/// Geometry in native-panel GUI units, relative to the safe content area's origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudGeometry {
    /// Current transformed bounds, as x, y, width and height.
    pub rect: [f32; 4],
    /// Authored layout width and height before the root's own paint transform.
    pub size: [f32; 2],
}

impl HudGeometry {
    /// Rejects non-finite, empty or unbounded geometry before transport.
    pub fn validate(&self) -> Result<(), String> {
        for value in self.rect.into_iter().chain(self.size) {
            SurfaceValue::Number(f64::from(value)).validate()?;
        }
        if self.rect[2] <= 0.0 || self.rect[3] <= 0.0 || self.size.iter().any(|size| *size <= 0.0) {
            return Err("HUD geometry must have positive dimensions".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_requests_are_unique_bounded_names_and_serialize_as_an_array() {
        let mut requests = HudGeometryRequests(vec!["fixture".into(), "other-root".into()]);
        assert!(requests.validate().is_ok());
        assert_eq!(
            serde_json::to_value(&requests).unwrap(),
            serde_json::json!(["fixture", "other-root"])
        );
        for name in ["", "bad/name", "é", &"a".repeat(97)] {
            assert!(HudGeometryRequests(vec![name.into()]).validate().is_err());
        }
        requests.0.push("fixture".into());
        assert!(requests.validate().is_err());
        let mut requests = HudGeometryRequests(
            (0..MAX_HUD_GEOMETRY_REQUESTS)
                .map(|i| format!("root{i}"))
                .collect(),
        );
        assert!(requests.validate().is_ok());
        requests.0.push("extra".into());
        assert!(requests.validate().is_err());
    }

    #[test]
    fn geometry_rejects_nonfinite_or_empty_dimensions() {
        let valid = HudGeometry {
            rect: [-5.0, 2.0, 80.0, 27.0],
            size: [40.0, 13.5],
        };
        assert!(valid.validate().is_ok());
        for (field, value) in [(0, f32::NAN), (1, f32::INFINITY), (2, 0.0), (3, -1.0)] {
            let mut geometry = valid.clone();
            geometry.rect[field] = value;
            assert!(geometry.validate().is_err());
        }
        let mut geometry = valid;
        geometry.size[0] = 0.0;
        assert!(geometry.validate().is_err());
    }
}
