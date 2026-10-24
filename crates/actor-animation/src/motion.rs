//! Per-actor walk cycle, arm swing, fish phase, and body rotation that vanilla animations read through
//! queries. Player walk cycles use horizontal movement; body turns remain independent.
//! The jumping walk multiplier remains incomplete.
use super::query::wrap_degrees;

/// Ticks one arm swing takes without haste or fatigue.
pub const ACTOR_SWING_TICKS: i32 = 6;
const WALK_STEP_GAIN: f32 = 1.6;
const WALK_STEP_TARGET_MAX: f32 = 0.4;
const WALK_TURN_GAIN: f32 = 0.02;
const WALK_TURN_TARGET_MAX: f32 = 0.2;
const WALK_SPEED_RETAIN: f32 = 0.6;
/// Burning actors walk their limb cycle half again as fast.
const WALK_ON_FIRE_SCALE: f32 = 1.5;
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
// Vanilla fish animation consumes actor velocity in blocks/tick.
const FISH_PHASE_SPEED_GAIN: f32 = 0.1;

/// Wire amplifiers of the effects that change how long an arm swing lasts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwingEffects {
    pub haste: Option<i32>,
    pub mining_fatigue: Option<i32>,
}

impl SwingEffects {
    /// Swing length in ticks: each Haste level takes one tick off and wins over Mining Fatigue,
    /// whose levels add two each. Negative (untrusted) amplifiers count as absent.
    #[must_use]
    pub fn ticks(self) -> i32 {
        let level = |amplifier: Option<i32>| {
            amplifier
                .filter(|value| *value >= 0)
                .map(|value| value.saturating_add(1))
        };
        let ticks = match (level(self.haste), level(self.mining_fatigue)) {
            (Some(haste), _) => ACTOR_SWING_TICKS.saturating_sub(haste),
            (None, Some(fatigue)) => ACTOR_SWING_TICKS.saturating_add(fatigue.saturating_mul(2)),
            (None, None) => ACTOR_SWING_TICKS,
        };
        ticks.max(1)
    }

    /// Bedrock swing length for `visuals`: the 1.12 profile shortens it only from Haste II, by
    /// one tick per level past the first, and otherwise lengthens it for Mining Fatigue.
    #[must_use]
    pub fn bedrock_ticks(self, visuals: render_api::CombatVisuals) -> i32 {
        if visuals == render_api::CombatVisuals::Modern {
            return self.ticks();
        }
        let amplifier = |value: Option<i32>| value.filter(|value| *value >= 0);
        let ticks = match (amplifier(self.haste), amplifier(self.mining_fatigue)) {
            (Some(haste), _) if haste >= 1 => (ACTOR_SWING_TICKS - 1).saturating_sub(haste),
            (_, Some(fatigue)) => {
                ACTOR_SWING_TICKS.saturating_add(fatigue.saturating_add(1).saturating_mul(2))
            }
            _ => ACTOR_SWING_TICKS,
        };
        ticks.max(1)
    }

    /// Applies one MobEffect change, ignoring effects that leave swings alone.
    pub fn apply(&mut self, effect_id: i32, amplifier: Option<i32>) {
        match effect_id {
            protocol::HASTE_EFFECT_ID => self.haste = amplifier,
            protocol::MINING_FATIGUE_EFFECT_ID => self.mining_fatigue = amplifier,
            _ => {}
        }
    }
}

/// One tick of actor state the motion model consumes.
pub(super) struct MotionInput {
    pub(super) delta: [f32; 3],
    pub(super) riding: bool,
    pub(super) player: bool,
    pub(super) yaw: f32,
    pub(super) head_yaw: f32,
    pub(super) on_fire: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MotionState {
    pub(super) distance: f32,
    pub(super) speed: f32,
    /// Swing counter; `-1` marks a swing requested since the last tick.
    swing: Option<i32>,
    local_swing: Option<f32>,
    /// Length of the current swing in ticks, after haste and fatigue.
    swing_ticks: i32,
    pub(super) body_yaw: f32,
    pub(super) previous_body_yaw: f32,
    stable_head_yaw: f32,
    stable_ticks: u32,
    /// Stride accumulator behind `query.walk_distance`.
    stride: f32,
    /// Fish animation phase survives geometry/controller resets for this actor lifetime.
    fish_phase: [f32; 2],
    pub(super) horse: super::horse::AnimationState,
    /// Shulker lid-open fraction, stepped each tick toward the synced peek target.
    pub(super) shulker_peek: f32,
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
            .is_some_and(|counter| counter >= 0 && counter < ticks.max(1) / 2)
        {
            return;
        }
        self.swing = Some(-1);
        self.swing_ticks = ticks.max(1);
    }

    /// Effect changes alter the current denominator without restarting the swing.
    pub(super) fn set_swing_duration(&mut self, ticks: i32) {
        self.swing_ticks = ticks.max(1);
    }

    /// Locally committed samples replace this actor clock's swing advancement.
    pub(super) fn set_local_swing(&mut self, progress: Option<f32>) {
        self.local_swing = progress;
    }

    pub(super) fn walk_distance(self) -> f32 {
        self.stride * STRIDE_READ_SCALE
    }

