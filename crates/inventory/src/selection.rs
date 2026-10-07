use crate::{
    EquipmentRoute, EquipmentRouteResult, InventoryRouterError, InventorySession,
    PlayerInventorySlot,
};
use protocol::{EquipmentEvent, PlayerGameMode};
use std::sync::Arc;

/// Equipment received at one ordered point in the current connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedLocalEquipment {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub event: EquipmentEvent,
}

/// The selected physical slot and its known, empty or present stack authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedStackSnapshot<'a> {
    pub slot: u8,
    pub state: PlayerInventorySlot<'a>,
}

impl InventorySession {
    /// Retained main-hand equipment while the ledger's selected slot is unknown.
    pub const fn local_selected_equipment(&self) -> Option<&SequencedLocalEquipment> {
        self.local_selected_equipment.as_ref()
    }
    /// Predict a local physical hotbar selection immediately.
    pub fn set_local_selected_slot(&mut self, slot: u8) {
        self.local_selected_slot = Some(slot);
    }
    /// The StartGame runtime identity used by inventory commands.
    pub fn local_runtime_id(&self) -> Option<u64> {
        self.equipment_router.local_runtime_id()
    }
    /// Publish identity and return previously buffered equipment in FIFO order.
    pub fn publish_local_runtime_id(
        &mut self,
        session: u64,
        runtime_id: u64,
    ) -> Result<Vec<EquipmentRoute>, InventoryRouterError> {
        self.equipment_router
            .publish_local_runtime_id(session, runtime_id)
    }
    /// Route equipment to local inventory selection or remote presentation.
    pub fn route_equipment(
        &mut self,
        session: u64,
        sequence: u64,
        event: EquipmentEvent,
    ) -> Result<EquipmentRouteResult, InventoryRouterError> {
        self.equipment_router.route(session, sequence, event)
    }
    /// Retain a main-hand echo without allowing offhand traffic to change selection.
    pub fn retain_local_selected_equipment(&mut self, fifo_sequence: u64, event: EquipmentEvent) {
        if event.handedness == Some(protocol::ActorHandedness::Left) {
            return;
        }
        self.local_selected_equipment = Some(SequencedLocalEquipment {
            session_id: self.session_id,
            fifo_sequence,
            event,
        });
    }
    /// Predict a slot and retain the latest command until transport accepts it.
    pub fn queue_local_hotbar_selection(&mut self, slot: u8, game_mode: Option<PlayerGameMode>) {
        if self.selected_hotbar_slot(game_mode) == Some(slot)
            && self.pending_hotbar_selection.is_none()
        {
            self.set_local_selected_slot(slot);
            return;
        }
        self.set_local_selected_slot(slot);
        self.pending_hotbar_selection = Some(slot);
    }
    /// The latest selected slot still awaiting transport.
    pub const fn pending_hotbar_selection(&self) -> Option<u8> {
        self.pending_hotbar_selection
    }
    /// Builds the pending selection from current authority without consuming it.
    /// Unknown cells and unresolved prediction IDs wait for a later inventory receipt;
    /// the caller clears the returned slot only after transport accepts the packet.
    pub fn pending_hotbar_packet(
        &self,
        game_mode: Option<PlayerGameMode>,
    ) -> Result<Option<(u8, protocol::Packet)>, protocol::InventoryPacketError> {
        let Some(target) = self.pending_hotbar_selection() else {
            return Ok(None);
        };
        let Some(runtime_id) = self.local_runtime_id() else {
            return Ok(None);
        };
        let Some(snapshot) = self.selected_stack_snapshot(game_mode) else {
            return Ok(None);
        };
        if snapshot.slot != target {
            return Ok(None);
        }
        let packet = match snapshot.state {
            PlayerInventorySlot::Unknown => return Ok(None),
            PlayerInventorySlot::Empty => protocol::select_hotbar_slot_packet(
                runtime_id,
                target,
                &protocol::NetworkItemStack::empty(),
            ),
            PlayerInventorySlot::Present(stack) => {
                // Predictions carry negative request IDs until the server answers.
                // The untracked server identity -1 is already ready to send.
                if stack.stack_network_id < -1 {
                    return Ok(None);
                }
                protocol::select_hotbar_slot_packet(runtime_id, target, stack)
            }
        }?;
        Ok(Some((target, packet)))
    }
    /// Clear the pending command only if it is still the slot just sent.
    pub fn clear_pending_hotbar_selection(&mut self, slot: u8) -> bool {
        if self.pending_hotbar_selection != Some(slot) {
            return false;
        }
        self.pending_hotbar_selection = None;
        true
    }
    /// Resolve selection using local prediction, server correction, equipment, then startup mode.
    pub fn selected_hotbar_slot(&self, game_mode: Option<protocol::PlayerGameMode>) -> Option<u8> {
        // Local selection is client-authoritative in Bedrock: once the player picks a slot
        // (number key / scroll / controller) that prediction wins over the server-echoed
        // equipment slot. A server PlayerHotbar with select_slot clears the local
        // prediction when it drains, so it takes effect at its FIFO position.
        self.local_selected_slot
            .or(self.server_selected_slot)
            .or_else(|| {
                self.local_selected_equipment
                    .as_ref()
                    .map(|equipment| equipment.event.selected_slot)
                    .filter(|slot| *slot < protocol::HOTBAR_SLOT_COUNT)
            })
            .or_else(|| {
                game_mode
                    .filter(|game_mode| game_mode.shows_hotbar())
                    .map(|_| 0)
            })
    }

