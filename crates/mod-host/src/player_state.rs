//! Bounded, read-only local player facts with callback-scoped reads and exact content tokens.

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
    previous_snapshot: Option<PlayerStateSnapshot>,
    revision: u64,
    reads: u32,
}

impl PlayerState {
    /// Clears current reads without discarding the exact observation used for change detection.
    pub fn begin_frame(&mut self) {
        self.snapshot = None;
        self.reads = 0;
    }

    /// Installs validated callback facts, retaining a comparison copy only when they change.
    pub fn set_snapshot(&mut self, snapshot: Option<PlayerStateSnapshot>) -> Result<()> {
        let Some(snapshot) = snapshot else {
            self.snapshot = None;
            return Ok(());
        };
        if self.previous_snapshot.as_ref() != Some(&snapshot) {
            self.advance_revision()?;
            self.previous_snapshot = Some(snapshot.clone());
        }
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Releases current and comparison facts after failure or loss of the instance.
    pub fn revoke(&mut self) {
        self.begin_frame();
        self.previous_snapshot = None;
    }

    /// Advances an instance-scoped content revision without wrapping the counter.
    fn advance_revision(&mut self) -> Result<()> {
        let Some(revision) = self.revision.checked_add(1) else {
            self.revoke();
            bail!("player-state revision exhausted");
        };
        self.revision = revision;
        Ok(())
    }

    /// Full snapshot and revision imports consume the same callback read allowance.
    fn read_budget(&mut self) -> Result<()> {
        self.reads += 1;
        if self.reads > MAX_IMPORT_WRITES {
            bail!("player-state read budget exhausted");
        }
        Ok(())
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
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.snapshot.clone()))
    }

    fn read_revision(&mut self) -> Result<Result<Option<u64>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self
            .player_state
            .snapshot
            .as_ref()
            .map(|_| self.player_state.revision)))
    }
}

#[cfg(test)]
#[path = "player_state_tests.rs"]
mod tests;
