//! Event-owned item snapshots for the camera's activation overlay.

use std::hash::BuildHasher;
use std::sync::Arc;

use super::{UiRuntime, UiRuntimeError, gameplay_authority::MILLIS_PER_SERVER_TICK};
use inventory::inventory_ledger::{InventoryTarget, PlayerInventorySlot, TOTEM_IDENTIFIER};

pub(super) const ACTIVATION_TICKS: u64 = 40;

#[derive(Clone, Debug)]
pub(super) struct ItemActivation {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub elapsed_millis: u64,
    pub started_clock_millis: u64,
    pub offsets: [i32; 2],
}

#[derive(Clone, Debug, Default)]
pub(super) struct ActivationClock {
    last_frame_millis: Option<u64>,
    elapsed_millis: u64,
}

impl ItemActivation {
    /// Fraction of the forty client ticks, including the current partial tick.
    pub fn progress(&self) -> Option<f32> {
        let age = self.elapsed_millis;
        let duration = ACTIVATION_TICKS * MILLIS_PER_SERVER_TICK;
        (age < duration).then_some(age as f32 / duration as f32)
    }
}

impl UiRuntime {
    /// Retains the equipped item at ingress while world publication may still be pending.
    pub fn retain_totem_snapshot(
        &mut self,
        player: &mut player_state::PlayerState,
        session: u64,
        sequence: u64,
    ) {
        if session != self.session_id()
            || self
                .pending_totem_snapshots
                .back()
                .is_some_and(|(last, _)| *last >= sequence)
        {
            return;
        }
        self.drain_inventory_prefix(player, Some(sequence));
        let metadata = self.equipped_totem_metadata(player);
        // The admitted world FIFO bounds how many activations can await publication.
        self.pending_totem_snapshots.push_back((sequence, metadata));
    }

    /// Advances the retained camera effect on bounded client time, including under menus.
    pub fn advance_item_activation(&mut self, now_millis: u64) {
        let clock = &mut self.activation_clock;
        let delta = clock
            .last_frame_millis
            .map_or(0, |last| now_millis.saturating_sub(last));
        clock.last_frame_millis = Some(
            clock
                .last_frame_millis
                .unwrap_or(now_millis)
                .max(now_millis),
        );
        clock.elapsed_millis = clock
            .elapsed_millis
            .saturating_add(delta.min(simulation_clock::MAX_FRAME_ELAPSED.as_millis() as u64));
        if let Some(activation) = self.item_activation.as_mut() {
            activation.elapsed_millis = clock
                .elapsed_millis
                .saturating_sub(activation.started_clock_millis);
            if activation.progress().is_none() {
                self.item_activation = None;
            }
        }
    }

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
        let retained = self
            .pending_totem_snapshots
            .iter()
            .position(|(pending, _)| *pending == sequence)
            .and_then(|index| self.pending_totem_snapshots.remove(index));
        let metadata = if let Some((_, metadata)) = retained {
            metadata
        } else {
            self.drain_inventory_prefix(player, Some(sequence));
            self.equipped_totem_metadata(player)
        };
        self.advance_item_activation(now_millis);
        let started_clock_millis =
            self.activation_clock.elapsed_millis / MILLIS_PER_SERVER_TICK * MILLIS_PER_SERVER_TICK;
        let random = std::collections::hash_map::RandomState::new().hash_one((session, sequence));
        // The camera renderer truncates each sampled offset toward zero.
        let offsets = [random as u32, (random >> 32) as u32].map(sampled_offset);
        self.item_activation = metadata.map(|metadata| ItemActivation {
            identifier: Arc::from(TOTEM_IDENTIFIER),
            metadata,
            elapsed_millis: self.activation_clock.elapsed_millis - started_clock_millis,
            started_clock_millis,
            offsets,
        });
        self.last_fifo_sequence = Some(sequence);
        self.last_local_millis = Some(now_millis);
        Ok(())
    }

    /// Resolves known inventory cells first, with equipment authority for unobserved hands.
    fn equipped_totem_metadata(&self, player: &player_state::PlayerState) -> Option<u32> {
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
        let offhand = match ledger.gear_slot_state(InventoryTarget::Offhand) {
            Some(PlayerInventorySlot::Present(stack)) => Some(stack),
            Some(PlayerInventorySlot::Unknown) => self.gameplay_hud.offhand_stack(),
            _ => None,
        };
        offhand
            .filter(is_totem)
            .or_else(|| selected.filter(is_totem))
            .map(|stack| stack.metadata)
    }
}

/// Preserves float rounding before the renderer truncates a signed unit offset.
fn sampled_offset(value: u32) -> i32 {
    let unit = (f64::from(value) / (f64::from(u32::MAX) + 1.0)) as f32;
    (unit * 2.0 - 1.0) as i32
}

#[cfg(test)]
mod tests;