    /// Borrows the selected slot and its one tri-state stack authority.
    pub fn selected_stack_snapshot(
        &self,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Option<SelectedStackSnapshot<'_>> {
        let slot = self.selected_hotbar_slot(game_mode)?;
        let ledger_state = self.inventory_ledger.slot_state(slot)?;
        let state = match ledger_state {
            PlayerInventorySlot::Unknown => self
                .local_selected_equipment
                .as_ref()
                .filter(|equipment| equipment.event.selected_slot == slot)
                .map_or(PlayerInventorySlot::Unknown, |equipment| {
                    if equipment.event.stack.is_empty() {
                        PlayerInventorySlot::Empty
                    } else {
                        PlayerInventorySlot::Present(&equipment.event.stack)
                    }
                }),
            known => known,
        };
        Some(SelectedStackSnapshot { slot, state })
    }

    /// Returns the present selected stack, preserving the existing optional API.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn selected_stack(
        &self,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Option<&protocol::NetworkItemStack> {
        match self.selected_stack_snapshot(game_mode)?.state {
            PlayerInventorySlot::Present(stack) => Some(stack),
            PlayerInventorySlot::Unknown | PlayerInventorySlot::Empty => None,
        }
    }

    /// The authoritative custom display name the selected hotbar cell
    /// presents, following the same predicted stack authority as
    /// [`Self::selected_stack_snapshot`]: during a pending gesture the
    /// travelling overlay of the predicted half serves beside the predicted
    /// stack. Presentation prefers it over the localized identifier
    /// fallback.
    pub fn selected_stack_custom_name(
        &self,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Option<Arc<str>> {
        let slot = self.selected_hotbar_slot(game_mode)?;
        self.inventory_ledger
            .presented_slot_overlay(slot)?
            .custom_name
            .clone()
    }

    /// Borrow the selected stack fallback, or the ledger's ordinary hotbar cell.
    pub fn presented_hotbar_stack(
        &self,
        slot: u8,
        game_mode: Option<PlayerGameMode>,
    ) -> Option<&protocol::NetworkItemStack> {
        if self.selected_hotbar_slot(game_mode) != Some(slot) {
            return self.inventory_ledger.displayed_stack(slot);
        }
        self.selected_stack(game_mode)
    }
    /// Accepted-response damage for each armor cell, which overrides the stack's own Damage tag.
    pub fn local_armor_damage_corrections(&self) -> [Option<u32>; 4] {
        std::array::from_fn(|slot| {
            self.inventory_ledger
                .presented_target_overlay(crate::inventory_ledger::InventoryTarget::Armor(
                    slot as u8,
                ))?
                .durability_correction
                .and_then(|damage| u32::try_from(damage).ok())
        })
    }

    /// Return helmet, chestplate, leggings and boots from the ledger's displayed armor cells.
    pub fn local_armor(&self) -> [protocol::NetworkItemStack; 4] {
        std::array::from_fn(|slot| {
            self.inventory_ledger
                .target_stack(crate::inventory_ledger::InventoryTarget::Armor(slot as u8))
                .cloned()
                .unwrap_or_default()
        })
    }
}
