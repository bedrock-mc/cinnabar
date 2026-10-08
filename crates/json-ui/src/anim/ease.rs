//! The 32 easing curves of 1.26.50's easing table, in the
//! client's single-precision arithmetic and 65536-entry sine table.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// An easing curve, in the vanilla client's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    Spring,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InQuint,
    OutQuint,
    InOutQuint,
    InSine,
    OutSine,
    InOutSine,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    InBounce,
    OutBounce,
    InOutBounce,
    InBack,
    OutBack,
    InOutBack,
    InElastic,
    OutElastic,
    InOutElastic,
}

const NAMES: [&str; 32] = [
    "linear",
    "spring",
    "in_quad",
    "out_quad",
    "in_out_quad",
    "in_cubic",
    "out_cubic",
    "in_out_cubic",
    "in_quart",
    "out_quart",
    "in_out_quart",
    "in_quint",
    "out_quint",
    "in_out_quint",
    "in_sine",
    "out_sine",
    "in_out_sine",
    "in_expo",
    "out_expo",
    "in_out_expo",
    "in_circ",
    "out_circ",
    "in_out_circ",
    "in_bounce",
    "out_bounce",
    "in_out_bounce",
    "in_back",
    "out_back",
    "in_out_back",
    "in_elastic",
    "out_elastic",
    "in_out_elastic",
];

/// Sine table lookup scale: 65536 entries over one turn.
const SIN_SCALE: f32 = 10430.378;
const SIN_QUARTER: f32 = 16384.0;
const TAU: f32 = 6.2831855;
const PI: f32 = std::f32::consts::PI;
const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;
const BACK: f32 = 1.70158;
const BACK_PLUS_ONE: f32 = 2.70158;
const BACK_IN_OUT: f32 = 2.5949094;
const BACK_IN_OUT_PLUS_ONE: f32 = 3.5949094;

impl Easing {
    /// The curve a (case-insensitive) name selects; unknown names are linear.
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        NAMES
            .iter()
            .position(|known| *known == lower)
            .map_or(Easing::Linear, |index| ALL[index])
    }

    /// `from` eased toward `to` at normalized time `t`.
    pub fn apply(self, from: f32, to: f32, t: f32) -> f32 {
        let d = to - from;
        match self {
            Easing::Linear => from + d * t,
            Easing::Spring => {
                let t = t.clamp(0.0, 1.0);
                let phase = (2.5 * t * t * t + 0.2) * PI * t * SIN_SCALE;
                let rest = 1.0 - t;
                (rest.powf(2.2) * sin_at(phase) + t) * (rest * 1.2 + 1.0) * d + from
            }
            Easing::InQuad => from + d * t * t,
            Easing::OutQuad => from - (t + -2.0) * d * t,
            Easing::InOutQuad => {
                let x = t + t;
                if x < 1.0 {
                    d * 0.5 * x * x + from
                } else {
                    d * -0.5 * ((-2.0 + x + -1.0) * (x + -1.0) + -1.0) + from
                }
            }
            Easing::InCubic => from + d * t * t * t,
            Easing::OutCubic => {
                let x = t + -1.0;
                from + (x * x * x + 1.0) * d
            }
            Easing::InOutCubic => in_out_power(from, d, t, 3),
            Easing::InQuart => from + d * t * t * t * t,
            Easing::OutQuart => {
                let x = t + -1.0;
                from - (x * x * x * x + -1.0) * d
            }
            Easing::InOutQuart => {
                let x = t + t;
                if x < 1.0 {
                    d * 0.5 * x * x * x * x + from
                } else {
                    let x = x + -2.0;
                    d * -0.5 * (x * x * x * x + -2.0) + from
                }
            }
            Easing::InQuint => from + d * t * t * t * t * t,
            Easing::OutQuint => {
                let x = t + -1.0;
                from + (x * x * x * x * x + 1.0) * d
            }
            Easing::InOutQuint => in_out_power(from, d, t, 5),
            Easing::InSine => from + (d - sin_at(t * HALF_PI * SIN_SCALE + SIN_QUARTER) * d),
            Easing::OutSine => from + d * sin_at(t * HALF_PI * SIN_SCALE),
            Easing::InOutSine => {
                from + (sin_at(t * PI * SIN_SCALE + SIN_QUARTER) + -1.0) * d * -0.5
            }
            Easing::InExpo => ((t + -1.0) * 10.0).exp2() * d + from,
            Easing::OutExpo => (1.0 - (t * -10.0).exp2()) * d + from,
            Easing::InOutExpo => {
                let x = t + t;
                let curve = if 1.0 <= x {
                    2.0 - ((x + -1.0) * -10.0).exp2()
                } else {
                    ((x + -1.0) * 10.0).exp2()
                };
                curve * (d * 0.5) + from
            }
            Easing::InCirc => from - ((1.0 - t * t).sqrt() + -1.0) * d,
            Easing::OutCirc => from + (1.0 - (t + -1.0) * (t + -1.0)).sqrt() * d,
            Easing::InOutCirc => {
                let x = t + t;
                let (square, offset, scale) = if 1.0 <= x {
                    ((x + -2.0) * (x + -2.0), 1.0, 0.5)
                } else {
                    (x * x, -1.0, -0.5)
                };
                ((1.0 - square).sqrt() + offset) * d * scale + from
            }
            Easing::InBounce => (d - (bounce(1.0 - t) * d + 0.0)) + from,
            Easing::OutBounce => d * bounce(t) + from,
            Easing::InOutBounce => {
                if t < 0.5 {
                    ((d - (bounce(1.0 - (t + t)) * d + 0.0)) + 0.0) * 0.5 + from
                } else {
                    d * 0.5 + (bounce(t + t + -1.0) * d + 0.0) * 0.5 + from
                }
            }
            Easing::InBack => from + (t * BACK_PLUS_ONE + -BACK) * d * t * t,
            Easing::OutBack => {
                let x = t + -1.0;
                from + (x * x * (x * BACK_PLUS_ONE + BACK) + 1.0) * d
            }
            Easing::InOutBack => {
                let x = t + t;
                let curve = if 1.0 <= x {
                    let x = x + -2.0;
                    (x * BACK_IN_OUT_PLUS_ONE + BACK_IN_OUT) * x * x + 2.0
                } else {
                    (x * BACK_IN_OUT_PLUS_ONE + -BACK_IN_OUT) * x * x
                };
                curve * d * 0.5 + from
            }
            Easing::InElastic => {
                if t == 0.0 {
                    return from;
                }
                if t == 1.0 {
                    return from + d;
                }
                let x = t + -1.0;
                from - (10.0 * x).exp2() * d * elastic_sin(x)
            }
            Easing::OutElastic => {
                if t == 0.0 {
                    return from;
                }
                if t == 1.0 {
                    return from + d;
                }
                from + (-10.0 * t).exp2() * d * elastic_sin(t) + d
            }
            Easing::InOutElastic => {
                if t == 0.0 {
                    return from;
                }
                let x = t + t;
                if x == 2.0 {
                    return from + d;
                }
                let x = x + -1.0;
                let wave = elastic_sin(x);
                if 1.0 <= x + 1.0 {
                    from + (x * -10.0).exp2() * d * wave * 0.5 + d
                } else {
                    from + d * (x * 10.0).exp2() * wave * -0.5
                }
            }
        }
    }
}

