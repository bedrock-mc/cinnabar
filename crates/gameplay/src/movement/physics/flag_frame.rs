//! Actor flags captured and corrected at a movement input boundary.

use super::*;

/// Pose state and climber writes consumed before one completed input.
#[derive(Debug, Clone)]
pub(super) struct FrameFlags {
    pub pose_entry: server_flags::FlagWord,
    pub pose_correction: server_flags::FlagCorrection,
    counters: server_flags::EntryCounters,
    /// Original live contact, including accepted writes; replay never changes this comparison.
    pub captured_climber: Option<ClimberFlags>,
    pub climber_write: climber_authority::ClimberWrite,
}

impl FrameFlags {
    /// Captures live flags and counters without consuming writes before the input succeeds.
    pub(super) fn capture(
        authority: &mut server_flags::ServerFlagAuthority,
        modes: &ModeTracker,
        state: &PlayerState,
        input: &mut super::super::TickInput,
        climber: Option<ClimberFlags>,
    ) -> Self {
        let (pose_entry, pose_correction, counters) =
            server_flags::capture_tick(authority, modes, state, input);
        Self {
            pose_entry,
            pose_correction,
            counters,
            captured_climber: climber,
            climber_write: Default::default(),
        }
    }

    /// Returns the pose and spin word after this input's server corrections.
    pub(super) fn pose_word(&self) -> server_flags::FlagWord {
        self.pose_correction.apply(self.pose_entry)
    }

    /// Restores the first snapshot or refreshes later pose state, then applies all
    /// consumed writes before local pose selection and the climber contact scan.
    pub(super) fn replay(
        &mut self,
        modes: &mut ModeTracker,
        state: &mut PlayerState,
        restore: bool,
        spun: bool,
        pending: climber_authority::ClimberWrite,
        climber: &mut Option<ClimberFlags>,
    ) {
        let dormant = if restore {
            self.counters.restore(modes, state)
        } else {
            self.pose_entry = server_flags::FlagWord::capture(
                modes.pose_flags(),
                state.spin_attack_ticks.is_some(),
            );
            let dormant = server_flags::dormant_spin(spun, state);
            self.counters = server_flags::EntryCounters::capture(modes, state, dormant);
            dormant
        };
        server_flags::impose(self.pose_entry, self.pose_correction, modes, state, dormant);
        self.climber_write.merge(pending);
        *climber = self.climber_write.apply_flags(*climber);
        if let Some(flags) = *climber {
            flags.apply(state, ClimberFlags::mask());
        }
    }
}
