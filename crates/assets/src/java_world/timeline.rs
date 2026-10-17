//! Allocation-free evaluation of periodic visual-resource keyframes.

use super::{
    Attribute, AttributeValue, JavaEnvironment,
    attributes::{Modifier, parse_value},
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub(super) struct Timeline {
    period: f64,
    tracks: Vec<Track>,
}

#[derive(Clone, Debug)]
struct Track {
    attribute: Attribute,
    modifier: Modifier,
    ease: Ease,
    keys: Vec<(f64, AttributeValue)>,
}

#[derive(Clone, Copy, Debug)]
enum Ease {
    Linear,
    Constant,
    Cubic([f64; 4]),
}

impl Timeline {
    /// Validates ordered periodic keys and retains only renderer-supported attributes.
    pub(super) fn parse(value: &Value) -> Result<Self, String> {
        let period = value["period_ticks"]
            .as_f64()
            .filter(|period| period.is_finite() && *period > 0.0 && *period <= 1.0e9)
            .ok_or("invalid Java visual timeline period")?;
        let mut tracks = Vec::new();
        for &attribute in Attribute::ALL {
            let Some(track) = value["tracks"].get(attribute.name()) else {
                continue;
            };
            let modifier = Modifier::parse(track["modifier"].as_str())?;
            let ease = match track.get("ease") {
                None => Ease::Linear,
                Some(Value::String(name)) if name == "linear" => Ease::Linear,
                Some(Value::String(name)) if name == "constant" => Ease::Constant,
                Some(value) => {
                    let points = value["cubic_bezier"]
                        .as_array()
                        .filter(|points| points.len() == 4)
                        .ok_or("unsupported Java visual easing")?;
                    let mut curve = [0.0; 4];
                    for (target, value) in curve.iter_mut().zip(points) {
                        *target = value
                            .as_f64()
                            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                            .ok_or("invalid Java visual easing point")?;
                    }
                    Ease::Cubic(curve)
                }
            };
            let keys = track["keyframes"]
                .as_array()
                .filter(|keys| !keys.is_empty() && keys.len() <= 256)
                .ok_or("invalid Java visual keyframes")?;
            let mut parsed = Vec::with_capacity(keys.len());
            for key in keys {
                let tick = key["ticks"]
                    .as_f64()
                    .filter(|tick| tick.is_finite() && *tick >= 0.0 && *tick < period)
                    .ok_or("invalid Java visual keyframe time")?;
                if parsed.last().is_some_and(|(previous, _)| *previous > tick) {
                    return Err("unordered Java visual keyframes".into());
                }
                parsed.push((tick, parse_value(&key["value"])?));
            }
            tracks.push(Track {
                attribute,
                modifier,
                ease,
                keys: parsed,
            });
        }
        Ok(Self { period, tracks })
    }

    /// Samples wrapped time without changing the server's clock or gameplay.
    pub(super) fn apply(&self, ticks: f64, environment: &mut JavaEnvironment) {
        let ticks = if ticks.is_finite() {
            ticks.rem_euclid(self.period)
        } else {
            0.0
        };
        for track in &self.tracks {
            let upper = track.keys.partition_point(|(tick, _)| *tick <= ticks);
            let (previous, next, sample) = if upper == 0 {
                let (tick, value) = track.keys[track.keys.len() - 1];
                ((tick - self.period, value), track.keys[0], ticks)
            } else if upper == track.keys.len() {
                let (tick, value) = track.keys[0];
                (track.keys[upper - 1], (tick + self.period, value), ticks)
            } else {
                (track.keys[upper - 1], track.keys[upper], ticks)
            };
            let fraction = if next.0 > previous.0 {
                (sample - previous.0) / (next.0 - previous.0)
            } else {
                1.0
            };
            let amount = track.ease.sample(fraction.clamp(0.0, 1.0)) as f32;
            let value = previous.1.mix(next.1, amount);
            environment.set(
                track.attribute,
                track
                    .modifier
                    .apply(environment.value(track.attribute), value),
            );
        }
    }
}

impl Ease {
    /// Inverts the monotonic time coordinate before evaluating the cubic's value coordinate.
    fn sample(self, fraction: f64) -> f64 {
        match self {
            Self::Linear => fraction,
            Self::Constant => {
                if fraction >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Cubic([x1, y1, x2, y2]) => {
                let mut bounds = [0.0, 1.0];
                for _ in 0..24 {
                    let middle = (bounds[0] + bounds[1]) * 0.5;
                    if cubic(middle, x1, x2) < fraction {
                        bounds[0] = middle;
                    } else {
                        bounds[1] = middle;
                    }
                }
                cubic((bounds[0] + bounds[1]) * 0.5, y1, y2)
            }
        }
    }
}

/// Evaluates one coordinate of a unit-endpoint cubic curve.
fn cubic(t: f64, first: f64, second: f64) -> f64 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * first + 3.0 * inverse * t * t * second + t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repeated_time_keys_preserve_angle_discontinuities_and_wrap_in_both_directions() {
        let timeline = Timeline::parse(&json!({"period_ticks": 100,
            "tracks": {"minecraft:visual/sun_angle": {"keyframes": [
                {"ticks": 25, "value": 360}, {"ticks": 25, "value": 0}
            ]}}
        }))
        .unwrap();
        for (ticks, expected) in [(25.0, 0.0), (75.0, 180.0), (-25.0, 180.0), (125.0, 0.0)] {
            let mut environment = JavaEnvironment::default();
            timeline.apply(ticks, &mut environment);
            assert_eq!(environment.value(Attribute::SunAngle).scalar(), expected);
        }
    }

    #[test]
    fn malformed_and_unordered_keys_are_rejected() {
        assert!(Timeline::parse(&json!({"period_ticks": 0})).is_err());
        assert!(
            Timeline::parse(&json!({"period_ticks": 100,
                "tracks": {"minecraft:visual/star_brightness": {"keyframes": [
                    {"ticks": 30, "value": 1}, {"ticks": 20, "value": 0}
                ]}}
            }))
            .is_err()
        );
    }
}
