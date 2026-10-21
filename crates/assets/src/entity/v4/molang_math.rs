use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

/// Every `math.*` function the vanilla client registers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MolangFunction {
    Abs,
    Acos,
    Asin,
    Atan,
    Atan2,
    Ceil,
    Clamp,
    CopySign,
    Cos,
    DieRoll,
    DieRollInteger,
    Exp,
    Floor,
    HermiteBlend,
    InverseLerp,
    Lerp,
    LerpRotate,
    Ln,
    Max,
    Min,
    MinAngle,
    Mod,
    Pow,
    Random,
    RandomInteger,
    Round,
    Sign,
    Sin,
    Sqrt,
    Trunc,
    Ease(MolangEaseCurve, MolangEaseMode),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MolangEaseCurve {
    Quad,
    Cubic,
    Quart,
    Quint,
    Sine,
    Expo,
    Circ,
    Bounce,
    Back,
    Elastic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MolangEaseMode {
    In,
    Out,
    InOut,
}

impl MolangFunction {
    /// Resolves a lowercased `math.*` name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("math.")?;
        if let Some(rest) = name.strip_prefix("ease_") {
            let (mode, curve) = if let Some(curve) = rest.strip_prefix("in_out_") {
                (MolangEaseMode::InOut, curve)
            } else if let Some(curve) = rest.strip_prefix("in_") {
                (MolangEaseMode::In, curve)
            } else {
                (MolangEaseMode::Out, rest.strip_prefix("out_")?)
            };
            let curve = match curve {
                "quad" => MolangEaseCurve::Quad,
                "cubic" => MolangEaseCurve::Cubic,
                "quart" => MolangEaseCurve::Quart,
                "quint" => MolangEaseCurve::Quint,
                "sine" => MolangEaseCurve::Sine,
                "expo" => MolangEaseCurve::Expo,
                "circ" => MolangEaseCurve::Circ,
                "bounce" => MolangEaseCurve::Bounce,
                "back" => MolangEaseCurve::Back,
                "elastic" => MolangEaseCurve::Elastic,
                _ => return None,
            };
            return Some(Self::Ease(curve, mode));
        }
        Some(match name {
            "abs" => Self::Abs,
            "acos" => Self::Acos,
            "asin" => Self::Asin,
            "atan" => Self::Atan,
            "atan2" => Self::Atan2,
            "ceil" => Self::Ceil,
            "clamp" => Self::Clamp,
            "copy_sign" => Self::CopySign,
            "cos" => Self::Cos,
            "die_roll" => Self::DieRoll,
            "die_roll_integer" => Self::DieRollInteger,
            "exp" => Self::Exp,
            "floor" => Self::Floor,
            "hermite_blend" => Self::HermiteBlend,
            "inverse_lerp" => Self::InverseLerp,
            "lerp" => Self::Lerp,
            "lerprotate" => Self::LerpRotate,
            "ln" => Self::Ln,
            "max" => Self::Max,
            "min" => Self::Min,
            "min_angle" => Self::MinAngle,
            "mod" => Self::Mod,
            "pow" => Self::Pow,
            "random" => Self::Random,
            "random_integer" => Self::RandomInteger,
            "round" => Self::Round,
            "sign" => Self::Sign,
            "sin" => Self::Sin,
            "sqrt" => Self::Sqrt,
            "trunc" => Self::Trunc,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::Atan2
            | Self::CopySign
            | Self::Max
            | Self::Min
            | Self::Mod
            | Self::Pow
            | Self::Random
            | Self::RandomInteger => 2,
            Self::Clamp
            | Self::Lerp
            | Self::LerpRotate
            | Self::InverseLerp
            | Self::DieRoll
            | Self::DieRollInteger
            | Self::Ease(..) => 3,
            _ => 1,
        }
    }

    /// Whether repeated calls with equal arguments may differ.
    #[must_use]
    pub const fn is_random(self) -> bool {
        matches!(
            self,
            Self::Random | Self::RandomInteger | Self::DieRoll | Self::DieRollInteger
        )
    }
}

/// Evaluates a `math.*` call with vanilla single-precision semantics: trigonometry takes and
/// returns degrees, `mod` by zero is zero, and NaN propagates. `random` yields values in
/// `[0, 1]` for the random family.
/// Arguments may be owned or borrowed numbers; absent arguments read as zero.
#[must_use]
pub fn molang_call(
    function: MolangFunction,
    args: impl IntoIterator<Item: Borrow<f32>>,
    random: &mut dyn FnMut() -> f32,
) -> f32 {
    let mut args = args.into_iter();
    let mut arg = || args.next().map_or(0.0, |value| *value.borrow());
    let (a, b, c) = (arg(), arg(), arg());
    match function {
        MolangFunction::Abs => a.abs(),
        MolangFunction::Acos => clamp_unit(a).acos().to_degrees(),
        MolangFunction::Asin => clamp_unit(a).asin().to_degrees(),
        MolangFunction::Atan => a.atan().to_degrees(),
        MolangFunction::Atan2 => a.atan2(b).to_degrees(),
        MolangFunction::Ceil => a.ceil(),
        MolangFunction::Clamp => {
            if a > c {
                c
            } else if b <= a {
                a
            } else {
                b
            }
        }
        MolangFunction::CopySign => a.abs().copysign(b),
        MolangFunction::Cos => a.to_radians().cos(),
        MolangFunction::DieRoll | MolangFunction::DieRollInteger => {
            let dice = a.trunc();
            if dice.is_nan() || dice <= 0.0 {
                return 0.0;
            }
            // Vanilla floors both bounds; the rolls themselves stay fractional for die_roll.
            let (low, high) = ordered(b.floor(), c.floor());
            let integer = function == MolangFunction::DieRollInteger;
            let mut sum = 0.0;
            // One call is charged one op; only `MAX_DICE` bounds the loop.
            for _ in 0..(dice.min(MAX_DICE) as u32) {
                let roll = random().clamp(0.0, 1.0);
                sum += if integer {
                    random_integer(low, high, roll)
                } else {
                    high * roll + (1.0 - roll) * low
                };
            }
            sum
        }
        MolangFunction::Exp => a.exp(),
        MolangFunction::Floor => a.floor(),
        MolangFunction::HermiteBlend => (3.0 * a) * a - ((2.0 * a) * a) * a,
        MolangFunction::InverseLerp => (c - a) / (b - a),
        MolangFunction::Lerp => (b - a) * c + a,
        MolangFunction::LerpRotate => {
            let mut delta = (b - a + 180.0) % 360.0;
            if delta < 0.0 {
                delta += 360.0;
            }
            a + (delta - 180.0) * c
        }
        MolangFunction::Ln => a.ln(),
        MolangFunction::Max => max(a, b),
        MolangFunction::Min => {
            if b <= a {
                b
            } else {
                a
            }
        }
        MolangFunction::MinAngle => {
            let mut wrapped = (a + 180.0) % 360.0;
            if wrapped < 0.0 {
                wrapped += 360.0;
            }
            wrapped - 180.0
        }
        MolangFunction::Mod => {
            if b == 0.0 {
                0.0
            } else {
                a % b
            }
        }
        MolangFunction::Pow => a.powf(b),
        MolangFunction::Random => {
            let (low, high) = ordered(a, b);
            let roll = random().clamp(0.0, 1.0);
            high * roll + (1.0 - roll) * low
        }
        MolangFunction::RandomInteger => {
            let (low, high) = ordered(a, b);
            random_integer(low, high, random().clamp(0.0, 1.0))
        }
        MolangFunction::Round => (a + ROUND_BIAS.copysign(a)).trunc(),
        MolangFunction::Sign => {
            if a >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        MolangFunction::Sin => a.to_radians().sin(),
        MolangFunction::Sqrt => a.sqrt(),
        MolangFunction::Trunc => a.trunc(),
        MolangFunction::Ease(curve, mode) => ease(curve, mode, a, b, c),
    }
}

/// Cinnabar's bound on dice rolled in one call, not a vanilla value.
const MAX_DICE: f32 = 1_024.0;
/// Rounds halves away from zero after truncation; needs independent measurement.
const ROUND_BIAS: f32 = 0.499_999_97;

fn max(a: f32, b: f32) -> f32 {
    if a <= b { b } else { a }
}

fn ordered(a: f32, b: f32) -> (f32, f32) {
    if b < a { (b, a) } else { (a, b) }
}

// Inputs just past the unit range are treated as rounding error; the tolerance needs
// independent measurement.
fn clamp_unit(value: f32) -> f32 {
    if value.abs() <= 1.0005 {
        value.clamp(-1.0, 1.0)
    } else {
        value
    }
}

/// Samples inclusive ordered bounds, propagating NaN without passing it to `clamp`.
/// The upper bound stops one ulp short; needs independent measurement.
fn random_integer(low: f32, high: f32, roll: f32) -> f32 {
    if low.is_nan() || high.is_nan() {
        return f32::NAN;
    }
    let span_high = high + 1.0 - high * f32::EPSILON;
    (span_high * roll + (1.0 - roll) * low)
        .floor()
        .clamp(low, high)
}

fn ease(curve: MolangEaseCurve, mode: MolangEaseMode, start: f32, end: f32, t: f32) -> f32 {
    let change = end - start;
    let unit = |t: f32| ease_in_unit(curve, t);
    let out = |t: f32| 1.0 - unit(1.0 - t);
    let progress = match (curve, mode) {
        (MolangEaseCurve::Elastic, _) => return elastic(mode, start, change, t),
        (_, MolangEaseMode::In) => unit(t),
        (_, MolangEaseMode::Out) => out(t),
        (MolangEaseCurve::Back, MolangEaseMode::InOut) => {
            let scaled = t * 2.0;
            if scaled < 1.0 {
                0.5 * (scaled * scaled * ((BACK_IN_OUT + 1.0) * scaled - BACK_IN_OUT))
            } else {
                let shifted = scaled - 2.0;
                0.5 * (shifted * shifted * ((BACK_IN_OUT + 1.0) * shifted + BACK_IN_OUT) + 2.0)
            }
        }
        (_, MolangEaseMode::InOut) => {
            if t < 0.5 {
                0.5 * unit(t * 2.0)
            } else {
                0.5 * out(t * 2.0 - 1.0) + 0.5
            }
        }
    };
    change * progress + start
}

const BACK_OVERSHOOT: f32 = 1.701_58;
const BACK_IN_OUT: f32 = 2.594_909_5;
const ELASTIC_PERIOD: f32 = 0.3;
const ELASTIC_SHIFT: f32 = 0.075;

fn ease_in_unit(curve: MolangEaseCurve, t: f32) -> f32 {
    match curve {
        MolangEaseCurve::Quad => t * t,
        MolangEaseCurve::Cubic => t * t * t,
        MolangEaseCurve::Quart => t * t * t * t,
        MolangEaseCurve::Quint => t * t * t * t * t,
        MolangEaseCurve::Sine => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
        MolangEaseCurve::Expo => 2.0_f32.powf(10.0 * (t - 1.0)),
        MolangEaseCurve::Circ => -((1.0 - t * t).sqrt() - 1.0),
        MolangEaseCurve::Back => t * t * ((BACK_OVERSHOOT + 1.0) * t - BACK_OVERSHOOT),
        MolangEaseCurve::Bounce => 1.0 - bounce_out(1.0 - t),
        MolangEaseCurve::Elastic => 0.0,
    }
}

fn bounce_out(t: f32) -> f32 {
    const SCALE: f32 = 7.5625;
    if t < 1.0 / 2.75 {
        SCALE * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        SCALE * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        SCALE * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        SCALE * t * t + 0.984_375
    }
}

// The elastic family's sine table is approximated by `sin`; needs independent measurement.
fn elastic(mode: MolangEaseMode, start: f32, change: f32, t: f32) -> f32 {
    let wave = |t: f32| ((t - ELASTIC_SHIFT) * std::f32::consts::TAU / ELASTIC_PERIOD).sin();
    match mode {
        MolangEaseMode::In => {
            let t = t - 1.0;
            -(change * 2.0_f32.powf(10.0 * t) * wave(t)) + start
        }
        MolangEaseMode::Out => change * 2.0_f32.powf(-10.0 * t) * wave(t) + change + start,
        MolangEaseMode::InOut => {
            let t = t * 2.0 - 1.0;
            if t < 0.0 {
                -0.5 * change * 2.0_f32.powf(10.0 * t) * wave(t) + start
            } else {
                0.5 * change * 2.0_f32.powf(-10.0 * t) * wave(t) + change + start
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(function: MolangFunction, args: &[f32]) -> f32 {
        molang_call(function, args, &mut || 0.5)
    }

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1.0e-4
    }

    #[test]
    fn trigonometry_takes_and_returns_degrees() {
        assert!(close(call(MolangFunction::Sin, &[90.0]), 1.0));
        assert!(close(call(MolangFunction::Cos, &[180.0]), -1.0));
        assert!(close(call(MolangFunction::Asin, &[1.0]), 90.0));
        assert!(
            close(call(MolangFunction::Acos, &[1.0004]), 0.0),
            "near-unit input clamps"
        );
        assert!(call(MolangFunction::Acos, &[1.01]).is_nan());
        assert!(close(call(MolangFunction::Atan, &[1.0]), 45.0));
        assert!(close(call(MolangFunction::Atan2, &[1.0, -1.0]), 135.0));
    }

    #[test]
    fn rounding_sign_and_modulo_follow_vanilla_edge_rules() {
        assert_eq!(call(MolangFunction::Round, &[2.5]), 3.0);
        assert_eq!(call(MolangFunction::Round, &[-2.5]), -3.0);
        assert_eq!(call(MolangFunction::Round, &[2.49]), 2.0);
        assert_eq!(call(MolangFunction::Trunc, &[-2.7]), -2.0);
        assert_eq!(call(MolangFunction::Sign, &[-0.0]), 1.0);
        assert_eq!(call(MolangFunction::Sign, &[f32::NAN]), -1.0);
        assert_eq!(call(MolangFunction::CopySign, &[-3.0, 1.0]), 3.0);
        assert_eq!(call(MolangFunction::Mod, &[-7.0, 3.0]), -1.0);
        assert_eq!(call(MolangFunction::Mod, &[7.0, 0.0]), 0.0);
    }

    #[test]
    fn range_functions_keep_their_nan_and_reversed_bound_behavior() {
        assert_eq!(call(MolangFunction::Clamp, &[f32::NAN, 1.0, 2.0]), 1.0);
        assert_eq!(call(MolangFunction::Clamp, &[5.0, 3.0, 1.0]), 1.0);
        assert_eq!(call(MolangFunction::Clamp, &[0.0, 3.0, 1.0]), 3.0);
        assert_eq!(call(MolangFunction::Min, &[1.0, f32::NAN]), 1.0);
        assert_eq!(call(MolangFunction::Max, &[1.0, f32::NAN]), 1.0);
        assert_eq!(call(MolangFunction::Lerp, &[2.0, 4.0, 1.5]), 5.0);
        assert_eq!(call(MolangFunction::InverseLerp, &[2.0, 4.0, 3.0]), 0.5);
        assert_eq!(call(MolangFunction::HermiteBlend, &[0.5]), 0.5);
        assert_eq!(call(MolangFunction::MinAngle, &[190.0]), -170.0);
        assert_eq!(call(MolangFunction::MinAngle, &[-180.0]), -180.0);
        assert!(close(
            call(MolangFunction::LerpRotate, &[170.0, -170.0, 0.5]),
            180.0
        ));
    }

    #[test]
    fn random_family_spans_ordered_inclusive_ranges() {
        let low = |function, args: &[f32]| molang_call(function, args, &mut || 0.0);
        let high = |function, args: &[f32]| molang_call(function, args, &mut || 1.0);
        assert_eq!(low(MolangFunction::Random, &[5.0, 2.0]), 2.0);
        assert_eq!(high(MolangFunction::Random, &[5.0, 2.0]), 5.0);
        assert_eq!(high(MolangFunction::RandomInteger, &[1.0, 3.0]), 3.0);
        assert_eq!(low(MolangFunction::RandomInteger, &[1.0, 3.0]), 1.0);
        assert_eq!(high(MolangFunction::DieRoll, &[2.0, 1.7, 3.2]), 6.0);
        assert_eq!(low(MolangFunction::DieRollInteger, &[3.0, 1.0, 6.0]), 3.0);
        assert_eq!(low(MolangFunction::DieRoll, &[0.0, 1.0, 6.0]), 0.0);
        assert_eq!(low(MolangFunction::RandomInteger, &[3.0, 1.0]), 1.0);
        assert_eq!(high(MolangFunction::RandomInteger, &[3.0, 1.0]), 3.0);
        assert_eq!(low(MolangFunction::DieRollInteger, &[3.0, 6.0, 1.0]), 3.0);
        assert_eq!(high(MolangFunction::DieRollInteger, &[3.0, 6.0, 1.0]), 18.0);
    }

    #[test]
    fn integer_random_nan_bounds_keep_the_number_of_rolls() {
        for (function, dice) in [
            (MolangFunction::RandomInteger, 1),
            (MolangFunction::DieRollInteger, 3),
        ] {
            for (low, high) in [(f32::NAN, 0.0), (0.0, f32::NAN)] {
                let args = if function == MolangFunction::RandomInteger {
                    vec![low, high]
                } else {
                    vec![dice as f32, low, high]
                };
                let mut calls = 0;
                let result = molang_call(function, &args, &mut || {
                    calls += 1;
                    0.5
                });
                assert!(result.is_nan());
                assert_eq!(calls, dice);
            }
        }
    }

    #[test]
    fn easings_meet_their_endpoints_and_known_midpoints() {
        let curves = [
            MolangEaseCurve::Quad,
            MolangEaseCurve::Cubic,
            MolangEaseCurve::Quart,
            MolangEaseCurve::Quint,
            MolangEaseCurve::Sine,
            MolangEaseCurve::Circ,
            MolangEaseCurve::Bounce,
            MolangEaseCurve::Back,
        ];
        for curve in curves {
            for mode in [
                MolangEaseMode::In,
                MolangEaseMode::Out,
                MolangEaseMode::InOut,
            ] {
                let ease = |t| call(MolangFunction::Ease(curve, mode), &[2.0, 6.0, t]);
                assert!(close(ease(0.0), 2.0), "{curve:?} {mode:?} start");
                assert!(close(ease(1.0), 6.0), "{curve:?} {mode:?} end");
            }
        }
        let quad = |mode, t| {
            call(
                MolangFunction::Ease(MolangEaseCurve::Quad, mode),
                &[0.0, 1.0, t],
            )
        };
        assert!(close(quad(MolangEaseMode::In, 0.5), 0.25));
        assert!(close(quad(MolangEaseMode::Out, 0.5), 0.75));
        assert!(close(quad(MolangEaseMode::InOut, 0.25), 0.125));
        let expo = call(
            MolangFunction::Ease(MolangEaseCurve::Expo, MolangEaseMode::In),
            &[0.0, 1.0, 0.0],
        );
        assert!(
            close(expo, 2.0_f32.powi(-10)),
            "expo keeps its start offset"
        );
        let elastic = |mode, t| {
            call(
                MolangFunction::Ease(MolangEaseCurve::Elastic, mode),
                &[0.0, 1.0, t],
            )
        };
        // Without endpoint special cases the decaying wave leaves a residue under 2^-10.
        assert!((elastic(MolangEaseMode::Out, 1.0) - 1.0).abs() < 1.0e-3);
        assert!((elastic(MolangEaseMode::In, 1.0) - 1.0).abs() < 1.0e-3);
    }

    #[test]
    fn every_function_name_resolves_with_its_arity() {
        let names = [
            ("math.abs", 1),
            ("math.atan2", 2),
            ("math.die_roll_integer", 3),
            ("math.ease_in_out_elastic", 3),
            ("math.ease_out_bounce", 3),
            ("math.ease_in_back", 3),
            ("math.hermite_blend", 1),
            ("math.random_integer", 2),
            ("math.copy_sign", 2),
        ];
        for (name, arity) in names {
            assert_eq!(
                MolangFunction::from_name(name).unwrap().arity(),
                arity,
                "{name}"
            );
        }
        assert!(MolangFunction::from_name("math.ease_sideways_quad").is_none());
        assert!(MolangFunction::from_name("math.e").is_none());
    }
}
