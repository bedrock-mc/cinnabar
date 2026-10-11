//! Fixed-tick sprint admission and collision-sensitive continuation.

use super::{ModeIntent, ModeObservation};
use crate::movement::control_modes::{SPRINT_THRESHOLD, can_continue_sprint};
use sim::Vec3;

const MIN_SPRINT_DISPLACEMENT: f32 = 0.00005;

/// Sprint double taps count successful simulation ticks, including catch-up ticks.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SprintTrigger {
    double_tap_ticks: u8,
    previous_forward: bool,
    previous_sneak: bool,
    started_by_button: bool,
    automatic_sprint: bool,
}

impl SprintTrigger {
    /// Retains the final processed forward lane and the independent sneak request.
    pub(super) fn record_controls(&mut self, forward: f32, sneak_down: bool) {
        self.previous_forward = forward >= SPRINT_THRESHOLD;
        self.previous_sneak = sneak_down;
    }

    /// Updates the seven-tick forward latch and applies start/continuation conditions.
    pub(super) fn select(
        &mut self,
        was_sprinting: bool,
        previous_feet: Option<Vec3>,
        intent: ModeIntent,
        observed: ModeObservation,
    ) -> bool {
        self.double_tap_ticks = self.double_tap_ticks.saturating_sub(1);
        let forward = observed.move_forward >= SPRINT_THRESHOLD;
        let admission = forward
            && !intent.sprint_blocked
            && !intent.sprint_start_blocked
            && !observed.sprint_blinded;
        let mut double_tap = false;
        if !was_sprinting
            && admission
            && !observed.sprint_down
            && (observed.on_ground || observed.in_water || intent.can_fly || intent.spectator)
            && !self.previous_forward
            && !self.previous_sneak
        {
            if self.double_tap_ticks > 0 {
                double_tap = true;
            } else {
                self.double_tap_ticks = 7;
            }
        }

        let start = admission && (observed.sprint_down || observed.sprinting || double_tap);
        if !was_sprinting && start {
            self.started_by_button = observed.sprint_down;
            self.automatic_sprint = double_tap;
        }
        let touch_release = observed.input_mode == protocol::PlayerInputMode::Touch
            && self.started_by_button
            && !observed.sprint_down;
        let continuing =
            was_sprinting && (observed.sprinting || observed.sprint_down || self.automatic_sprint);
        let sprinting = (continuing || start)
            && !intent.stop_sprinting
            && !intent.sprint_blocked
            && !touch_release
            && can_sprint(was_sprinting, previous_feet, observed);
        if !sprinting {
            self.started_by_button = false;
            self.automatic_sprint = false;
        }
        sprinting
    }
}

