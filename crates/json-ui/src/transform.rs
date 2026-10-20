//! Cinnabar paint-time transforms: a control declaring [`TRANSFORM_PROPERTY`] scales and
//! rotates its own draws and every descendant's about an origin in its box, composing like
//! nested CSS transforms, without changing layout. Only catalogs that opt in with
//! [`crate::Catalog::with_paint_transforms`] keep the property, so resource packs and server
//! JSON-UI lay out and paint exactly as before.

use serde_json::Value;

use crate::layout::Rect;

/// The control property a mod surface authors: `{"scale", "rotation", "origin"}`.
pub const TRANSFORM_PROPERTY: &str = "cinnabar_transform";
/// An application-owned paint region inherited by descendants; zero clears the region.
pub const PAINT_REGION_PROPERTY: &str = "cinnabar_paint_region";
/// The transform field holding a number or an `[x, y]` scale.
pub const SCALE_FIELD: &str = "scale";
/// The transform field holding the clockwise rotation in degrees.
pub const ROTATION_FIELD: &str = "rotation";
/// The transform field holding the origin as `[x, y]` fractions of the control's box.
pub const ORIGIN_FIELD: &str = "origin";
/// Binding target replacing the scale: a number or an `[x, y]` pair.
pub const SCALE_BINDING: &str = "#cinnabar_scale";
/// Binding target replacing the clockwise rotation in degrees.
pub const ROTATION_BINDING: &str = "#cinnabar_rotation";
/// Binding target replacing the origin, as `[x, y]` fractions of the control's box.
pub const ORIGIN_BINDING: &str = "#cinnabar_origin";
/// Largest scale on either axis; zero collapses the control.
pub const MAX_SCALE: f64 = 4.0;
/// Largest rotation magnitude in degrees.
pub const MAX_ROTATION_DEGREES: f64 = 360.0;
/// Range of each origin fraction; `0.5` is the centre and values outside `0..=1` lie beyond
/// the box.
pub const ORIGIN_RANGE: std::ops::RangeInclusive<f64> = -1.0..=2.0;

/// A 2-D affine map `p -> linear * p + translation`, with `linear` column-major
/// `[a, b, c, d]`: `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub linear: [f64; 4],
    pub translation: [f64; 2],
}

impl Affine {
    /// The map that moves nothing.
    pub const IDENTITY: Self = Self {
        linear: [1.0, 0.0, 0.0, 1.0],
        translation: [0.0, 0.0],
    };

    /// `point` under this map.
    pub fn apply(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d] = self.linear;
        [
            a * x + c * y + self.translation[0],
            b * x + d * y + self.translation[1],
        ]
    }

    /// This map after `inner`: points go through `inner` first.
    pub fn then(&self, inner: &Affine) -> Affine {
        let [a, b, c, d] = self.linear;
        let [e, f, g, h] = inner.linear;
        Affine {
            linear: [a * e + c * f, b * e + d * f, a * g + c * h, b * g + d * h],
            translation: self.apply(inner.translation),
        }
    }

    /// The inverse map, or `None` when this one collapses an axis.
    pub fn inverse(&self) -> Option<Affine> {
        let [a, b, c, d] = self.linear;
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant.abs() < 1e-12 {
            return None;
        }
        let linear = [
            d / determinant,
            -b / determinant,
            -c / determinant,
            a / determinant,
        ];
        let [x, y] = self.translation;
        Some(Affine {
            linear,
            translation: [
                -(linear[0] * x + linear[2] * y),
                -(linear[1] * x + linear[3] * y),
            ],
        })
    }

    /// The axis-aligned bounds of `rect` under this map; exact without rotation.
    pub fn bounds(&self, rect: Rect) -> Rect {
        let corners = [
            [rect.x, rect.y],
            [rect.x + rect.w, rect.y],
            [rect.x, rect.y + rect.h],
            [rect.x + rect.w, rect.y + rect.h],
        ]
        .map(|corner| self.apply(corner));
        let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for corner in corners {
            for axis in 0..2 {
                min[axis] = min[axis].min(corner[axis]);
                max[axis] = max[axis].max(corner[axis]);
            }
        }
        Rect::new(min[0], min[1], max[0] - min[0], max[1] - min[1])
    }
}

/// One control's authored transform, clamped to its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlTransform {
    pub scale: [f64; 2],
    /// Clockwise degrees.
    pub rotation: f64,
    /// Fractions of the control's box.
    pub origin: [f64; 2],
}

