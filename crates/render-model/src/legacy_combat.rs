//! Bedrock 1.12 camera and swing effects, independent of gameplay aim and rendering cadence.

use glam::{Mat4, Vec3};
use render_api::BEDROCK_112;
use std::sync::OnceLock;

mod swing;
pub use swing::{LegacySwingPose, legacy_first_person_arm, legacy_item_swing, legacy_swing_pose};

/// A completed movement tick. `movement` is collision-resolved displacement, not a request.
#[derive(Debug, Clone, Copy)]
pub struct LegacyCameraTick {
    pub tick: u64,
    pub position: [f32; 3],
    pub movement: [f32; 3],
    pub velocity: [f32; 3],
    pub on_ground: bool,
    pub alive: bool,
    pub riding: bool,
    pub walks: bool,
}

/// Previous/current endpoints used to sample an entire view effect at one partial tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LegacyCameraEndpoints {
    pub walked: [f32; 2],
    pub bob: [f32; 2],
    pub fall_degrees: [f32; 2],
}

/// Retains camera motion only; simulation state is read without modification.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LegacyCameraState {
    last: Option<(u64, Vec3)>,
    pub endpoints: LegacyCameraEndpoints,
}

const TELEPORT_BLOCKS: f32 = 8.0;
const TABLE_SIZE: usize = 1 << 16;
static SINE_TABLE: OnceLock<Box<[f32]>> = OnceLock::new();

/// Initializes the shared lookup before camera frames begin, avoiding first-hit work.
pub fn prepare_trigonometry() {
    let _ = table_sample(0.0);
}

/// Looks up binary32 sine at a truncated, wrapped Bedrock table index.
pub fn legacy_sin(radians: f32) -> f32 {
    table_sample(radians * BEDROCK_112.sine_index_scale)
}

/// Adds the quarter-turn before truncation, including negative angles.
pub fn legacy_cos(radians: f32) -> f32 {
    table_sample(radians * BEDROCK_112.sine_index_scale + (TABLE_SIZE / 4) as f32)
}

/// Builds the float table once and wraps the caller's signed index.
fn table_sample(index: f32) -> f32 {
    let table = SINE_TABLE.get_or_init(|| {
        (0..TABLE_SIZE)
            .map(|index| (index as f32 * BEDROCK_112.sine_step).sin())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    });
    table[((index as i32) & (TABLE_SIZE as i32 - 1)) as usize]
}

impl LegacyCameraState {
    /// Reanchors presentation to a corrected simulation position without replaying old motion.
    pub fn reset_at(&mut self, tick: u64, position: [f32; 3]) {
        let position = Vec3::from_array(position);
        *self = Self {
            last: position.is_finite().then_some((tick, position)),
            endpoints: LegacyCameraEndpoints::default(),
        };
    }
    /// Returns the completed tick cursor for a borrowed movement history.
    pub fn last_tick(&self) -> Option<u64> {
        self.last.map(|(tick, _)| tick)
    }

    /// Steps each completed tick once; rewinds, missing ticks and teleports reset motion.
    pub fn advance(&mut self, tick: LegacyCameraTick) {
        let position = Vec3::from_array(tick.position);
        let movement = Vec3::from_array(tick.movement);
        let velocity = Vec3::from_array(tick.velocity);
        if !position.is_finite() || !movement.is_finite() || !velocity.is_finite() {
            *self = Self::default();
            return;
        }
        let Some((previous_tick, previous_position)) = self.last else {
            self.last = Some((tick.tick, position));
            return;
        };
        if position.distance(previous_position) > TELEPORT_BLOCKS {
            self.reset_at(tick.tick, tick.position);
            return;
        }
        if tick.tick == previous_tick {
            return;
        }
        self.last = Some((tick.tick, position));
        if tick.tick != previous_tick.saturating_add(1) {
            self.endpoints = LegacyCameraEndpoints::default();
            return;
        }
        let rules = BEDROCK_112;
        let state = &mut self.endpoints;
        state.walked[0] = state.walked[1];
        state.bob[0] = state.bob[1];
        state.fall_degrees[0] = state.fall_degrees[1];
        if tick.walks {
            let distance = (movement.x * movement.x + movement.z * movement.z).sqrt();
            state.walked[1] += rules.walk_gain * distance;
        }
        let speed = (velocity.x * velocity.x + velocity.z * velocity.z).sqrt();
        let target_bob = if tick.on_ground && tick.alive && !tick.riding {
            speed.min(rules.bob_cap)
        } else {
            0.0
        };
        let target_fall = if !tick.on_ground && tick.alive {
            rules.fall_degrees_gain * (rules.fall_velocity_gain * velocity.y).atan()
        } else {
            0.0
        };
        state.bob[1] += rules.bob_follow * (target_bob - state.bob[1]);
        state.fall_degrees[1] += rules.fall_follow * (target_fall - state.fall_degrees[1]);
    }
}

impl LegacyCameraEndpoints {
    /// Extrapolates walking phase and interpolates amplitude/tilt, preserving rotation order.
    pub fn bob_matrix(self, alpha: f32) -> Mat4 {
        let alpha = partial_tick(alpha);
        let rules = BEDROCK_112;
        let [previous, current] = self.walked;
        let phase = -std::f32::consts::PI * (current + (current - previous) * alpha);
        let bob = interpolate(self.bob, alpha);
        let fall = interpolate(self.fall_degrees, alpha);
        let sine = legacy_sin(phase);
        Mat4::from_translation(Vec3::new(
            rules.horizontal_gain * bob * sine,
            -(bob * legacy_cos(phase)).abs(),
            0.0,
        )) * Mat4::from_rotation_z(rules.roll_radians * bob * sine)
            * Mat4::from_rotation_x(
                rules.pitch_radians * (bob * legacy_cos(phase + rules.pitch_phase)).abs(),
            )
            * Mat4::from_rotation_x(fall.to_radians())
    }
}

/// Samples the actor's tick counters; strength affects hurt angle without scaling death roll.
pub fn legacy_hurt_matrix(
    hurt_ticks: u8,
    direction_degrees: f32,
    death_ticks: Option<f32>,
    alpha: f32,
    strength: f32,
) -> Mat4 {
    let rules = BEDROCK_112;
    let alpha = partial_tick(alpha);
    let mut effect =
        death_ticks
            .filter(|ticks| ticks.is_finite())
            .map_or(Mat4::IDENTITY, |ticks| {
                Mat4::from_rotation_z(
                    rules.death_numerator_radians / (ticks + alpha + rules.death_tick_offset)
                        + rules.death_offset_radians,
                )
            });
    if hurt_ticks > 0 && direction_degrees.is_finite() {
        let progress = (f32::from(hurt_ticks) - alpha) / f32::from(rules.hurt_ticks);
        let squared = progress * progress;
        let shake = legacy_sin((squared * squared) * std::f32::consts::PI);
        let strength = if strength.is_finite() {
            strength.clamp(0.0, render_api::MAX_HURT_CAMERA_INTENSITY)
        } else {
            1.0
        };
        let direction = direction_degrees.to_radians();
        effect *= Mat4::from_rotation_y(-direction)
            * Mat4::from_rotation_z(rules.hurt_radians * shake * strength)
            * Mat4::from_rotation_y(direction);
    }
    effect
}

/// Sanitizes a render fraction without changing tick-owned values.
pub fn partial_tick(alpha: f32) -> f32 {
    if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Interpolates one retained pair at a validated render fraction.
fn interpolate([previous, current]: [f32; 2], alpha: f32) -> f32 {
    previous + (current - previous) * alpha
}

#[cfg(test)]
mod tests;
