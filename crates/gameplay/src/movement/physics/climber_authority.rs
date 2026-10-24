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

/// An accepted write awaiting its first successful live or corrected input.
#[derive(Debug, Clone, Copy)]
struct PendingClimberWrite {
    replay_tick: Option<u64>,
    write: ClimberWrite,
}

/// Arrival-ordered writes stay pending until an input successfully consumes them.
#[derive(Debug, Clone, Default)]
pub(super) struct PendingClimberWrites(Vec<PendingClimberWrite>);

impl PendingClimberWrites {
    /// Queues changed bits, optionally offering them to a retained historical input first.
    fn push(&mut self, replay_tick: Option<u64>, write: ClimberWrite) {
        if write.mask != 0 {
            self.0.push(PendingClimberWrite { replay_tick, write });
        }
    }

    /// Transfers all still-pending writes to the successful live input that consumed them.
    pub(super) fn consume_live(&mut self) -> ClimberWrite {
        let mut consumed = ClimberWrite::default();
        for pending in self.0.drain(..) {
            consumed.merge(pending.write);
        }
        consumed
    }

    /// Offers historical writes to this replay input without committing their consumption.
    pub(super) fn for_replay(&self, tick: u64) -> ClimberWrite {
        let mut consumed = ClimberWrite::default();
        for pending in &self.0 {
            if pending.replay_tick == Some(tick) {
                consumed.merge(pending.write);
            }
        }
        consumed
    }

    /// Retires successfully replayed writes and any older pending bits they superseded.
    pub(super) fn complete_replay(&mut self, anchor: u64, through: u64) {
        let mut consumed_mask = 0;
        for pending in self.0.iter_mut().rev() {
            if pending
                .replay_tick
                .is_some_and(|tick| tick > anchor && tick <= through)
            {
                consumed_mask |= pending.write.mask;
                pending.write.mask = 0;
            } else {
                pending.write.supersede(consumed_mask);
            }
        }
        self.0.retain(|pending| pending.write.mask != 0);
    }

    /// Restores unconsumed server bits over the last completed contact report.
    pub(super) fn apply_flags(&self, mut flags: Option<ClimberFlags>) -> Option<ClimberFlags> {
        for pending in &self.0 {
            flags = pending.write.apply_flags(flags);
        }
        flags
    }

    /// Restores the same unconsumed bits to the simulator after a replay.
    pub(super) fn apply(&self, state: &mut PlayerState) {
        for pending in &self.0 {
            pending.write.apply(state);
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
        self.pending_climber = Default::default();
        self.pending_climber.push(None, write);
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
        let historical = if stamp != 0 && anchor < state.tick {
            self.controller_history
                .iter()
                .find(|frame| frame.tick == anchor.saturating_add(1))
        } else {
            None
        };
        // Completed inputs compare with their original live entry, including accepted writes.
        let baseline = historical
            .as_ref()
            .map_or(live, |frame| frame.entry_climber.unwrap_or_default().word());
        let mask = (baseline ^ word) & ClimberFlags::mask();
        let flags = ClimberFlags::from_word((live & !mask) | (word & mask));
        self.live_climber = Some(flags);
        flags.apply(state, mask);
        let write = ClimberWrite { word, mask };
        self.pending_climber
            .push(historical.map(|frame| frame.tick), write);
        (historical.is_some() && mask != 0).then_some(anchor)
    }
}