impl Default for ControlTransform {
    fn default() -> Self {
        Self {
            scale: [1.0; 2],
            rotation: 0.0,
            origin: [0.5; 2],
        }
    }
}

impl ControlTransform {
    /// Reads an object property; missing, malformed or non-finite fields keep their defaults
    /// and finite values clamp to the documented bounds.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let mut transform = Self::default();
        if let Some(scale) = object.get(SCALE_FIELD).and_then(scale_pair) {
            transform.scale = scale;
        }
        if let Some(rotation) = object.get(ROTATION_FIELD).and_then(finite) {
            transform.rotation = rotation.clamp(-MAX_ROTATION_DEGREES, MAX_ROTATION_DEGREES);
        }
        if let Some(origin) = object.get(ORIGIN_FIELD).and_then(pair) {
            transform.origin =
                origin.map(|value| value.clamp(*ORIGIN_RANGE.start(), *ORIGIN_RANGE.end()));
        }
        Some(transform)
    }

    /// Whether the transform leaves every point in place.
    pub fn is_identity(&self) -> bool {
        self.scale == [1.0; 2] && self.rotation == 0.0
    }

    /// The map this transform applies to points of a control laid out at `rect`.
    pub fn about(&self, rect: Rect) -> Affine {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let [sx, sy] = self.scale;
        let linear = [cos * sx, sin * sx, -sin * sy, cos * sy];
        let origin = [
            rect.x + self.origin[0] * rect.w,
            rect.y + self.origin[1] * rect.h,
        ];
        let moved = Affine {
            linear,
            translation: [0.0; 2],
        }
        .apply(origin);
        Affine {
            linear,
            translation: [origin[0] - moved[0], origin[1] - moved[1]],
        }
    }
}

/// A finite JSON number.
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}

/// A finite `[x, y]` pair.
fn pair(value: &Value) -> Option<[f64; 2]> {
    match value.as_array()?.as_slice() {
        [x, y] => Some([finite(x)?, finite(y)?]),
        _ => None,
    }
}

/// A uniform number or an `[x, y]` pair, each axis clamped to `0..=MAX_SCALE`.
fn scale_pair(value: &Value) -> Option<[f64; 2]> {
    let scale = finite(value)
        .map(|uniform| [uniform; 2])
        .or_else(|| pair(value))?;
    Some(scale.map(|axis| axis.clamp(0.0, MAX_SCALE)))
}

/// Writes a bound transform target into the control's transform object; a control that does
/// not declare [`TRANSFORM_PROPERTY`] ignores these targets.
pub(crate) fn bind(target: &str, value: &Value, current: Option<&Value>) -> Option<Value> {
    let mut object = current?.as_object()?.clone();
    let field = match target {
        SCALE_BINDING => "scale",
        ROTATION_BINDING => "rotation",
        ORIGIN_BINDING => "origin",
        _ => return None,
    };
    object.insert(field.to_owned(), value.clone());
    Some(Value::Object(object))
}

/// Where a control paints and what clip its draws and hits use once transforms apply.
/// Transform and clip are `None` outside every transformed subtree, where layout coordinates and
/// clips are already screen coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintSpace {
    /// An application-owned scope carried to emitted artwork without matching control names.
    pub region: Option<std::num::NonZeroU64>,
    /// The composed map from layout to painted coordinates.
    pub transform: Option<Affine>,
    /// The screen-space clip, replacing the layout clip.
    pub clip: Option<Rect>,
}

/// The clip geometry a control contributes, as layout computes it.
pub(crate) struct ClipInputs {
    pub(crate) rect: Rect,
    /// The clip its parent hands down, in layout coordinates.
    pub(crate) parent: Rect,
    pub(crate) clips: bool,
    pub(crate) clip_offset: [f64; 2],
    pub(crate) allows: bool,
    pub(crate) screen: Rect,
}