    pub(super) fn attack_time(self) -> f32 {
        if let Some(progress) = self.local_swing {
            return progress;
        }
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
        if self.local_swing.is_none() {
            self.swing = self
                .swing
                .map(|counter| counter + 1)
                .filter(|counter| *counter < self.swing_ticks);
        }
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
        let mut target = if step != 0.0 || input.player {
            (WALK_STEP_GAIN * step).min(WALK_STEP_TARGET_MAX)
        } else {
            let turn = wrap_degrees(self.body_yaw - self.previous_body_yaw).abs();
            (WALK_TURN_GAIN * turn).min(WALK_TURN_TARGET_MAX)
        };
        if input.on_fire {
            target *= WALK_ON_FIRE_SCALE;
        }
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
            on_fire: false,
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

    /// Burning walkers settle one and a half times faster through the limb cycle.
    #[test]
    fn burning_walkers_advance_the_walk_cycle_half_again_as_fast() {
        let (mut cool, mut burning) = (MotionState::default(), MotionState::default());
        for _ in 0..40 {
            cool.advance(&input([0.3, 0.0, 0.0], false, 0.0));
            burning.advance(&MotionInput {
                on_fire: true,
                ..input([0.3, 0.0, 0.0], false, 0.0)
            });
        }
        assert!((burning.speed - 1.5 * cool.speed).abs() < 1.0e-3);
        assert!((burning.distance - 1.5 * cool.distance).abs() < 1.0e-3);
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
    fn stationary_player_turns_do_not_advance_walk_cycle() {
        let mut motion = MotionState::spawn(0.0, 0.0);
        for yaw in [30.0, 90.0, 120.0, 179.0, -179.0, -90.0, 0.0] {
            motion.advance(&input([0.0; 3], true, yaw));
            assert_eq!(motion.speed, 0.0, "yaw {yaw}");
            assert_eq!(motion.distance, 0.0, "yaw {yaw}");
            assert_eq!(motion.walk_distance(), 0.0, "yaw {yaw}");
        }
        assert_ne!(motion.body_yaw, 0.0, "the body still follows the head");
    }

    #[test]
    fn turning_after_a_player_stops_does_not_restart_walking() {
        let mut straight = MotionState::spawn(0.0, 0.0);
        straight.advance(&input([0.2, 0.0, 0.0], true, 0.0));
        let mut turning = straight;
        for yaw in [120.0, -120.0, 179.0, -179.0] {
            let previous_speed = straight.speed;
            straight.advance(&input([0.0; 3], true, 0.0));
            turning.advance(&input([0.0, 0.5, 0.0], true, yaw));
            assert!(straight.speed < previous_speed);
            assert_eq!(turning.speed, straight.speed, "yaw {yaw}");
            assert_eq!(turning.distance, straight.distance, "yaw {yaw}");
            assert_eq!(turning.walk_distance(), straight.walk_distance());
        }
    }

    #[test]
    fn player_walk_speed_uses_even_small_horizontal_steps() {
        let mut creeping = MotionState::spawn(0.0, 0.0);
        creeping.advance(&input([1.0e-4, 0.0, 0.0], true, 90.0));
        assert!(
            (creeping.speed - 1.6e-4).abs() < 1.0e-9,
            "{}",
            creeping.speed
        );
    }

    #[test]
    fn stationary_mobs_retain_body_turn_walk_cycle() {
        let mut motion = MotionState::spawn(0.0, 0.0);
        motion.advance(&input([0.0; 3], false, 120.0));
        assert_ne!(motion.body_yaw, 0.0);
        assert!(motion.speed > 0.0);
        assert!(motion.distance > 0.0);
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

    /// Haste takes a tick per level and wins over Mining Fatigue's two; removal restores six.
    #[test]
    fn swing_effects_set_the_swing_length() {
        let mut effects = SwingEffects::default();
        assert_eq!(effects.ticks(), 6);
        effects.apply(protocol::MINING_FATIGUE_EFFECT_ID, Some(0));
        assert_eq!(effects.ticks(), 8);
        effects.apply(protocol::HASTE_EFFECT_ID, Some(1));
        assert_eq!(effects.ticks(), 4);
        effects.apply(protocol::HASTE_EFFECT_ID, Some(9));
        assert_eq!(effects.ticks(), 1);
        effects.apply(protocol::HASTE_EFFECT_ID, None);
        effects.apply(8, Some(3));
        assert_eq!(effects.ticks(), 8);
        effects.apply(protocol::MINING_FATIGUE_EFFECT_ID, Some(-1));
        assert_eq!(effects.ticks(), 6);
    }

    /// The 1.12 profile keeps six ticks under Haste I, then takes one per further level.
    #[test]
    fn legacy_swing_length_starts_shortening_at_haste_two() {
        let legacy = render_api::CombatVisuals::Legacy112;
        let effects = |haste, mining_fatigue| SwingEffects {
            haste,
            mining_fatigue,
        };
        for (haste, fatigue, ticks) in [
            (None, None, 6),
            (Some(0), None, 6),
            (Some(1), None, 4),
            (Some(2), None, 3),
            (Some(9), None, 1),
            (None, Some(0), 8),
            (None, Some(1), 10),
            (Some(0), Some(0), 8),
            (Some(1), Some(0), 4),
        ] {
            assert_eq!(
                effects(haste, fatigue).bedrock_ticks(legacy),
                ticks,
                "haste {haste:?}, fatigue {fatigue:?}"
            );
            assert_eq!(
                effects(haste, fatigue).bedrock_ticks(render_api::CombatVisuals::Modern),
                effects(haste, fatigue).ticks()
            );
        }
    }
}
