use std::collections::HashMap;

use super::*;

impl WorldAuthority {
    /// Retains canonical custom state identities independently of visual pack compilation.
    /// Sequential identities require the admitted internal range; hashed identities use their hash.
    pub fn set_custom_block_identities(&mut self, definitions: &CustomBlocks) {
        let mut identities = HashMap::new();
        let mut next = self.custom_block_ids.start;
        let mut skipped = 0_usize;
        for (index, block) in definitions.blocks.iter().enumerate() {
            let first = next;
            if self.network_id_mode == NetworkIdMode::Sequential {
                let Some(end) = next.checked_add(block.state_count) else {
                    skipped = skipped.saturating_add(definitions.blocks.len() - index);
                    break;
                };
                next = end;
                if end > self.custom_block_ids.end {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
            }
            let states = block.hashed_states();
            if states.len() != block.state_count as usize {
                skipped = skipped.saturating_add(1);
                continue;
            }
            for (offset, state) in states.into_iter().enumerate() {
                let internal = match self.network_id_mode {
                    NetworkIdMode::Sequential => first + offset as u32,
                    NetworkIdMode::Hashed => state.hash,
                };
                identities.entry(state.hash).or_insert(internal);
            }
        }
        eprintln!(
            "SESSION_CUSTOM_BLOCK_IDENTITIES session={} mode={:?} states={} skipped_definitions={skipped}",
            self.actor_session_id,
            self.network_id_mode,
            identities.len(),
        );
        self.custom_block_identities = Arc::new(identities);
    }
}
