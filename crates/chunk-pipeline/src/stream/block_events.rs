//! Presentation access to authoritative block-event cues.

pub use client_world::BlockEventCue;
use client_world::ingestion::BlockEventEvent;

use super::WorldStream;

impl WorldStream {
    /// Retains a block cue at its ordered commit position.
    pub(super) fn consume_block_event(&mut self, sequence: u64, event: BlockEventEvent) {
        self.authority.consume_block_event(sequence, event);
    }

    /// Clears dimension-scoped cues before the stream evicts the old dimension.
    pub(super) fn clear_block_events(&mut self) {
        self.authority.clear_block_events();
    }

    /// The latest cue at `position`, if any has arrived this dimension.
    #[must_use]
    pub fn block_event_cue(&self, position: [i32; 3]) -> Option<BlockEventCue> {
        self.authority.block_event_cue(position)
    }

    /// Cues replaced to admit a newer position.
    #[must_use]
    pub const fn replaced_block_events(&self) -> u64 {
        self.authority.replaced_block_events()
    }
}
