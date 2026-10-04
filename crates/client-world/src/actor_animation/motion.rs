//! Per-actor walk cycle, arm swing, fish phase, and body rotation that vanilla animations read through
//! queries. Ordinary walking and turning use vanilla constants;
//! specialized hurt/fire/jump multipliers and render-time query sampling remain incomplete.
use super::query::wrap_degrees;

/// Ticks one arm swing takes without haste or fatigue.
pub const ACTOR_SWING_TICKS: i32 = 6;
const WALK_STEP_GAIN: f32 = 1.6;
const WALK_STEP_TARGET_MAX: f32 = 0.4;
const WALK_TURN_GAIN: f32 = 0.02;
const WALK_TURN_TARGET_MAX: f32 = 0.2;
const WALK_SPEED_RETAIN: f32 = 0.6;
const PLAYER_FACING_STEP: f32 = 0.05;
const PLAYER_BODY_FOLLOW: f32 = 0.3;
const PLAYER_HEAD_LIMIT: f32 = 75.0;
const PLAYER_HEAD_SOFT_LIMIT: f32 = 50.0;
const PLAYER_HEAD_SOFT_PULL: f32 = 0.75;
const MOB_MOVING_DISPLACEMENT_SQUARED: f32 = 2.5e-7;
const MOB_BODY_TURN_STEP: f32 = 25.0;
const MOB_HEAD_LIMIT: f32 = 75.0;
const MOB_HEAD_RESTABLE: f32 = 15.0;
const MOB_STABLE_TICKS: u32 = 10;
const STRIDE_STEP_MIN: f32 = 0.05;
const STRIDE_GAIN: f32 = 3.0;
const STRIDE_READ_SCALE: f32 = 0.6;
// FishAnimationSystem tick consumes StateVector velocity in blocks/tick.
const FISH_PHASE_SPEED_GAIN: f32 = 0.1;

/// One tick of actor state the motion model consumes.
pub(super) struct MotionInput {
    pub(super) delta: [f32; 3],
    pub(super) riding: bool,
    pub(super) player: bool,
    pub(super) yaw: f32,
    pub(super) head_yaw: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MotionState {
    pub(super) distance: f32,
    pub(super) speed: f32,
    /// Swing counter; `-1` marks a swing requested since the last tick.
    swing: Option<i32>,
    /// Length of the current swing in ticks, after haste and fatigue.
    swing_ticks: i32,
    pub(super) body_yaw: f32,
    pub(super) previous_body_yaw: f32,
    stable_head_yaw: f32,
    stable_ticks: u32,
    /// Stride accumulator behind `query.walk_distance`.
    stride: f32,
    /// FishAnimationComponent survives geometry/controller resets for this actor lifetime.
    fish_phase: [f32; 2],
    pub(super) horse: super::horse::AnimationState,
}

impl MotionState {
    pub(super) fn spawn(body_yaw: f32, head_yaw: f32) -> Self {
        Self {
            body_yaw,
            previous_body_yaw: body_yaw,
            stable_head_yaw: head_yaw,
            ..Self::default()
        }
    }

    /// Restarts a `ticks`-long swing unless one is still in its first half.
    pub(super) fn start_swing(&mut self, ticks: i32) {
        if self
            .swing
            .is_some_and(|counter| counter < self.swing_ticks / 2)
        {
            return;
        }
        self.swing = Some(-1);
        self.swing_ticks = ticks.max(1);
    }

    pub(super) fn walk_distance(self) -> f32 {
        self.stride * STRIDE_READ_SCALE
    }

    pub(super) fn attack_time(self) -> f32 {
        self.swing.map_or(0.0, |counter| {
            counter.max(0) as f32 / self.swing_ticks.max(1) as f32
        })
    }

    /// Current and previous native fish animation amounts, in that order.
    pub(super) fn fish_phase(self) -> [f32; 2] {
        self.fish_phase
    }

    pub(super) fn advance_fish(&mut self, velocity: [f32; 3]) {
        self.fish_phase[1] = self.fish_phase[0];
        let [x, y, z] = velocity;
        let speed = (z * z + y * y + x * x).sqrt();
        self.fish_phase[0] = (self.fish_phase[0] + 1.0) + speed * FISH_PHASE_SPEED_GAIN;
    }

    pub(super) fn advance(&mut self, input: &MotionInput) {
        self.swing = self
            .swing
            .map(|counter| counter + 1)
            .filter(|counter| *counter < self.swing_ticks);
        self.previous_body_yaw = self.body_yaw;
        if input.player {
            self.turn_player_body(input);
        } else {
            self.turn_mob_body(input);
        }
        let step = input.delta[0].hypot(input.delta[2]);
        if step > STRIDE_STEP_MIN {
            self.stride += STRIDE_GAIN * step;
        }
        if input.riding {
            self.speed = 0.0;
            return;
        }
        let target = if step != 0.0 {
            (WALK_STEP_GAIN * step).min(WALK_STEP_TARGET_MAX)
        } else {
            let turn = wrap_degrees(self.body_yaw - self.previous_body_yaw).abs();
            (WALK_TURN_GAIN * turn).min(WALK_TURN_TARGET_MAX)
        };
        self.speed = WALK_SPEED_RETAIN * self.speed + target;
        self.distance += self.speed;
    }