impl PaintSpace {
    /// The control's own paint space and the one its children inherit. A transformed
    /// control's clip maps through its transform: exactly while the chain only scales,
    /// and to its axis-aligned bounds under rotation, which surface validation rejects.
    pub(crate) fn place(
        self,
        own: Option<ControlTransform>,
        inputs: ClipInputs,
    ) -> (PaintSpace, PaintSpace) {
        let local = own
            .filter(|own| !own.is_identity())
            .map(|own| own.about(inputs.rect));
        let transform = match (self.transform, local) {
            (Some(outer), Some(inner)) => Some(outer.then(&inner)),
            (outer, inner) => outer.or(inner),
        };
        if transform.is_none() && self.clip.is_none() {
            return (self, self);
        }
        let parent = self.clip.unwrap_or(inputs.parent);
        let handed = if inputs.clips {
            let rect = inputs.rect;
            let [dx, dy] = inputs.clip_offset;
            let inset = Rect::new(
                rect.x + dx,
                rect.y + dy,
                (rect.w - 2.0 * dx).max(0.0),
                (rect.h - 2.0 * dy).max(0.0),
            );
            parent.intersect(transform.map_or(inset, |transform| transform.bounds(inset)))
        } else {
            parent
        };
        let own_clip = if inputs.allows { handed } else { inputs.screen };
        (
            PaintSpace {
                region: self.region,
                transform,
                clip: Some(own_clip),
            },
            PaintSpace {
                region: self.region,
                transform,
                clip: Some(handed),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn close(left: [f64; 2], right: [f64; 2]) -> bool {
        (left[0] - right[0]).abs() < 1e-9 && (left[1] - right[1]).abs() < 1e-9
    }

    #[test]
    fn transforms_scale_and_rotate_about_their_origin() {
        let rect = Rect::new(10.0, 20.0, 40.0, 20.0);
        let scaled = ControlTransform::parse(&json!({"scale": 2}))
            .unwrap()
            .about(rect);
        assert!(close(scaled.apply([30.0, 30.0]), [30.0, 30.0]));
        assert!(close(scaled.apply([10.0, 20.0]), [-10.0, 10.0]));
        let corner = ControlTransform::parse(&json!({"scale": [0.5, 2], "origin": [0, 0]}))
            .unwrap()
            .about(rect);
        assert!(close(corner.apply([50.0, 40.0]), [30.0, 60.0]));
        // A clockwise quarter turn sends the right-hand edge's midpoint to the bottom.
        let turned = ControlTransform::parse(&json!({"rotation": 90}))
            .unwrap()
            .about(rect);
        assert!(close(turned.apply([50.0, 30.0]), [30.0, 50.0]));
    }

    #[test]
    fn transforms_clamp_authored_values_and_skip_malformed_ones() {
        let parsed =
            ControlTransform::parse(&json!({"scale": 9, "rotation": -720, "origin": [3, -2]}))
                .unwrap();
        assert_eq!(parsed.scale, [MAX_SCALE; 2]);
        assert_eq!(parsed.rotation, -MAX_ROTATION_DEGREES);
        assert_eq!(parsed.origin, [2.0, -1.0]);
        let malformed = ControlTransform::parse(&json!({"scale": "big", "origin": [1]})).unwrap();
        assert_eq!(malformed, ControlTransform::default());
        assert!(malformed.is_identity());
        assert_eq!(ControlTransform::parse(&json!(2)), None);
    }

    #[test]
    fn nested_transforms_compose_like_css_and_invert() {
        let outer = ControlTransform::parse(&json!({"scale": 2, "origin": [0, 0]}))
            .unwrap()
            .about(Rect::new(0.0, 0.0, 100.0, 100.0));
        let inner = ControlTransform::parse(&json!({"rotation": 90}))
            .unwrap()
            .about(Rect::new(10.0, 10.0, 20.0, 20.0));
        let world = outer.then(&inner);
        // The inner box's right midpoint turns to its bottom, then doubles from the corner.
        assert!(close(world.apply([30.0, 20.0]), [40.0, 60.0]));
        let inverse = world.inverse().unwrap();
        assert!(close(inverse.apply([40.0, 60.0]), [30.0, 20.0]));
        let collapsed = ControlTransform::parse(&json!({"scale": [0, 1]}))
            .unwrap()
            .about(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(collapsed.inverse(), None);
    }

    #[test]
    fn untransformed_subtrees_keep_layout_clips() {
        let inputs = || ClipInputs {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            parent: Rect::new(0.0, 0.0, 400.0, 300.0),
            clips: true,
            clip_offset: [0.0; 2],
            allows: true,
            screen: Rect::new(0.0, 0.0, 400.0, 300.0),
        };
        let (own, children) = PaintSpace::default().place(None, inputs());
        assert_eq!(
            (own, children),
            (PaintSpace::default(), PaintSpace::default())
        );
        let identity = ControlTransform::default();
        assert_eq!(
            PaintSpace::default().place(Some(identity), inputs()).1,
            PaintSpace::default()
        );
        let half = ControlTransform {
            scale: [0.5; 2],
            origin: [0.0; 2],
            ..ControlTransform::default()
        };
        let (_, children) = PaintSpace::default().place(Some(half), inputs());
        assert_eq!(children.clip, Some(Rect::new(0.0, 0.0, 50.0, 25.0)));
    }
}
