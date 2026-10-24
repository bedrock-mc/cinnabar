//! Server climber writes move from pending state to the input frame that consumes them.

use super::*;

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
    /// Replaces live contact and unconsumed writes when ordered authority starts a new actor.
    pub(super) fn replace_climber_authority(&mut self, flags: client_world::MovementFlagUpdate) {
        let word = flags.extended_flags.unwrap_or(0);
        let write = ClimberWrite {
            word,
            mask: ClimberFlags::mask(),
        };
        self.live_climber = Some(ClimberFlags::from_word(word));
        self.pending_climber = write;
        if let Some(state) = self.state.as_mut() {
            write.apply(state);
        }
    }

    /// Adopts changed climber bits now and retains their input boundary for later replay.
    pub(super) fn adopt_climber_flags(&mut self, stamp: u64, word: u64) -> Option<u64> {
        let state = self.state.as_mut()?;
        let live = self.live_climber.unwrap_or_default().word();
        let oldest = self.history.oldest_tick().unwrap_or(state.tick);
        let anchor = stamp.max(oldest);
        let consumed = if stamp != 0 && anchor < state.tick {
            self.controller_history
                .iter_mut()
                .find(|frame| frame.tick == anchor.saturating_add(1))
        } else {
            None
        };
        // Completed inputs compare with their original live entry, including accepted writes.
        let baseline = consumed
            .as_ref()
            .map_or(live, |frame| frame.entry_climber.unwrap_or_default().word());
        let mask = (baseline ^ word) & ClimberFlags::mask();
        let flags = ClimberFlags::from_word((live & !mask) | (word & mask));
        self.live_climber = Some(flags);
        flags.apply(state, mask);
        let write = ClimberWrite { word, mask };
        if let Some(frame) = consumed {
            frame.climber_write.merge(write);
            self.pending_climber.supersede(mask);
            (mask != 0).then_some(anchor)
        } else {
            self.pending_climber.merge(write);
            None
        }
    }
}
