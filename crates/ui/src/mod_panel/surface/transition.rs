//! Declared easing for numeric surface bindings, sampled by the host at its own clock.

use json_ui::Easing;
use serde::{Deserialize, Serialize};

use super::{SurfaceValue, bounded};

/// Longest declared transition, keeping retained motion short-lived.
pub const MAX_TRANSITION_SECONDS: f64 = 5.0;
/// Bound on cubic-bezier control-point heights, which may overshoot `0..=1`.
const MAX_BEZIER_HEIGHT: f64 = 4.0;

/// Eases a numeric binding toward each new value instead of replacing it at once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTransition {
    /// Seconds to reach a new value; zero changes it at once.
    pub duration: f64,
    #[serde(default)]
    pub easing: TransitionEasing,
    /// The value shown when the binding first appears, so it eases in when the surface opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<SurfaceValue>,
    /// The value the binding eases to when the panel closes; the surface keeps painting,
    /// without input, until every exit settles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<SurfaceValue>,
}

impl SurfaceTransition {
    /// Rejects unbounded durations, curves and non-numeric starting or exit values.
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=MAX_TRANSITION_SECONDS).contains(&self.duration) {
            return Err("surface transition duration exceeds its bounds".into());
        }
        self.easing.validate()?;
        let unbounded = |value: &Option<SurfaceValue>| {
            value.as_ref().is_some_and(|value| {
                value
                    .numbers()
                    .is_none_or(|numbers| !numbers.iter().all(bounded))
            })
        };
        if unbounded(&self.from) {
            return Err("surface transition start must be a bounded number or array".into());
        }
        if unbounded(&self.exit) {
            return Err("surface transition exit must be a bounded number or array".into());
        }
        Ok(())
    }
}

/// A vanilla JSON-UI easing name, or CSS-style cubic-bezier control points `[x1, y1, x2, y2]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EasingSource", into = "EasingSource")]
pub enum TransitionEasing {
    Named(Easing),
    CubicBezier([f64; 4]),
}

impl Default for TransitionEasing {
    fn default() -> Self {
        Self::Named(Easing::Linear)
    }
}

impl TransitionEasing {
    /// Keeps the curve a function of time: both x coordinates within `0..=1`, heights bounded.
    pub fn validate(self) -> Result<(), String> {
        if let Self::CubicBezier([x1, y1, x2, y2]) = self {
            let x = |x: f64| (0.0..=1.0).contains(&x);
            let y = |y: f64| y.is_finite() && y.abs() <= MAX_BEZIER_HEIGHT;
            if !(x(x1) && x(x2) && y(y1) && y(y2)) {
                return Err("surface transition cubic-bezier points exceed their bounds".into());
            }
        }
        Ok(())
    }

    /// Eased progress at normalized time `t`: exactly 0 at the start and 1 at the end.
    pub fn progress(self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        match self {
            Self::Named(easing) => f64::from(easing.apply(0.0, 1.0, t as f32)),
            Self::CubicBezier([x1, y1, x2, y2]) => bezier(y1, y2, bezier_parameter(x1, x2, t)),
        }
    }
}

/// One coordinate of a unit cubic bezier from 0 to 1 with inner control values `p1` and `p2`.
fn bezier(p1: f64, p2: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
}

/// The curve parameter whose x coordinate is `x`; x is monotonic because `x1, x2` lie in `0..=1`.
fn bezier_parameter(x1: f64, x2: f64, x: f64) -> f64 {
    const EPSILON: f64 = 1e-9;
    let mut s = x;
    for _ in 0..8 {
        let error = bezier(x1, x2, s) - x;
        if error.abs() < EPSILON {
            return s;
        }
        let u = 1.0 - s;
        let slope = 3.0 * u * u * x1 + 6.0 * u * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        if slope.abs() < 1e-6 {
            break;
        }
        s -= error / slope;
    }
    let (mut low, mut high) = (0.0, 1.0);
    s = x;
    for _ in 0..64 {
        let error = bezier(x1, x2, s) - x;
        if error.abs() < EPSILON {
            break;
        }
        if error < 0.0 {
            low = s;
        } else {
            high = s;
        }
        s = (low + high) / 2.0;
    }
    s
}