const ALL: [Easing; 32] = [
    Easing::Linear,
    Easing::Spring,
    Easing::InQuad,
    Easing::OutQuad,
    Easing::InOutQuad,
    Easing::InCubic,
    Easing::OutCubic,
    Easing::InOutCubic,
    Easing::InQuart,
    Easing::OutQuart,
    Easing::InOutQuart,
    Easing::InQuint,
    Easing::OutQuint,
    Easing::InOutQuint,
    Easing::InSine,
    Easing::OutSine,
    Easing::InOutSine,
    Easing::InExpo,
    Easing::OutExpo,
    Easing::InOutExpo,
    Easing::InCirc,
    Easing::OutCirc,
    Easing::InOutCirc,
    Easing::InBounce,
    Easing::OutBounce,
    Easing::InOutBounce,
    Easing::InBack,
    Easing::OutBack,
    Easing::InOutBack,
    Easing::InElastic,
    Easing::OutElastic,
    Easing::InOutElastic,
];

/// The cubic/quintic in-out shape, whose second half adds 2 as the client does.
fn in_out_power(from: f32, d: f32, t: f32, power: i32) -> f32 {
    let x = t + t;
    let pow = |x: f32| (0..power).fold(1.0f32, |product, _| product * x);
    if x < 1.0 {
        d * 0.5 * pow(x) + from
    } else {
        d * 0.5 * (pow(x + -2.0) + 2.0) + from
    }
}

/// The piecewise bounce curve over `x` in `0..=1`.
fn bounce(x: f32) -> f32 {
    const K: f32 = 7.5625;
    if x < 0.36363637 {
        K * x * x
    } else if x < 0.72727275 {
        let x = x + -0.54545456;
        K * x * x + 0.75
    } else if x < 0.90909094 {
        let x = x + -0.8181818;
        K * x * x + 0.9375
    } else {
        let x = x + -0.95454544;
        K * x * x + 0.984375
    }
}

fn elastic_sin(x: f32) -> f32 {
    sin_at((((x + -0.075) * TAU) / 0.3) * SIN_SCALE)
}

/// The client's sine table indexed by a pre-scaled angle.
fn sin_at(scaled: f32) -> f32 {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..65536)
            .map(|index| (f64::from(index) * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
            .collect()
    });
    table[(scaled as i32 & 0xffff) as usize]
}

#[cfg(test)]
mod tests {
    use super::Easing;

    // Names map case-insensitively to the table; `step` is not a vanilla curve.
    #[test]
    fn names_follow_the_client_table() {
        assert_eq!(Easing::from_name("OUT_BOUNCE"), Easing::OutBounce);
        assert_eq!(Easing::from_name("spring"), Easing::Spring);
        assert_eq!(Easing::from_name("step"), Easing::Linear);
    }

    // Single-precision evaluation matches the client's sampled values.
    #[test]
    fn curves_match_native_float_samples() {
        assert_eq!(Easing::InQuad.apply(0.0, 1.0, 0.1), 0.010000001);
        assert!((Easing::InOutBack.apply(0.5, 1.0, 0.25) - 0.45015908).abs() < 1e-6);
        assert_eq!(Easing::InExpo.apply(0.0, 1.0, 0.0), 1.0 / 1024.0);
        assert_eq!(Easing::OutExpo.apply(0.0, 1.0, 1.0), 1.0 - 1.0 / 1024.0);
        assert_eq!(Easing::InOutExpo.apply(0.0, 1.0, 0.0), 1.0 / 2048.0);
        assert!((Easing::OutBounce.apply(0.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((Easing::InBounce.apply(0.0, 1.0, 0.5) - 0.234375).abs() < 1e-6);
        assert_eq!(Easing::InElastic.apply(0.0, 1.0, 1.0), 1.0);
    }

    // Spring overshoots its target before settling on it.
    #[test]
    fn spring_is_its_own_curve() {
        let mid = Easing::Spring.apply(0.0, 1.0, 0.5);
        assert!(mid != 0.5);
        assert!((Easing::Spring.apply(0.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
