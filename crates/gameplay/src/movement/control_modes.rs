//! Latched sprint and sneak state: sprint key, double-tap forward, toggle options and
//! the conditions that end a sprint.

use std::time::Duration;

/// Minimum forward input needed to begin sprinting.
pub(super) const SPRINT_THRESHOLD: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Food level at or below which survival sprinting is refused.
pub const SPRINT_HUNGER_FLOOR: u16 = 6;

/// One render frame of sprint/sneak-relevant facts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlObservation {
    pub now: Duration,
    /// Forward axis after device normalization; positive is forward.
    pub forward: f32,
    pub sideways: f32,
    /// Touch sprint ends when its initiating sprint control is released.
    pub touch_input: bool,
    pub sprint_pressed: bool,
    pub sprint_held: bool,
    pub sneak_pressed: bool,
    pub sneak_held: bool,
    pub toggle_sprint: bool,
    pub always_sprint: bool,
    pub toggle_sneak: bool,
    /// Hunger prevents both new and continuing sprints.
    pub sprint_blocked: bool,
    /// Using an item prevents a new sprint without stopping an existing one.
    pub sprint_start_blocked: bool,
    /// Ability flight is active, where sneak means descend and never latches.
    pub flying: bool,
    /// Vanilla cannot stop an existing sprint while the previous
    /// swimming pose has current body-water contact.
    pub retain_sprint: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlOutput {
    pub sprint_request: bool,
    pub sprint_down: bool,
    pub stop_sprinting: bool,
    pub sneaking: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ControlModes {
    sprinting: bool,
    sprint_toggled: bool,
    was_always_sprint: bool,
    sneak_toggled: bool,
    stop_sprinting: bool,
    started_by_sprint_control: bool,
}

impl ControlModes {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Adopts the completed fixed tick's actor flag without changing toggle intent.
    pub(crate) fn adopt_tick_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.stop_sprinting = false;
    }

    /// Adopts server-authored sprint/sneak states; the next local transition still wins.
    pub fn adopt_server_flags(&mut self, sprinting: Option<bool>, sneaking: Option<bool>) {
        if let Some(sprinting) = sprinting {
            self.sprinting = sprinting;
            self.sprint_toggled = sprinting;
        }
        if let Some(sneaking) = sneaking {
            self.sneak_toggled = sneaking;
        }
    }

    pub fn update(&mut self, observed: ControlObservation) -> ControlOutput {
        let moving_forward = observed.forward >= SPRINT_THRESHOLD;
        let sneaking = if observed.toggle_sneak && !observed.flying {
            if observed.sneak_pressed {
                self.sneak_toggled = !self.sneak_toggled;
            }
            self.sneak_toggled
        } else {
            self.sneak_toggled = false;
            observed.sneak_held
        };

        if self.was_always_sprint && !observed.always_sprint && !observed.retain_sprint {
            self.sprinting = false;
            self.stop_sprinting = true;
        }
        self.was_always_sprint = observed.always_sprint;

        if observed.toggle_sprint {
            if observed.sprint_pressed {
                self.sprint_toggled = !self.sprint_toggled;
                if !self.sprint_toggled && !observed.retain_sprint {
                    self.sprinting = false;
                    self.stop_sprinting = true;
                }
            }
        } else {
            self.sprint_toggled = false;
        }

        let sprint_down = observed.always_sprint
            || if observed.toggle_sprint {
                self.sprint_toggled
            } else {
                observed.sprint_held
            };
        let directional = if self.sprinting {
            can_continue_sprint(observed.sideways, observed.forward)
        } else {
            moving_forward
        };
        let touch_released = observed.touch_input && self.started_by_sprint_control && !sprint_down;
        let can_sprint = directional
            && !observed.sprint_blocked
            && !touch_released
            && (self.sprinting || !observed.sprint_start_blocked);
        if !can_sprint {
            if !observed.retain_sprint {
                self.sprinting = false;
            }
        } else if observed.always_sprint
            || (observed.sprint_held && !observed.toggle_sprint)
            || self.sprint_toggled
        {
            if !self.sprinting {
                self.started_by_sprint_control = sprint_down;
            }
            self.sprinting = true;
        }
        ControlOutput {
            sprint_request: self.sprinting,
            sprint_down,
            stop_sprinting: self.stop_sprinting,
            sneaking,
        }
    }
}

