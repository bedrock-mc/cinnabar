/// Client ticks from correction to sync.
const SYNC_DELAY_TICKS: u16 = 200;

#[derive(Debug, Clone, Default)]
pub(in crate::movement) struct PredictionSyncCountdown(Option<u16>);

impl PredictionSyncCountdown {
    /// Arms an idle countdown without postponing a pending sync.
    pub(in crate::movement) fn arm(&mut self) {
        self.0.get_or_insert(SYNC_DELAY_TICKS);
    }

    /// Counts a newly completed client tick, never a correction replay.
    pub(in crate::movement) fn tick(&mut self) {
        if let Some(remaining) = &mut self.0 {
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Keeps a due sync pending until the outbound queue accepts it.
    pub(in crate::movement) fn due(&self) -> bool {
        self.0 == Some(0)
    }

    /// Retires a sent sync or a discarded session.
    pub(in crate::movement) fn clear(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_only_after_two_hundred_ticks_and_does_not_postpone_on_corrections() {
        let mut timer = PredictionSyncCountdown::default();
        for _ in 0..300 {
            timer.tick();
        }
        assert!(!timer.due());
        timer.arm();
        assert!(!timer.due());
        for _ in 1..SYNC_DELAY_TICKS {
            timer.tick();
            timer.arm();
            assert!(!timer.due());
        }
        timer.tick();
        assert!(timer.due());
        timer.tick();
        timer.arm();
        assert!(timer.due());
        timer.clear();
        assert!(!timer.due());
        timer.arm();
        assert!(!timer.due());
    }
}
