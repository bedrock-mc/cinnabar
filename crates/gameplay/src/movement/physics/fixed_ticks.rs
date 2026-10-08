use super::*;

pub(super) fn frame(
    elapsed: Duration,
    accumulated_seconds: &mut f64,
    discard_next_elapsed: &mut bool,
) -> LocalPhysicsFrame {
    if std::mem::take(discard_next_elapsed) {
        return LocalPhysicsFrame::default();
    }
    *accumulated_seconds += elapsed.as_secs_f64();
    let due = whole_ticks(*accumulated_seconds);
    *accumulated_seconds -= due as f64 * LOCAL_PHYSICS_TICK_SECONDS;
    let allowed = due.min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;
    LocalPhysicsFrame {
        due_ticks: due,
        dropped_ticks: due.saturating_sub(allowed as u64),
        samples: Vec::with_capacity(allowed),
        ..LocalPhysicsFrame::default()
    }
}

/// Whole fixed ticks contained in accumulated seconds.
pub(super) fn whole_ticks(accumulated_seconds: f64) -> u64 {
    ((accumulated_seconds + f64::EPSILON) / LOCAL_PHYSICS_TICK_SECONDS)
        .floor()
        .clamp(0.0, u64::MAX as f64) as u64
}
