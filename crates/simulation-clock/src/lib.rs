//! Shared simulation clock values and duration conversion, without a world-model dependency.

/// Simulation ticks per second.
pub const TICKS_PER_SECOND: u32 = 20;

/// Whole-tick frame limit; a frame further behind discards the excess ticks.
pub const MAX_TICKS_PER_FRAME: u32 = 10;

/// Maximum elapsed time admitted by the client timer in one rendered frame.
pub const MAX_FRAME_ELAPSED: std::time::Duration = std::time::Duration::from_millis(100);

/// Duration of one simulation tick.
pub const TICK_DURATION: std::time::Duration = std::time::Duration::from_nanos(
    std::time::Duration::from_secs(1).as_nanos() as u64 / TICKS_PER_SECOND as u64,
);

/// Converts nonnegative float seconds to whole simulation ticks, discarding fractions.
/// Non-finite or unrepresentable results remain unresolved instead of saturating.
pub fn whole_ticks_from_seconds(seconds: f32) -> Option<u32> {
    let ticks = seconds * TICKS_PER_SECOND as f32;
    (ticks.is_finite() && ticks >= 0.0 && f64::from(ticks) <= f64::from(u32::MAX))
        .then_some(ticks as u32)
}
