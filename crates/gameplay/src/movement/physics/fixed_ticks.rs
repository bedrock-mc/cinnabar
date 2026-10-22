use super::*;

impl LocalPhysicsController {
    /// Fixed ticks a frame of `elapsed` would complete, without advancing the clock.
    pub fn ticks_due(&self, elapsed: Duration) -> u64 {
        if self.state.is_none() || self.discard_next_elapsed {
            return 0;
        }
        whole_ticks(self.accumulated_seconds + elapsed.as_secs_f64())
    }
}

/// Admits whole ticks while keeping fractional elapsed time free of subtraction roundoff debt.
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
    *accumulated_seconds =
        (*accumulated_seconds - due as f64 * LOCAL_PHYSICS_TICK_SECONDS).max(0.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_tick_batches_do_not_accumulate_negative_roundoff() {
        let mut accumulated = 0.0;
        let mut discard = false;
        for _ in 0..16 {
            let frame = frame(
                simulation_clock::TICK_DURATION * 3,
                &mut accumulated,
                &mut discard,
            );
            assert_eq!(frame.due_ticks, 3);
            assert_eq!(frame.dropped_ticks, 0);
            assert_eq!(accumulated, 0.0);
        }
    }

    #[test]
    fn fractional_ticks_and_frame_limits_remain_intact() {
        let mut accumulated = 0.0;
        let mut discard = false;
        let half_tick = simulation_clock::TICK_DURATION / 2;
        assert_eq!(
            frame(half_tick, &mut accumulated, &mut discard).due_ticks,
            0
        );
        assert!(accumulated > 0.0);
        assert_eq!(
            frame(half_tick, &mut accumulated, &mut discard).due_ticks,
            1
        );
        assert_eq!(accumulated, 0.0);

        let ticks = simulation_clock::MAX_TICKS_PER_FRAME + 1;
        let frame = frame(
            simulation_clock::TICK_DURATION * ticks,
            &mut accumulated,
            &mut discard,
        );
        assert_eq!(frame.due_ticks, u64::from(ticks));
        assert_eq!(frame.dropped_ticks, 1);
        assert!(accumulated < LOCAL_PHYSICS_TICK_SECONDS);
    }
}
