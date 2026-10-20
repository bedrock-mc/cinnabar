//! Opt-in normalized placement of an authored control within its parent's available travel.

use serde_json::Value;

use crate::{ResolvedControl, layout::Rect, transform::ControlTransform};

/// Position as fractions of the space left after the control's painted bounds.
pub const PROPERTY: &str = "cinnabar_position";
/// Binding target for an authored normalized position.
pub const BINDING: &str = "#cinnabar_position";

/// Reads a finite pair, clamping each coordinate to the parent travel bounds.
pub fn parse(value: &Value) -> Option<[f64; 2]> {
    let [x, y] = value.as_array()?.as_slice() else {
        return None;
    };
    let coordinate = |value: &Value| {
        value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 1.0))
    };
    Some([coordinate(x)?, coordinate(y)?])
}

/// Places painted bounds on the requested fraction while retaining the layout size.
/// Scale, rotation and transform origin are included so an edge position remains visible.
pub(crate) fn place(control: &ResolvedControl, parent: Rect, size: [f64; 2]) -> Option<Rect> {
    let position = parse(control.properties.get(PROPERTY)?)?;
    let own = Rect::new(0.0, 0.0, size[0], size[1]);
    let bounds = control
        .properties
        .get(crate::transform::TRANSFORM_PROPERTY)
        .and_then(ControlTransform::parse)
        .map_or(own, |transform| transform.about(own).bounds(own));
    Some(Rect::new(
        parent.x + position[0] * (parent.w - bounds.w).max(0.0) - bounds.x,
        parent.y + position[1] * (parent.h - bounds.h).max(0.0) - bounds.y,
        size[0],
        size[1],
    ))
}