/// Uses the preceding movement request's dominant axis to detect a blocked sprint.
pub(super) fn can_sprint(
    was_sprinting: bool,
    previous_feet: Option<Vec3>,
    observed: ModeObservation,
) -> bool {
    if !can_continue_sprint(observed.move_sideways, observed.move_forward)
        || (!was_sprinting && (observed.move_forward < SPRINT_THRESHOLD || observed.sprint_blinded))
    {
        return false;
    }
    let Some(previous) = previous_feet else {
        return true;
    };
    let requested_x = (observed.requested_movement.x as f32).abs();
    let requested_z = (observed.requested_movement.z as f32).abs();
    let moved_x = (observed.feet.x as f32 - previous.x as f32).abs();
    let moved_z = (observed.feet.z as f32 - previous.z as f32).abs();
    (requested_z <= requested_x || moved_z >= MIN_SPRINT_DISPLACEMENT)
        && (requested_x <= requested_z || moved_x >= MIN_SPRINT_DISPLACEMENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Produces a forward sprint request after a prior world-space movement attempt.
    fn sprint(request: Vec3, position: Vec3) -> ModeObservation {
        ModeObservation {
            feet: position,
            requested_movement: request,
            on_ground: true,
            in_water: false,
            sprinting: true,
            sprint_blinded: false,
            sprint_down: false,
            input_mode: protocol::PlayerInputMode::Mouse,
            move_sideways: 0.0,
            move_forward: 1.0,
            sneaking: false,
            pitch: 0.0,
            yaw: 0.0,
            liquid_attach_height: protocol::PLAYER_NETWORK_OFFSET,
            jumping: false,
            jump_edge: false,
        }
    }

    /// Runs the trigger and stores the test's already processed input lanes.
    fn select(
        trigger: &mut SprintTrigger,
        was_sprinting: bool,
        previous: Option<Vec3>,
        intent: ModeIntent,
        observed: ModeObservation,
    ) -> bool {
        let sprinting = trigger.select(was_sprinting, previous, intent, observed);
        trigger.record_controls(observed.move_forward, observed.sneaking);
        sprinting
    }

    /// The wall check tests displacement along the dominant requested axis, with ties allowed.
    #[test]
    fn sprint_stall_uses_requested_axis_and_exact_displacement_boundary() {
        let request = Vec3::new(0.01, 0.0, 0.2);
        assert!(!can_sprint(
            true,
            Some(Vec3::ZERO),
            sprint(request, Vec3::new(0.1, 0.0, 0.0))
        ));
        assert!(can_sprint(
            true,
            Some(Vec3::ZERO),
            sprint(
                request,
                Vec3::new(0.0, 0.0, f64::from(MIN_SPRINT_DISPLACEMENT))
            )
        ));
        assert!(!can_sprint(
            true,
            Some(Vec3::ZERO),
            sprint(
                request,
                Vec3::new(
                    0.0,
                    0.0,
                    f64::from(f32::from_bits(MIN_SPRINT_DISPLACEMENT.to_bits() - 1))
                )
            )
        ));
        assert!(can_sprint(
            true,
            Some(Vec3::ZERO),
            sprint(Vec3::new(0.2, 0.0, 0.2), Vec3::ZERO)
        ));
    }

    /// Blindness refuses a new sprint without ending an already active sprint.
    #[test]
    fn blindness_only_blocks_sprint_admission() {
        let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
        observed.sprint_blinded = true;
        assert!(!can_sprint(false, None, observed));
        assert!(can_sprint(true, None, observed));
    }
    /// Forward double taps expire after seven completed ticks and require grounded, wet or flying admission.
    #[test]
    fn double_tap_uses_tick_expiry_and_start_environment() {
        for (gap, expected) in [(6, true), (7, false)] {
            let mut trigger = SprintTrigger::default();
            let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
            observed.sprinting = false;
            assert!(!select(
                &mut trigger,
                false,
                None,
                ModeIntent::default(),
                observed
            ));
            for _ in 1..gap {
                let mut neutral = observed;
                neutral.move_forward = 0.0;
                assert!(!select(
                    &mut trigger,
                    false,
                    None,
                    ModeIntent::default(),
                    neutral
                ));
            }
            assert_eq!(
                select(&mut trigger, false, None, ModeIntent::default(), observed),
                expected
            );
        }
        for (grounded, water, can_fly, expected) in [
            (false, false, false, false),
            (true, false, false, true),
            (false, true, false, true),
            (false, false, true, true),
        ] {
            let mut trigger = SprintTrigger::default();
            let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
            observed.sprinting = false;
            observed.on_ground = grounded;
            observed.in_water = water;
            let intent = ModeIntent {
                can_fly,
                ..Default::default()
            };
            assert!(!select(&mut trigger, false, None, intent, observed));
            let mut neutral = observed;
            neutral.move_forward = 0.0;
            assert!(!select(&mut trigger, false, None, intent, neutral));
            assert_eq!(
                select(&mut trigger, false, None, intent, observed),
                expected
            );
        }
    }

    /// An automatic double-tap sprint remains active on later ticks in the same render frame.
    #[test]
    fn double_tap_sprint_continues_without_a_button_request() {
        let mut trigger = SprintTrigger::default();
        let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
        observed.sprinting = false;
        assert!(!select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
        let mut neutral = observed;
        neutral.move_forward = 0.0;
        assert!(!select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            neutral
        ));
        assert!(select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
        assert!(select(
            &mut trigger,
            true,
            None,
            ModeIntent::default(),
            observed
        ));
        assert!(!select(
            &mut trigger,
            true,
            None,
            ModeIntent {
                sprint_blocked: true,
                ..Default::default()
            },
            observed
        ));
    }
    /// Explicit setting stops clear an automatic sprint even while forward remains held.
    #[test]
    fn explicit_stop_clears_the_automatic_double_tap_latch() {
        let mut trigger = SprintTrigger::default();
        let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
        observed.sprinting = false;
        assert!(!select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
        let mut neutral = observed;
        neutral.move_forward = 0.0;
        assert!(!select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            neutral
        ));
        assert!(select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
        assert!(!select(
            &mut trigger,
            true,
            None,
            ModeIntent {
                stop_sprinting: true,
                ..Default::default()
            },
            observed
        ));
        assert!(!select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
    }

    /// Sprint intent runs before sneak slowdown, and a crouch request does not cancel its action.
    #[test]
    fn a_sprint_button_can_start_while_crouch_is_requested() {
        let mut trigger = SprintTrigger::default();
        let mut observed = sprint(Vec3::ZERO, Vec3::ZERO);
        observed.sprint_down = true;
        observed.sneaking = true;
        assert!(select(
            &mut trigger,
            false,
            None,
            ModeIntent::default(),
            observed
        ));
        assert!(select(
            &mut trigger,
            true,
            None,
            ModeIntent::default(),
            observed
        ));
    }
}