    fn turn_player_body(&mut self, input: &MotionInput) {
        let [dx, _, dz] = input.delta;
        let mut target = if dx.hypot(dz) > PLAYER_FACING_STEP {
            dz.atan2(dx).to_degrees() - 90.0
        } else {
            self.body_yaw
        };
        if self.attack_time() > 0.0 {
            target = input.yaw;
        }
        self.body_yaw += wrap_degrees(target - self.body_yaw) * PLAYER_BODY_FOLLOW;
        if input.riding {
            return;
        }
        let mut difference =
            wrap_degrees(input.yaw - self.body_yaw).clamp(-PLAYER_HEAD_LIMIT, PLAYER_HEAD_LIMIT);
        if difference.abs() > PLAYER_HEAD_SOFT_LIMIT {
            difference -= (difference.abs() - PLAYER_HEAD_SOFT_LIMIT)
                * PLAYER_HEAD_SOFT_PULL
                * difference.signum();
        }
        self.body_yaw = wrap_degrees(input.yaw - difference);
    }

    fn turn_mob_body(&mut self, input: &MotionInput) {
        let [dx, dy, dz] = input.delta;
        if dx * dx + dy * dy + dz * dz > MOB_MOVING_DISPLACEMENT_SQUARED {
            self.body_yaw += wrap_degrees(input.yaw - self.body_yaw)
                .clamp(-MOB_BODY_TURN_STEP, MOB_BODY_TURN_STEP);
            self.stable_head_yaw = input.head_yaw;
            self.stable_ticks = 0;
            return;
        }
        let limit = if wrap_degrees(input.head_yaw - self.stable_head_yaw).abs() > MOB_HEAD_RESTABLE
        {
            self.stable_head_yaw = input.head_yaw;
            self.stable_ticks = 0;
            MOB_HEAD_LIMIT
        } else {
            self.stable_ticks = self.stable_ticks.saturating_add(1);
            let settled = self.stable_ticks.saturating_sub(MOB_STABLE_TICKS) as f32;
            MOB_HEAD_LIMIT * (1.0 - settled / MOB_STABLE_TICKS as f32).max(0.0)
        };
        self.body_yaw =
            input.head_yaw + wrap_degrees(self.body_yaw - input.head_yaw).clamp(-limit, limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(delta: [f32; 3], player: bool, yaw: f32) -> MotionInput {
        MotionInput {
            delta,
            riding: false,
            player,
            yaw,
            head_yaw: yaw,
        }
    }

    #[test]
    fn walking_saturates_the_walk_speed_and_riding_zeroes_it() {
        let mut motion = MotionState::default();
        for _ in 0..40 {
            motion.advance(&input([0.3, 0.0, 0.0], false, 0.0));
        }
        assert!((motion.speed - 1.0).abs() < 1.0e-3);
        let distance = motion.distance;
        motion.advance(&MotionInput {
            riding: true,
            ..input([0.3, 0.0, 0.0], false, 0.0)
        });
        assert_eq!(motion.speed, 0.0);
        assert_eq!(motion.distance, distance);
    }

    #[test]
    fn swing_reads_zero_then_rises_by_sixths_then_idles() {
        let mut motion = MotionState::default();
        motion.start_swing(ACTOR_SWING_TICKS);
        let progress = (0..=ACTOR_SWING_TICKS)
            .map(|_| {
                motion.advance(&input([0.0; 3], true, 0.0));
                motion.attack_time()
            })
            .collect::<Vec<_>>();
        let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 0.0].map(|sixths| sixths / 6.0);
        assert_eq!(progress, expected);
    }

    #[test]
    fn a_second_swing_in_the_first_half_is_ignored() {
        let mut motion = MotionState::default();
        motion.start_swing(ACTOR_SWING_TICKS);
        motion.advance(&input([0.0; 3], true, 0.0));
        motion.advance(&input([0.0; 3], true, 0.0));
        motion.start_swing(ACTOR_SWING_TICKS);
        motion.advance(&input([0.0; 3], true, 0.0));
        assert_eq!(motion.attack_time(), 2.0 / 6.0);
    }

    /// Haste shortens the rendered swing to the same duration the packet guard uses.
    #[test]
    fn a_shortened_swing_completes_in_its_own_ticks() {
        let mut motion = MotionState::default();
        motion.start_swing(4);
        let progress = (0..=4)
            .map(|_| {
                motion.advance(&input([0.0; 3], true, 0.0));
                motion.attack_time()
            })
            .collect::<Vec<_>>();
        assert_eq!(progress, [0.0, 0.25, 0.5, 0.75, 0.0]);
    }

    #[test]
    fn player_body_turns_toward_movement_and_never_lags_the_head_past_the_soft_limit() {
        let mut motion = MotionState::spawn(0.0, 0.0);
        for _ in 0..60 {
            motion.advance(&input([0.2, 0.0, 0.0], true, -90.0));
        }
        assert!((motion.body_yaw + 90.0).abs() < 0.5, "moving +x faces -90");
        let mut idle = MotionState::spawn(0.0, 0.0);
        idle.advance(&input([0.0; 3], true, 120.0));
        let lag = wrap_degrees(120.0 - idle.body_yaw);
        assert!(lag <= 56.25 + 1.0e-3, "lag {lag}");
    }

    #[test]
    fn idle_mob_body_settles_onto_the_head() {
        let mut motion = MotionState::spawn(0.0, 60.0);
        for _ in 0..25 {
            motion.advance(&MotionInput {
                head_yaw: 60.0,
                ..input([0.0; 3], false, 0.0)
            });
        }
        assert!((motion.body_yaw - 60.0).abs() < 1.0e-3);
    }
}
