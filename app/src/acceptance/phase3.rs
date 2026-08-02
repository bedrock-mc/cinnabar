use std::time::Instant;

use super::{AcceptanceRun, TRANSPARENT_PRESENTATION_EXIT_GRACE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase3TerminalDrainDecision {
    Drained,
    Wait,
    TimedOut,
}

impl AcceptanceRun {
    pub(crate) fn deadline_reached(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn phase3_terminal_drain_decision(
        &self,
        now: Instant,
        candidate_physics: bool,
        pending_count: usize,
    ) -> Phase3TerminalDrainDecision {
        if self.shutdown_requested {
            return Phase3TerminalDrainDecision::Drained;
        }
        if !candidate_physics || pending_count == 0 {
            return Phase3TerminalDrainDecision::Drained;
        }
        let Some(deadline) = self.deadline else {
            return Phase3TerminalDrainDecision::Wait;
        };
        let drain_deadline = deadline
            .checked_add(TRANSPARENT_PRESENTATION_EXIT_GRACE)
            .expect("Phase 3 terminal drain deadline overflowed");
        if now < drain_deadline {
            Phase3TerminalDrainDecision::Wait
        } else {
            Phase3TerminalDrainDecision::TimedOut
        }
    }
}
