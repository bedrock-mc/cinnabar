//! Dormant first-person hand/viewmodel bob evaluator.
//!
//! The presentation runtime does not yet own an exact walk-distance query
//! cadence, so this module is intentionally not wired into production. It
//! consumes already-authoritative query values and owns no camera transform,
//! movement state, or clock.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ViewmodelBobInput {
    pub life_time: f32,
    pub walk_distance: f32,
    pub position_delta_x: f32,
    pub position_delta_z: f32,
    pub on_ground: bool,
    pub alive: bool,
    pub bob_animation: bool,
    pub short_arm_offset_pixels: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ViewmodelBobOutput {
    pub left_arm_pixels: [f32; 3],
    pub right_arm_pixels: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ViewmodelBobEvaluator {
    horizontal_motion: f32,
    phase_radians: f64,
}

impl ViewmodelBobEvaluator {
    /// Evaluates one externally-cadenced query sample. Invalid numeric input
    /// is rejected without changing retained state.
    pub fn evaluate(&mut self, input: ViewmodelBobInput) -> Option<ViewmodelBobOutput> {
        let numeric_inputs = [
            input.life_time,
            input.walk_distance,
            input.position_delta_x,
            input.position_delta_z,
            input.short_arm_offset_pixels,
        ];
        if numeric_inputs.iter().any(|value| !value.is_finite()) {
            return None;
        }

        // Keep phase in f64 so every finite f32 walk distance remains finite
        // through the authored degree-to-radian conversion.
        let phase_radians = -f64::from(input.walk_distance) * std::f64::consts::PI;
        let horizontal_motion = if input.life_time < 0.01 {
            0.0
        } else {
            let target = if input.on_ground && input.alive {
                input
                    .position_delta_x
                    .hypot(input.position_delta_z)
                    .clamp(0.0, 0.1)
            } else {
                0.0
            };
            self.horizontal_motion + (target - self.horizontal_motion) * 0.02
        };
        if !phase_radians.is_finite() || !horizontal_motion.is_finite() {
            return None;
        }

        let output = if input.bob_animation {
            let x = (phase_radians.sin() * f64::from(horizontal_motion) * 9.75) as f32;
            let y = (-phase_radians.cos().abs() * f64::from(horizontal_motion) * 15.0
                + f64::from(input.short_arm_offset_pixels)) as f32;
            let arm = [x, y, 0.0];
            if arm.iter().any(|value| !value.is_finite()) {
                return None;
            }
            ViewmodelBobOutput {
                left_arm_pixels: arm,
                right_arm_pixels: arm,
            }
        } else {
            ViewmodelBobOutput::default()
        };

        self.horizontal_motion = horizontal_motion;
        self.phase_radians = phase_radians;
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1.0e-6;

    fn input() -> ViewmodelBobInput {
        ViewmodelBobInput {
            life_time: 1.0,
            walk_distance: 0.5,
            position_delta_x: 0.1,
            position_delta_z: 0.0,
            on_ground: true,
            alive: true,
            bob_animation: true,
            short_arm_offset_pixels: 2.0,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn life_time_reset_clears_smoothed_motion_before_output() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        evaluator.evaluate(input()).unwrap();

        let output = evaluator
            .evaluate(ViewmodelBobInput {
                life_time: 0.009,
                ..input()
            })
            .unwrap();

        assert_eq!(evaluator.horizontal_motion, 0.0);
        assert_close(output.left_arm_pixels[0], 0.0);
        assert_close(output.left_arm_pixels[1], 2.0);

        evaluator
            .evaluate(ViewmodelBobInput {
                life_time: 0.01,
                ..input()
            })
            .unwrap();
        assert_close(evaluator.horizontal_motion, 0.002);
    }

    #[test]
    fn airborne_or_dead_samples_decay_toward_zero() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        evaluator.evaluate(input()).unwrap();
        assert_close(evaluator.horizontal_motion, 0.002);

        evaluator
            .evaluate(ViewmodelBobInput {
                on_ground: false,
                ..input()
            })
            .unwrap();
        assert_close(evaluator.horizontal_motion, 0.00196);

        evaluator
            .evaluate(ViewmodelBobInput {
                alive: false,
                ..input()
            })
            .unwrap();
        assert_close(evaluator.horizontal_motion, 0.0019208);
    }

    #[test]
    fn horizontal_target_is_clamped_to_one_tenth() {
        let mut at_limit = ViewmodelBobEvaluator::default();
        let mut above_limit = ViewmodelBobEvaluator::default();

        let expected = at_limit.evaluate(input()).unwrap();
        let clamped = above_limit
            .evaluate(ViewmodelBobInput {
                position_delta_x: f32::MAX,
                position_delta_z: f32::MAX,
                ..input()
            })
            .unwrap();

        assert_eq!(expected, clamped);
        assert_close(above_limit.horizontal_motion, 0.002);
        assert!(
            clamped
                .left_arm_pixels
                .iter()
                .all(|value| value.is_finite())
        );
    }

    #[test]
    fn both_arms_receive_the_same_signed_translation() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        let output = evaluator
            .evaluate(ViewmodelBobInput {
                walk_distance: 0.25,
                ..input()
            })
            .unwrap();

        assert_eq!(output.left_arm_pixels, output.right_arm_pixels);
        let phase = -std::f64::consts::FRAC_PI_4;
        assert_close(
            output.left_arm_pixels[0],
            (phase.sin() * 0.002 * 9.75) as f32,
        );
        assert_close(
            output.left_arm_pixels[1],
            (-phase.cos().abs() * 0.002 * 15.0 + 2.0) as f32,
        );
        assert_eq!(output.left_arm_pixels[2], 0.0);
    }

    #[test]
    fn phase_is_derived_only_from_supplied_walk_distance() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        evaluator.evaluate(input()).unwrap();
        let phase = evaluator.phase_radians;

        evaluator
            .evaluate(ViewmodelBobInput {
                position_delta_x: 0.0,
                position_delta_z: 0.1,
                ..input()
            })
            .unwrap();

        assert_eq!(evaluator.phase_radians, phase);
    }

    #[test]
    fn animation_toggle_hides_output_without_resetting_state() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        evaluator.evaluate(input()).unwrap();

        let hidden = evaluator
            .evaluate(ViewmodelBobInput {
                walk_distance: 0.25,
                bob_animation: false,
                ..input()
            })
            .unwrap();

        assert_eq!(hidden, ViewmodelBobOutput::default());
        assert_close(evaluator.horizontal_motion, 0.00396);
        assert_close(evaluator.phase_radians as f32, -std::f32::consts::FRAC_PI_4);

        let visible = evaluator
            .evaluate(ViewmodelBobInput {
                walk_distance: 0.25,
                ..input()
            })
            .unwrap();
        assert_ne!(visible, ViewmodelBobOutput::default());
    }

    #[test]
    fn non_finite_inputs_fail_closed_without_mutating_state() {
        let mut evaluator = ViewmodelBobEvaluator::default();
        evaluator.evaluate(input()).unwrap();
        let retained_motion = evaluator.horizontal_motion;
        let retained_phase = evaluator.phase_radians;

        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for sample in [
                ViewmodelBobInput {
                    life_time: invalid,
                    ..input()
                },
                ViewmodelBobInput {
                    walk_distance: invalid,
                    ..input()
                },
                ViewmodelBobInput {
                    position_delta_x: invalid,
                    ..input()
                },
                ViewmodelBobInput {
                    position_delta_z: invalid,
                    ..input()
                },
                ViewmodelBobInput {
                    short_arm_offset_pixels: invalid,
                    ..input()
                },
            ] {
                assert_eq!(evaluator.evaluate(sample), None);
                assert_eq!(evaluator.horizontal_motion, retained_motion);
                assert_eq!(evaluator.phase_radians, retained_phase);
            }
        }
    }
}
