//! Latest server block-event cue per position; presentation interprets the values.

use std::collections::BTreeMap;

use protocol::BlockEventEvent;

use super::WorldAuthority;

/// Retention bound on cues; the oldest cue is replaced once it is full.
pub const MAX_RETAINED_BLOCK_EVENTS: usize = 4_096;

/// The most recent cue for one block position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockEventCue {
    pub event_type: i32,
    pub event_value: i32,
    /// Commit sequence of the packet; changes on every new cue, including repeats.
    pub sequence: u64,
}

#[derive(Default)]
pub(super) struct BlockEvents {
    cues: BTreeMap<[i32; 3], BlockEventCue>,
    replaced: u64,
}

impl BlockEvents {
    /// Retains the latest cue and evicts the oldest position when full.
    fn record(&mut self, position: [i32; 3], cue: BlockEventCue) {
        if !self.cues.contains_key(&position)
            && self.cues.len() >= MAX_RETAINED_BLOCK_EVENTS
            && let Some(oldest) = self
                .cues
                .iter()
                .min_by_key(|(_, cue)| cue.sequence)
                .map(|(&position, _)| position)
        {
            self.cues.remove(&oldest);
            self.replaced = self.replaced.saturating_add(1);
        }
        self.cues.insert(position, cue);
    }
}

impl WorldAuthority {
    /// Retains a current-dimension block cue at its committed sequence.
    pub fn consume_block_event(&mut self, sequence: u64, event: BlockEventEvent) {
        if event.dimension != self.current_dimension {
            return;
        }
        self.block_events.record(
            event.position,
            BlockEventCue {
                event_type: event.event_type,
                event_value: event.event_value,
                sequence,
            },
        );
    }

    /// Clears dimension-local cues before the coordinator retires old terrain.
    pub fn clear_block_events(&mut self) {
        self.block_events.cues.clear();
    }

    /// The latest cue at `position`, if any has arrived this dimension.
    #[must_use]
    pub fn block_event_cue(&self, position: [i32; 3]) -> Option<BlockEventCue> {
        self.block_events.cues.get(&position).copied()
    }

    /// Cues replaced to admit a newer position.
    #[must_use]
    pub const fn replaced_block_events(&self) -> u64 {
        self.block_events.replaced
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a cue with a chosen commit sequence.
    fn cue(sequence: u64) -> BlockEventCue {
        BlockEventCue {
            event_type: 1,
            event_value: 1,
            sequence,
        }
    }

    /// A long session must keep admitting new positions by replacing the oldest cue.
    #[test]
    fn full_retention_replaces_the_oldest_cue_and_known_positions_keep_updating() {
        let mut events = BlockEvents::default();
        for index in 0..MAX_RETAINED_BLOCK_EVENTS as i32 {
            events.record([index, 0, 0], cue(u64::try_from(index).unwrap() + 1));
        }
        events.record([0, 0, 0], cue(9_000));
        events.record([-1, 0, 0], cue(9_001));
        assert_eq!(events.cues.len(), MAX_RETAINED_BLOCK_EVENTS);
        assert_eq!(events.cues[&[-1, 0, 0]].sequence, 9_001);
        assert_eq!(events.cues[&[0, 0, 0]].sequence, 9_000);
        assert!(!events.cues.contains_key(&[1, 0, 0]));
        assert_eq!(events.replaced, 1);
    }
}
