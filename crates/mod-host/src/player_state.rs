//! Bounded, read-only local player facts, retained for exactly one callback.

use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{PlayerStateItem, PlayerStateSlot, PlayerStateSnapshot};
use anyhow::{Result, bail, ensure};
use mod_api::{
    MAX_ITEM_IDENTIFIER_BYTES, MAX_PLAYER_STATE_EFFECTS, PLAYER_STATE_ARMOR_SLOTS,
    PLAYER_STATE_INVENTORY_SLOTS,
};

#[derive(Default)]
pub(super) struct PlayerState {
    pub snapshot: Option<PlayerStateSnapshot>,
    reads: u32,
}

impl PlayerState {
    /// Revoke stale local facts before and after every callback, including inactive ones.
    pub fn begin_frame(&mut self) {
        self.snapshot = None;
        self.reads = 0;
    }
}

fn valid_item(item: &PlayerStateItem) -> bool {
    item.network_id != 0
        && item.count > 0
        && item.identifier.as_ref().is_none_or(|identifier| {
            !identifier.is_empty()
                && identifier.len() <= MAX_ITEM_IDENTIFIER_BYTES
                && !identifier.chars().any(char::is_control)
        })
        && item.max_durability != Some(0)
}

fn valid_slot(slot: &PlayerStateSlot) -> bool {
    slot.item
        .as_ref()
        .is_none_or(|item| slot.known && valid_item(item))
}

/// Reject an invalid app payload before any facts cross the guest boundary.
pub(super) fn validate(snapshot: Option<&PlayerStateSnapshot>) -> Result<()> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    ensure!(
        snapshot.inventory.len() == PLAYER_STATE_INVENTORY_SLOTS
            && snapshot.armor.len() == PLAYER_STATE_ARMOR_SLOTS
            && snapshot.effects.len() <= MAX_PLAYER_STATE_EFFECTS,
        "invalid player-state snapshot lengths"
    );
    ensure!(
        snapshot.selected_slot.is_none_or(|slot| slot < 9)
            && snapshot.inventory.iter().all(valid_slot)
            && snapshot.armor.iter().all(valid_slot)
            && valid_slot(&snapshot.offhand)
            && snapshot
                .effects
                .iter()
                .all(|effect| { effect.effect_id > 0 && effect.remaining_ticks != Some(0) })
            && snapshot
                .effects
                .windows(2)
                .all(|pair| pair[0].effect_id < pair[1].effect_id),
        "invalid player-state item, selection or effect"
    );
    Ok(())
}

impl cinnabar::extension::player_state::Host for State {
    fn read_snapshot(&mut self) -> Result<Result<Option<PlayerStateSnapshot>, String>> {
        self.player_state.reads += 1;
        if self.player_state.reads > MAX_IMPORT_WRITES {
            bail!("player-state read budget exhausted");
        }
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.snapshot.clone()))
    }
}

#[cfg(test)]
#[path = "player_state_tests.rs"]
mod tests;
