//! Shared Bedrock 1.12 combat presentation rules.

/// Scalar rules shared by camera sampling and shader uploads.
#[derive(Clone, Copy, Debug)]
pub struct Bedrock112Rules {
    pub hurt_overlay: [f32; 4],
    pub hurt_ticks: u8,
    pub glint_period_ms: [i32; 2],
    pub glint_rotation_radians: [f32; 2],
    pub walk_gain: f32,
    pub bob_cap: f32,
    pub bob_follow: f32,
    pub fall_follow: f32,
    pub fall_velocity_gain: f32,
    pub fall_degrees_gain: f32,
    pub horizontal_gain: f32,
    pub roll_radians: f32,
    pub pitch_radians: f32,
    pub pitch_phase: f32,
    pub hurt_radians: f32,
    pub death_numerator_radians: f32,
    pub death_offset_radians: f32,
    pub death_tick_offset: f32,
    pub sine_step: f32,
    pub sine_index_scale: f32,
}

/// Bedrock 1.12 rules. The profile changes presentation only.
pub const BEDROCK_112: Bedrock112Rules = Bedrock112Rules {
    hurt_overlay: [1.0, 0.0, 0.0, 0.6],
    hurt_ticks: 10,
    glint_period_ms: [1750, 3000],
    glint_rotation_radians: [
        -20.0 * std::f32::consts::PI / 180.0,
        80.0 * std::f32::consts::PI / 180.0,
    ],
    walk_gain: 0.6,
    bob_cap: 0.1,
    bob_follow: 0.4,
    fall_follow: 0.8,
    fall_velocity_gain: -0.2,
    fall_degrees_gain: 15.0,
    horizontal_gain: 0.65,
    roll_radians: 0.052_359_88,
    pitch_radians: 0.087_266_46,
    pitch_phase: -0.2,
    hurt_radians: -0.244_346_1,
    death_numerator_radians: -139.626_34,
    death_offset_radians: 0.698_131_7,
    death_tick_offset: 200.0,
    sine_step: f32::from_bits(0x38c9_0fdb),
    sine_index_scale: 10_430.378,
};

/// Selects the explicit combat profile independently of Java pose selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombatVisuals {
    Modern,
    Legacy112,
}

/// One code-level choice for all consumers; Java still owns its poses and items.
pub const DEFAULT_COMBAT_VISUALS: CombatVisuals = CombatVisuals::Legacy112;

impl Default for CombatVisuals {
    /// Follows the shared code-level profile without a saved user preference.
    fn default() -> Self {
        DEFAULT_COMBAT_VISUALS
    }
}