/// The authored form: a curve name or four control-point numbers.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum EasingSource {
    Name(String),
    Points([f64; 4]),
}

impl TryFrom<EasingSource> for TransitionEasing {
    type Error = String;

    fn try_from(source: EasingSource) -> Result<Self, String> {
        let easing = match source {
            EasingSource::Name(name) => Self::Named(
                Easing::named(&name).ok_or("surface transition easing is not a known curve")?,
            ),
            EasingSource::Points(points) => Self::CubicBezier(points),
        };
        easing.validate()?;
        Ok(easing)
    }
}

impl From<TransitionEasing> for EasingSource {
    fn from(easing: TransitionEasing) -> Self {
        match easing {
            TransitionEasing::Named(easing) => Self::Name(easing.name().into()),
            TransitionEasing::CubicBezier(points) => Self::Points(points),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a transition the way a guest authors it.
    fn parse(json: serde_json::Value) -> Result<SurfaceTransition, String> {
        let transition: SurfaceTransition =
            serde_json::from_value(json).map_err(|error| error.to_string())?;
        transition.validate()?;
        Ok(transition)
    }

    #[test]
    fn transitions_accept_named_curves_bezier_points_and_numeric_starts_and_exits() {
        let named = parse(serde_json::json!({"duration": 0.2, "easing": "Out_Cubic"})).unwrap();
        assert_eq!(named.easing, TransitionEasing::Named(Easing::OutCubic));
        let bezier = parse(serde_json::json!({
            "duration": 0.25, "easing": [0.16, 1, 0.3, 1], "from": [0, 6], "exit": [0, -6]
        }))
        .unwrap();
        assert_eq!(bezier.exit, Some(SurfaceValue::Vector(vec![0.0, -6.0])));
        assert_eq!(
            bezier.easing,
            TransitionEasing::CubicBezier([0.16, 1.0, 0.3, 1.0])
        );
        let plain = parse(serde_json::json!({"duration": 0})).unwrap();
        assert_eq!(plain.easing, TransitionEasing::default());
        assert_eq!(
            serde_json::to_value(&named).unwrap(),
            serde_json::json!({"duration": 0.2, "easing": "out_cubic"})
        );
    }

    #[test]
    fn transitions_reject_unbounded_or_unknown_values() {
        for json in [
            serde_json::json!({"duration": -0.1}),
            serde_json::json!({"duration": 6}),
            serde_json::json!({"duration": 0.2, "easing": "step"}),
            serde_json::json!({"duration": 0.2, "easing": [1.2, 0, 0.5, 1]}),
            serde_json::json!({"duration": 0.2, "easing": [0.5, 9, 0.5, 1]}),
            serde_json::json!({"duration": 0.2, "from": "hidden"}),
            serde_json::json!({"duration": 0.2, "from": [1]}),
            serde_json::json!({"duration": 0.2, "exit": true}),
            serde_json::json!({"duration": 0.2, "exit": [0, 2e6]}),
            serde_json::json!({"duration": 0.2, "delay": 1}),
        ] {
            assert!(parse(json.clone()).is_err(), "{json}");
        }
    }

    #[test]
    fn bezier_progress_matches_css_curves_and_ends_exactly() {
        let ease = TransitionEasing::CubicBezier([0.25, 0.1, 0.25, 1.0]);
        assert!((ease.progress(0.5) - 0.802_403_387_739_911_2).abs() < 1e-6);
        let linear = TransitionEasing::CubicBezier([0.0, 0.0, 1.0, 1.0]);
        for t in [0.1, 0.37, 0.9] {
            assert!((linear.progress(t) - t).abs() < 1e-9);
        }
        let overshoot = TransitionEasing::CubicBezier([0.3, 1.6, 0.6, 1.0]);
        assert!(overshoot.progress(0.6) > 1.0);
        for easing in [ease, overshoot, TransitionEasing::Named(Easing::OutExpo)] {
            assert_eq!(easing.progress(0.0), 0.0);
            assert_eq!(easing.progress(1.0), 1.0);
        }
        assert_eq!(
            TransitionEasing::Named(Easing::OutCubic).progress(0.5),
            0.875
        );
    }
}