/// Existing sprints permit a reduced forward component only within the forward cone.
pub(super) fn can_continue_sprint(sideways: f32, forward: f32) -> bool {
    (sideways * sideways + forward * forward).sqrt() >= SPRINT_THRESHOLD
        && forward > 0.0
        && sideways.abs() <= SPRINT_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(millis: u64, forward: f32) -> ControlObservation {
        ControlObservation {
            now: Duration::from_millis(millis),
            forward,
            ..ControlObservation::default()
        }
    }

    /// Analogue admission and continuation have distinct input thresholds.
    #[test]
    fn analogue_sprint_uses_start_threshold_and_continuation_cone() {
        let mut modes = ControlModes::default();
        let below = f32::from_bits(SPRINT_THRESHOLD.to_bits() - 1);
        assert!(
            !modes
                .update(ControlObservation {
                    sprint_held: true,
                    ..frame(0, below)
                })
                .sprint_request
        );
        assert!(
            modes
                .update(ControlObservation {
                    sprint_held: true,
                    ..frame(50, SPRINT_THRESHOLD)
                })
                .sprint_request
        );
        assert!(
            modes
                .update(ControlObservation {
                    sideways: 0.6,
                    ..frame(100, 0.5)
                })
                .sprint_request
        );
        assert!(
            !modes
                .update(ControlObservation {
                    sideways: 0.8,
                    ..frame(150, 0.5)
                })
                .sprint_request
        );
    }

    /// Touch release stops a key-started sprint while keyboard release remains latched.
    #[test]
    fn touch_sprint_control_release_ends_its_sprint() {
        let mut modes = ControlModes::default();
        assert!(
            modes
                .update(ControlObservation {
                    touch_input: true,
                    sprint_held: true,
                    ..frame(0, 1.0)
                })
                .sprint_request
        );
        assert!(
            !modes
                .update(ControlObservation {
                    touch_input: true,
                    ..frame(50, 1.0)
                })
                .sprint_request
        );
    }

    /// Item use blocks a new sprint while an existing sprint keeps its actor state.
    #[test]
    fn item_use_only_blocks_new_sprint_admission() {
        let mut modes = ControlModes::default();
        assert!(
            !modes
                .update(ControlObservation {
                    sprint_held: true,
                    sprint_start_blocked: true,
                    ..frame(0, 1.0)
                })
                .sprint_request
        );
        assert!(
            modes
                .update(ControlObservation {
                    sprint_held: true,
                    ..frame(50, 1.0)
                })
                .sprint_request
        );
        assert!(
            modes
                .update(ControlObservation {
                    sprint_start_blocked: true,
                    ..frame(100, 1.0)
                })
                .sprint_request
        );
    }

    #[test]
    fn sprint_key_latches_until_forward_input_ends() {
        let mut modes = ControlModes::default();
        let held = ControlObservation {
            sprint_held: true,
            ..frame(0, 1.0)
        };
        assert!(modes.update(held).sprint_request);
        assert!(modes.update(frame(50, 1.0)).sprint_request);
        assert!(!modes.update(frame(100, 0.0)).sprint_request);
        assert!(!modes.update(frame(900, 1.0)).sprint_request);
    }

    #[test]
    fn wet_swimming_retains_sprint_until_the_native_stop_path_is_available() {
        let mut modes = ControlModes::default();
        assert!(
            modes
                .update(ControlObservation {
                    sprint_held: true,
                    ..frame(0, 1.0)
                })
                .sprint_request
        );
        for (millis, forward, sneak, blocked) in [
            (50, -1.0, false, false),
            (100, 0.0, false, false),
            (150, 1.0, true, false),
            (200, 1.0, false, true),
        ] {
            assert!(
                modes
                    .update(ControlObservation {
                        retain_sprint: true,
                        sneak_held: sneak,
                        sprint_blocked: blocked,
                        ..frame(millis, forward)
                    })
                    .sprint_request
            );
        }
        assert!(!modes.update(frame(250, -1.0)).sprint_request);
        // Retention inhibits stopping; it cannot create an invalid new start.
        assert!(
            !modes
                .update(ControlObservation {
                    retain_sprint: true,
                    sprint_held: true,
                    ..frame(300, -1.0)
                })
                .sprint_request
        );
    }

    #[test]
    fn completed_sprint_state_does_not_replace_toggle_intent() {
        let mut modes = ControlModes::default();
        modes.adopt_tick_sprinting(true);
        assert!(!modes.sprint_toggled);
        assert!(
            modes
                .update(ControlObservation {
                    retain_sprint: true,
                    ..frame(50, -1.0)
                })
                .sprint_request
        );
        modes.adopt_tick_sprinting(false);
        assert!(!modes.update(frame(100, 1.0)).sprint_request);
    }

    #[test]
    fn render_frame_forward_taps_wait_for_fixed_tick_admission() {
        let mut modes = ControlModes::default();
        modes.update(frame(0, 1.0));
        modes.update(frame(100, 0.0));
        assert!(!modes.update(frame(200, 1.0)).sprint_request);

        let mut slow = ControlModes::default();
        slow.update(frame(0, 1.0));
        slow.update(frame(100, 0.0));
        assert!(!slow.update(frame(900, 1.0)).sprint_request);
    }

    #[test]
    fn sneak_retains_sprint_and_hunger_ends_it() {
        let mut modes = ControlModes::default();
        let sprint = ControlObservation {
            sprint_held: true,
            ..frame(0, 1.0)
        };
        assert!(modes.update(sprint).sprint_request);

        let sneak = ControlObservation {
            sneak_held: true,
            ..sprint
        };
        let output = modes.update(sneak);
        assert!(output.sneaking && output.sprint_request);

        let hungry = ControlObservation {
            sprint_blocked: true,
            ..sprint
        };
        assert!(!modes.update(hungry).sprint_request);
    }

    #[test]
    fn toggle_sprint_survives_key_release_and_toggles_off_on_second_press() {
        let mut modes = ControlModes::default();
        let press = ControlObservation {
            toggle_sprint: true,
            sprint_pressed: true,
            sprint_held: true,
            ..frame(0, 1.0)
        };
        assert!(modes.update(press).sprint_request);
        let released = ControlObservation {
            toggle_sprint: true,
            ..frame(50, 1.0)
        };
        assert!(modes.update(released).sprint_request);
        assert!(
            !modes
                .update(ControlObservation {
                    now: Duration::from_millis(100),
                    ..press
                })
                .sprint_request
        );
    }

    #[test]
    fn toggle_sneak_latches_and_flight_never_latches() {
        let mut modes = ControlModes::default();
        let press = ControlObservation {
            toggle_sneak: true,
            sneak_pressed: true,
            sneak_held: true,
            ..ControlObservation::default()
        };
        assert!(modes.update(press).sneaking);
        let released = ControlObservation {
            toggle_sneak: true,
            ..ControlObservation::default()
        };
        assert!(modes.update(released).sneaking);
        let flying = ControlObservation {
            flying: true,
            ..released
        };
        assert!(!modes.update(flying).sneaking);
        assert!(!modes.update(released).sneaking);
    }

    #[test]
    fn toggle_disabled_follows_the_held_button() {
        let mut modes = ControlModes::default();
        let held = ControlObservation {
            sneak_held: true,
            ..ControlObservation::default()
        };
        assert!(modes.update(held).sneaking);
        assert!(!modes.update(ControlObservation::default()).sneaking);
    }
}
