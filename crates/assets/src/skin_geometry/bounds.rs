use serde_json::Value;

const MIN_VISIBLE_DIMENSION: f64 = 0.1;
const MAX_VISIBLE_DIMENSION: f64 = 50.0;
const DEFAULT_VISIBLE_DIMENSION: f64 = 1.0;

/// A skin's authored visibility box, expressed in blocks in the actor coordinate frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinGeometryBounds {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

impl Default for SkinGeometryBounds {
    /// Preserves the existing player culling box when a skin supplies no usable bounds.
    fn default() -> Self {
        Self {
            center: [0.0, 1.0, 0.0],
            half_extents: [0.5, 1.0, 0.5],
        }
    }
}

impl SkinGeometryBounds {
    /// Reads the geometry description's native visibility dimensions and offset.
    #[must_use]
    pub fn from_description(description: &Value) -> Option<Self> {
        parse(description)
    }

    pub(crate) fn has_valid_dimensions(self) -> bool {
        let dimensions = [self.half_extents[0] * 2.0, self.half_extents[1] * 2.0];
        self.half_extents[0] == self.half_extents[2]
            && dimensions.iter().all(|dimension| {
                dimension.is_finite()
                    && f64::from(*dimension) >= MIN_VISIBLE_DIMENSION
                    && f64::from(*dimension) <= MAX_VISIBLE_DIMENSION
            })
    }

    /// Positions the box at the actor's feet using the shared conservative scale policy.
    pub fn at(self, feet: [f32; 3], scale: f32) -> ([f32; 3], [f32; 3]) {
        let scale = if scale.is_finite() {
            scale.max(1.0)
        } else {
            1.0
        };
        (
            std::array::from_fn(|axis| {
                feet[axis] + (self.center[axis] - self.half_extents[axis]) * scale
            }),
            std::array::from_fn(|axis| {
                feet[axis] + (self.center[axis] + self.half_extents[axis]) * scale
            }),
        )
    }
}

/// Reads vanilla's 0.1..50 dimensions; absent bounds retain the existing player fallback.
pub(super) fn parse(description: &Value) -> Option<SkinGeometryBounds> {
    if [
        "visible_bounds_width",
        "visible_bounds_height",
        "visible_bounds_offset",
    ]
    .iter()
    .all(|field| description.get(field).is_none())
    {
        return None;
    }
    let dimension = |key| {
        number(description.get(key), DEFAULT_VISIBLE_DIMENSION)
            .clamp(MIN_VISIBLE_DIMENSION, MAX_VISIBLE_DIMENSION) as f32
    };
    let width = dimension("visible_bounds_width");
    let height = dimension("visible_bounds_height");
    let mut center = [0.0; 3];
    if let Some(offset) = description.get("visible_bounds_offset") {
        for (axis, component) in center.iter_mut().enumerate() {
            *component = number(offset.get(axis), 0.0) as f32;
            if !component.is_finite() {
                return None;
            }
        }
    }
    center[0] = -center[0];
    let bounds = SkinGeometryBounds {
        center,
        half_extents: [width * 0.5, height * 0.5, width * 0.5],
    };
    (0..3)
        .all(|axis| {
            (center[axis] - bounds.half_extents[axis]).is_finite()
                && (center[axis] + bounds.half_extents[axis]).is_finite()
        })
        .then_some(bounds)
}

/// Matches the numeric/default conversions used by the geometry JSON reader.
fn number(value: Option<&Value>, default: f64) -> f64 {
    match value {
        None | Some(Value::Null) => default,
        Some(Value::Bool(value)) => f64::from(*value),
        Some(value) => value.as_f64().unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_larger_bounds_survive_and_default_matches_existing_player_box() {
        let bounds = parse(&serde_json::json!({"visible_bounds_width":3,"visible_bounds_height":4,"visible_bounds_offset":[0,2,0]})).unwrap();
        assert_eq!(
            bounds.at([10.0, 20.0, 30.0], 1.0),
            ([8.5, 20.0, 28.5], [11.5, 24.0, 31.5])
        );
        assert_eq!(
            SkinGeometryBounds::default().at([0.0; 3], 1.0),
            ([-0.5, 0.0, -0.5], [0.5, 2.0, 0.5])
        );
    }

    #[test]
    fn bounds_dimensions_clamp_and_missing_components_use_vanilla_defaults() {
        for width in [-1.0, 0.0] {
            let bounds = parse(
                &serde_json::json!({"visible_bounds_width":width,"visible_bounds_height":100}),
            )
            .unwrap();
            assert_eq!(
                bounds.half_extents,
                [
                    MIN_VISIBLE_DIMENSION as f32 * 0.5,
                    MAX_VISIBLE_DIMENSION as f32 * 0.5,
                    MIN_VISIBLE_DIMENSION as f32 * 0.5
                ]
            );
        }
        let bounds = parse(&serde_json::json!({"visible_bounds_height":3})).unwrap();
        assert_eq!(
            bounds.half_extents,
            [
                DEFAULT_VISIBLE_DIMENSION as f32 * 0.5,
                1.5,
                DEFAULT_VISIBLE_DIMENSION as f32 * 0.5
            ]
        );
        assert_eq!(parse(&serde_json::json!({})), None);
    }
}
