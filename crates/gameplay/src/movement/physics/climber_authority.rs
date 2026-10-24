//! Accepted climber flag edits retained at their pre-input replay boundary.

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ClimberCorrection {
    pub(super) tick: u64,
    pub(super) write: ClimberWrite,
}

/// Changed climber bits, composed in arrival order without replacing untouched contact.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ClimberWrite {
    word: u64,
    mask: u64,
}

impl ClimberWrite {
    /// Combines accepted edits, allowing a later clear to replace an earlier set.
    pub(super) fn merge(&mut self, update: Self) {
        self.word = (self.word & !update.mask) | (update.word & update.mask);
        self.mask |= update.mask;
    }

    /// Removes edits whose newer correction will be consumed by retained input replay.
    fn supersede(&mut self, mask: u64) {
        self.mask &= !mask;
    }

    /// Overlays the supplied bits on the flags captured before local input.
    pub(super) fn apply_flags(self, flags: Option<ClimberFlags>) -> Option<ClimberFlags> {
        let word = flags.unwrap_or_default().word();
        if self.mask == 0 {
            flags
        } else {
            Some(ClimberFlags::from_word(
                (word & !self.mask) | (self.word & self.mask),
            ))
        }
    }

    /// Applies changed contact bits without replacing unrelated replayed contact.
    pub(super) fn apply(self, state: &mut PlayerState) {
        if self.mask != 0 {
            ClimberFlags::from_word(self.word).apply(state, self.mask);
        }
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
        // The following capture includes live writes accepted between completed ticks.
        let baseline = if stamp != 0 && anchor < state.tick {
            self.controller_history
                .iter()
                .find(|frame| frame.tick == anchor.saturating_add(1))
                .and_then(|frame| frame.entry_climber)
                .map_or(live, ClimberFlags::word)
        } else {
            live
        };
        let mask = (baseline ^ word) & ClimberFlags::mask();
        let flags = ClimberFlags::from_word((live & !mask) | (word & mask));
        self.live_climber = Some(flags);
        flags.apply(state, mask);
        let write = ClimberWrite { word, mask };
        if boundary.is_none_or(|tick| tick > state.tick) {
            self.sync_state.pending_climber.merge(write);
        } else {
            self.sync_state.pending_climber.supersede(mask);
        }
        if let Some(tick) = boundary {
            self.climber_corrections.retain(|edit| edit.tick > oldest);
            if let Some(edit) = self
                .climber_corrections
                .iter_mut()
                .find(|edit| edit.tick == tick)
            {
                edit.write.merge(write);
            } else if mask != 0 {
                if self.climber_corrections.len() >= self.history_capacity {
                    self.climber_corrections.pop_front();
                }
                self.climber_corrections
                    .push_back(ClimberCorrection { tick, write });
            }
        }
        (stamp != 0 && anchor < state.tick && mask != 0).then_some(anchor)
    }
}
