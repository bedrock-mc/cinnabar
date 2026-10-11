//! Event-owned item snapshots for the camera's activation overlay.

use std::hash::BuildHasher;
use std::sync::Arc;

use super::{UiRuntime, UiRuntimeError, gameplay_authority::MILLIS_PER_SERVER_TICK};
use inventory::inventory_ledger::{InventoryTarget, PlayerInventorySlot};

pub(super) const ACTIVATION_TICKS: u64 = 40;
pub(super) const TOTEM_IDENTIFIER: &str = "minecraft:totem_of_undying";

#[derive(Clone, Debug)]
pub(super) struct ItemActivation {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub started_millis: u64,
    pub offsets: [i32; 2],
}

impl ItemActivation {
    /// Fraction of the forty client ticks, including the current partial tick.
    pub fn progress(&self, now_millis: u64) -> Option<f32> {
        let age = now_millis.saturating_sub(self.started_millis);
        let duration = ACTIVATION_TICKS * MILLIS_PER_SERVER_TICK;
        (age < duration).then_some(age as f32 / duration as f32)
    }
}

impl UiRuntime {
    /// Snapshots the equipped totem before subsequent inventory packets consume it.
    /// A new activation replaces the preceding one, including when no totem is equipped.
    pub fn activate_totem(
        &mut self,
        player: &mut player_state::PlayerState,
        session: u64,
        sequence: u64,
        now_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        self.validate_identity(session, sequence, now_millis, None)?;
        self.drain_inventory_prefix(player, Some(sequence));
        let ledger = player.inventory.ledger();
        let is_totem = |stack: &&protocol::NetworkItemStack| {
            stack.count > 0
                && ledger
                    .negotiated_item_entry(stack.network_id)
                    .is_some_and(|entry| entry.identifier.as_ref() == TOTEM_IDENTIFIER)
        };
        let selected = player
            .selected_stack_snapshot()
            .and_then(|snapshot| match snapshot.state {
                PlayerInventorySlot::Present(stack) => Some(stack),
                _ => None,
            });
        let random = std::collections::hash_map::RandomState::new().hash_one((session, sequence));
        // The camera renderer truncates each sampled offset toward zero.
        let offsets = [random as u32, (random >> 32) as u32].map(sampled_offset);
        self.item_activation = ledger
            .target_stack(InventoryTarget::Offhand)
            .filter(is_totem)
            .or_else(|| selected.filter(is_totem))
            .map(|stack| ItemActivation {
                identifier: Arc::from(TOTEM_IDENTIFIER),
                metadata: stack.metadata,
                started_millis: now_millis,
                offsets,
            });
        self.last_fifo_sequence = Some(sequence);
        self.last_local_millis = Some(now_millis);
        Ok(())
    }
}

/// Preserves float rounding before the renderer truncates a signed unit offset.
fn sampled_offset(value: u32) -> i32 {
    let unit = (f64::from(value) / (f64::from(u32::MAX) + 1.0)) as f32;
    (unit * 2.0 - 1.0) as i32
}

#[cfg(test)]
mod tests;
