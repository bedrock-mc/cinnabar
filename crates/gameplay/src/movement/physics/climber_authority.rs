//! Accepted climber flag edits retained at their pre-input replay boundary.

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ClimberCorrection {
    pub(super) tick: u64,
    word: u64,
    mask: u64,
}

impl ClimberCorrection {
    /// Applies changed contact bits without replacing unrelated replayed contact.
    pub(super) fn apply(self, state: &mut PlayerState) {
        ClimberFlags::from_word(self.word).apply(state, self.mask);
    }
}

impl LocalPhysicsController {
    /// Adopts a server's changed climber bits now and retains stamped edits for later replay.
    pub(super) fn adopt_climber_flags(&mut self, stamp: u64, word: u64) -> Option<u64> {
        let state = self.state.as_mut()?;
        let live = self.live_climber.unwrap_or_default().word();
        let oldest = self.history.oldest_tick().unwrap_or(state.tick);
        let anchor = stamp.max(oldest);
        let boundary = (stamp != 0).then(|| {
            if stamp > state.tick {
                state.tick
            } else {
                anchor.saturating_add(1)
            }
        });
        // A following capture holds the preceding completed tick's contact.
        let baseline = if stamp != 0 && anchor < state.tick {
            self.controller_history
                .iter()
                .find(|frame| frame.tick == anchor)
                .and_then(|frame| frame.climber)
                .map_or(live, ClimberFlags::word)
        } else {
            live
        };
        let mask = (baseline ^ word) & ClimberFlags::mask();
        let flags = ClimberFlags::from_word((live & !mask) | (word & mask));
        self.live_climber = Some(flags);
        flags.apply(state, mask);
        if let Some(tick) = boundary {
            self.climber_corrections.retain(|edit| edit.tick > oldest);
            if let Some(edit) = self
                .climber_corrections
                .iter_mut()
                .find(|edit| edit.tick == tick)
            {
                edit.word = (edit.word & !mask) | (word & mask);
                edit.mask |= mask;
            } else if mask != 0 {
                if self.climber_corrections.len() >= self.history_capacity {
                    self.climber_corrections.pop_front();
                }
                self.climber_corrections
                    .push_back(ClimberCorrection { tick, word, mask });
            }
        }
        (stamp != 0 && anchor < state.tick && mask != 0).then_some(anchor)
    }
}
