//! Camera easing curves addressed by wire selector or name; standard easing families.
//! The spring curve is an approximation and needs native measurement.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

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

/// Wire selector for a named easing; unknown names resolve to linear.
#[must_use]
pub fn kind_from_name(name: &str) -> u8 {
    NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map_or(0, |index| index as u8)
}

/// Eased progress for wire selector `kind` at `t` in `0..=1`; unknown selectors are linear.
#[must_use]
pub fn ease(kind: u8, t: f32) -> f32 {
    if !t.is_finite() {
        return 1.0;
    }
    let t = t.clamp(0.0, 1.0);
    if t >= 1.0 {
        return 1.0;
    }
    let family = kind.saturating_sub(2) / 3;
    let variant = kind.saturating_sub(2) % 3;
    match kind {
        0 | 32.. => t,
        1 => 1.0 - (-6.0 * t).exp() * (8.0 * t).cos(),
        2..=13 => polynomial(u32::from(family) + 2, variant, t),
        14..=16 => match variant {
            0 => 1.0 - (t * FRAC_PI_2).cos(),
            1 => (t * FRAC_PI_2).sin(),
            _ => -((PI * t).cos() - 1.0) / 2.0,
        },
        17..=19 => expo(variant, t),
        20..=22 => circ(variant, t),
        23..=25 => bounce(variant, t),
        26..=28 => back(variant, t),
        _ => elastic(variant, t),
    }
}

fn polynomial(power: u32, variant: u8, t: f32) -> f32 {
    let power = power as i32;
    match variant {
        0 => t.powi(power),
        1 => 1.0 - (1.0 - t).powi(power),
        _ if t < 0.5 => 2.0_f32.powi(power - 1) * t.powi(power),
        _ => 1.0 - (-2.0 * t + 2.0).powi(power) / 2.0,
    }
}

fn expo(variant: u8, t: f32) -> f32 {
    let rise = |x: f32| {
        if x <= 0.0 {
            0.0
        } else {
            2.0_f32.powf(10.0 * x - 10.0)
        }
    };
    match variant {
        0 => rise(t),
        1 => 1.0 - rise(1.0 - t),
        _ if t <= 0.0 => 0.0,
        _ if t < 0.5 => 2.0_f32.powf(20.0 * t - 10.0) / 2.0,
        _ => (2.0 - 2.0_f32.powf(-20.0 * t + 10.0)) / 2.0,
    }
}

fn circ(variant: u8, t: f32) -> f32 {
    match variant {
        0 => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
        1 => (1.0 - (t - 1.0).powi(2)).max(0.0).sqrt(),
        _ if t < 0.5 => (1.0 - (1.0 - (2.0 * t).powi(2)).max(0.0).sqrt()) / 2.0,
        _ => ((1.0 - (-2.0 * t + 2.0).powi(2)).max(0.0).sqrt() + 1.0) / 2.0,
    }
}

fn out_bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984_375
    }
}

fn bounce(variant: u8, t: f32) -> f32 {
    match variant {
        0 => 1.0 - out_bounce(1.0 - t),
        1 => out_bounce(t),
        _ if t < 0.5 => (1.0 - out_bounce(1.0 - 2.0 * t)) / 2.0,
        _ => (1.0 + out_bounce(2.0 * t - 1.0)) / 2.0,
    }
}

fn back(variant: u8, t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C2: f32 = C1 * 1.525;
    const C3: f32 = C1 + 1.0;
    match variant {
        0 => C3 * t * t * t - C1 * t * t,
        1 => 1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2),
        _ if t < 0.5 => ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0,
        _ => ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0,
    }
}

fn elastic(variant: u8, t: f32) -> f32 {
    const C4: f32 = TAU / 3.0;
    const C5: f32 = TAU / 4.5;
    if t <= 0.0 {
        return 0.0;
    }
    match variant {
        0 => -(2.0_f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * C4).sin(),
        1 => 2.0_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0,
        _ if t < 0.5 => -(2.0_f32.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * C5).sin()) / 2.0,
        _ => (2.0_f32.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * C5).sin()) / 2.0 + 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        for kind in 0..=32u8 {
            assert!(ease(kind, 0.0).abs() < 1e-3, "kind {kind} start");
            assert!((ease(kind, 1.0) - 1.0).abs() < 1e-6, "kind {kind} end");
            assert!(ease(kind, 0.37).is_finite(), "kind {kind} mid");
        }
    }

    #[test]
    fn symmetric_curves_cross_half_at_the_midpoint() {
        for name in [
            "linear",
            "in_out_quad",
            "in_out_cubic",
            "in_out_sine",
            "in_out_circ",
        ] {
            assert!(
                (ease(kind_from_name(name), 0.5) - 0.5).abs() < 1e-5,
                "{name}"
            );
        }
    }

    #[test]
    fn quad_family_matches_reference_values() {
        assert!((ease(2, 0.5) - 0.25).abs() < 1e-6);
        assert!((ease(3, 0.5) - 0.75).abs() < 1e-6);
        assert!((ease(5, 0.5) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn names_map_to_selectors_and_unknowns_are_linear() {
        assert_eq!(kind_from_name("in_out_elastic"), 31);
        assert_eq!(kind_from_name("out_quad"), 3);
        assert_eq!(kind_from_name("nonsense"), 0);
        assert!((ease(200, 0.25) - 0.25).abs() < 1e-6);
        assert_eq!(ease(3, f32::NAN), 1.0);
    }
}
