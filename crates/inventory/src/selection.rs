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

/// Cancellation identity for local input preceding a forced server selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionAuthority {
    session: u64,
    generation: u64,
}

/// One cell's authoritative lifetime, independent of its current local prediction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionStamp {
    authority: SelectionAuthority,
    slot: u8,
    revision: Option<u64>,
}

/// A selection packet and the inventory evidence that permits that exact packet to retry.
pub struct PreparedHotbarSelection {
    stamp: SelectionStamp,
    packet: protocol::Packet,
}

impl PreparedHotbarSelection {
    /// Borrows the exact descriptor prepared before transport refusal.
    pub fn packet(&self) -> &protocol::Packet {
        &self.packet
    }
    /// Returns the selected cell announced by this proposal.
    pub const fn slot(&self) -> u8 {
        self.stamp.slot
    }
}

impl InventorySession {
    /// Identifies input invalidated by an authoritative forced selection, including same-slot writes.
    pub const fn selection_authority(&self) -> SelectionAuthority {
        SelectionAuthority {
            session: self.session_id,
            generation: self.selection_generation,
        }
    }

    /// Observes one cell's revision under the current selection authority.
    pub fn selection_stamp(&self, slot: u8) -> SelectionStamp {
        SelectionStamp {
            authority: self.selection_authority(),
            slot,
            revision: self.inventory_ledger.authoritative_slot_revision(slot),
        }
    }

    /// Rejects a retained cell after an authoritative write or session replacement.
    pub fn selection_stamp_current(&self, stamp: SelectionStamp) -> bool {
        self.selection_stamp(stamp.slot) == stamp
    }

    /// Prepares one slot's equipment packet together with its cancellation evidence.
    pub fn prepare_hotbar_selection(
        &self,
        slot: u8,
    ) -> Result<Option<PreparedHotbarSelection>, protocol::InventoryPacketError> {
        Ok(self
            .hotbar_packet(slot)?
            .map(|packet| PreparedHotbarSelection {
                stamp: self.selection_stamp(slot),
                packet,
            }))
    }

    /// Checks both authoritative lifetime and fallback equipment before retrying an exact packet.
    pub fn hotbar_selection_current(&self, proposal: &PreparedHotbarSelection) -> bool {
        self.selection_stamp_current(proposal.stamp)
            && self.hotbar_packet(proposal.slot()).ok().flatten().as_ref()
                == Some(proposal.packet())
    }

    /// Retained main-hand equipment while the ledger's selected slot is unknown.
    pub const fn local_selected_equipment(&self) -> Option<&SequencedLocalEquipment> {
        self.local_selected_equipment.as_ref()
    }
    /// Predict a local physical hotbar selection immediately.
    pub fn set_local_selected_slot(&mut self, slot: u8) {
        self.local_selected_slot = Some(slot);
    }

    /// Applies a server-selected hotbar slot and retires any local prediction.
    pub(crate) fn apply_server_selected_slot(&mut self, slot: u8) {
        debug_assert!(slot < protocol::HOTBAR_SLOT_COUNT);
        self.selection_generation = self
            .selection_generation
            .checked_add(1)
            .expect("selection authority generation exhausted");
        self.server_selected_slot = Some(slot);
        self.local_selected_slot = None;
        self.pending_hotbar_selection = None;
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
        Arc::make_mut(&mut self.equipment_router).publish_local_runtime_id(session, runtime_id)
    }
    /// Route equipment to local inventory selection or remote presentation.
    pub fn route_equipment(
        &mut self,
        session: u64,
        sequence: u64,
        event: EquipmentEvent,
    ) -> Result<EquipmentRouteResult, InventoryRouterError> {
        Arc::make_mut(&mut self.equipment_router).route(session, sequence, event)
    }
    /// Retain a main-hand server selection without allowing offhand traffic to change selection.
    pub fn retain_local_selected_equipment(&mut self, fifo_sequence: u64, event: EquipmentEvent) {
        if event.handedness == Some(protocol::ActorHandedness::Left) {
            return;
        }
        if event.selected_slot < protocol::HOTBAR_SLOT_COUNT {
            self.apply_server_selected_slot(event.selected_slot);
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
    /// Unknown cells announce an empty hand; unsent local backing changes still wait.
    pub fn pending_hotbar_packet(
        &self,
        game_mode: Option<PlayerGameMode>,
    ) -> Result<Option<(u8, protocol::Packet)>, protocol::InventoryPacketError> {
        if self.pending_hotbar_selection.is_none() {
            return Ok(None);
        }
        self.changed_held_equipment_packet(game_mode)
    }

    /// Builds changed held equipment for a local tick without consuming it.
    /// Unknown cells announce an empty hand; accepted sends commit the slot and stack together.
    pub fn changed_held_equipment_packet(
        &self,
        game_mode: Option<PlayerGameMode>,
    ) -> Result<Option<(u8, protocol::Packet)>, protocol::InventoryPacketError> {
        let Some(snapshot) = self.selected_backing_stack_snapshot(game_mode) else {
            return Ok(None);
        };
        let target = snapshot.slot;
        if self
            .pending_hotbar_selection
            .is_some_and(|pending| pending != target)
        {
            return Ok(None);
        }
        let empty = protocol::NetworkItemStack::empty();
        let stack = match snapshot.state {
            PlayerInventorySlot::Unknown | PlayerInventorySlot::Empty => &empty,
            PlayerInventorySlot::Present(stack) => stack,
        };
        let (announced_slot, announced_stack) = &self.announced_hotbar;
        if self.pending_hotbar_selection.is_none()
            && *announced_slot == target
            && announced_stack.count == stack.count
            && protocol::held_items_match(announced_stack, stack)
        {
            return Ok(None);
        }
        Ok(self.hotbar_packet(target)?.map(|packet| (target, packet)))
    }

    /// Builds backing equipment after local changes enter transport, without selecting the slot.
    /// Unknown cells announce an empty hand unless equipment supplies their stack.
    pub fn hotbar_packet(
        &self,
        target: u8,
    ) -> Result<Option<protocol::Packet>, protocol::InventoryPacketError> {
        let Some(runtime_id) = self.local_runtime_id() else {
            return Ok(None);
        };
        if self.inventory_ledger.slot_backing_transport_pending(target) {
            return Ok(None);
        }
        let Some(state) = self.inventory_ledger.backing_slot_state(target) else {
            return Ok(None);
        };
        let empty = protocol::NetworkItemStack::empty();
        let stack = match self.selected_snapshot_with_equipment(target, state).state {
            PlayerInventorySlot::Unknown | PlayerInventorySlot::Empty => &empty,
            PlayerInventorySlot::Present(stack) => stack,
        };
        protocol::select_hotbar_slot_packet(runtime_id, target, stack).map(Some)
    }

    /// Builds a changed known offhand stack without consuming its pending announcement.
    pub fn changed_offhand_equipment_packet(
        &self,
    ) -> Result<Option<protocol::Packet>, protocol::InventoryPacketError> {
        let Some(runtime_id) = self.local_runtime_id() else {
            return Ok(None);
        };
        let Some(stack) = self.known_offhand_stack() else {
            return Ok(None);
        };
        if self.announced_offhand.count == stack.count
            && protocol::held_items_match(&self.announced_offhand, &stack)
        {
            return Ok(None);
        }
        protocol::offhand_equipment_packet(runtime_id, &stack).map(Some)
    }

    /// Commits the offhand stack only after transport accepts its announcement.
    pub fn acknowledge_offhand_equipment(&mut self) {
        if let Some(stack) = self.known_offhand_stack() {
            self.announced_offhand = stack;
        }
    }

    /// Resolves offhand authority, preserving unknown separately from known empty.
    fn known_offhand_stack(&self) -> Option<protocol::NetworkItemStack> {
        match self
            .inventory_ledger
            .gear_slot_state(crate::inventory_ledger::InventoryTarget::Offhand)?
        {
            PlayerInventorySlot::Unknown => None,
            PlayerInventorySlot::Empty => Some(protocol::NetworkItemStack::empty()),
            PlayerInventorySlot::Present(stack) => Some(stack.clone()),
        }
    }

    /// Commits held backing equipment after acceptance, preserving newer pending selections.
    pub fn clear_pending_hotbar_selection(&mut self, slot: u8) -> bool {
        if self
            .pending_hotbar_selection
            .is_some_and(|pending| pending != slot)
            || self
                .selected_hotbar_slot(None)
                .is_some_and(|selected| selected != slot)
        {
            return false;
        }
        let Some(state) = self.inventory_ledger.backing_slot_state(slot) else {
            return false;
        };
        let stack = match self.selected_snapshot_with_equipment(slot, state).state {
            PlayerInventorySlot::Unknown | PlayerInventorySlot::Empty => {
                protocol::NetworkItemStack::empty()
            }
            PlayerInventorySlot::Present(stack) => stack.clone(),
        };
        self.announced_hotbar = (slot, stack);
        self.pending_hotbar_selection = None;
        true
    }
    /// Resolve selection using local prediction, server authority, equipment, then startup mode.
    pub fn selected_hotbar_slot(&self, game_mode: Option<protocol::PlayerGameMode>) -> Option<u8> {
        // Local input is shown immediately. `apply_server_selected_slot` retires that
        // prediction when a server PlayerHotbar or main-hand MobEquipment update arrives.
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
        let state = self.hotbar_slot_state(slot)?;
        Some(SelectedStackSnapshot { slot, state })
    }

    /// Resolves a physical hotbar cell, using its equipment echo only while authority is unknown.
    fn hotbar_slot_state(&self, slot: u8) -> Option<PlayerInventorySlot<'_>> {
        let ledger_state = self.inventory_ledger.slot_state(slot)?;
        Some(
            self.selected_snapshot_with_equipment(slot, ledger_state)
                .state,
        )
    }

    /// Borrows selected player backing while screen predictions retain their separate view.
    pub fn selected_backing_stack_snapshot(
        &self,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Option<SelectedStackSnapshot<'_>> {
        let slot = self.selected_hotbar_slot(game_mode)?;
        let state = self.inventory_ledger.backing_slot_state(slot)?;
        Some(self.selected_snapshot_with_equipment(slot, state))
    }

    /// Uses admitted equipment only while the addressed player cell remains unknown.
    fn selected_snapshot_with_equipment<'a>(
        &'a self,
        slot: u8,
        ledger_state: PlayerInventorySlot<'a>,
    ) -> SelectedStackSnapshot<'a> {
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
        SelectedStackSnapshot { slot, state }
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod ordered_tests {
    use super::*;
    use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};

    #[test]
    fn preparing_an_ordered_selection_does_not_leave_a_cancelled_prediction() {
        let mut session = InventorySession::new(7);
        session.publish_local_runtime_id(7, 42).unwrap();
        session.set_local_selected_slot(0);
        session
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 1,
                },
                stack: protocol::NetworkItemStack::empty(),
                storage_item: None,
            }));
        assert!(session.hotbar_packet(1).unwrap().is_some());
        assert_eq!(
            session.selected_hotbar_slot(Some(PlayerGameMode::Survival)),
            Some(0)
        );
        assert_eq!(session.pending_hotbar_selection(), None);
    }
}

#[cfg(test)]
mod selection_lifetime_tests {
    use super::*;
    use protocol::{
        ContainerIdentity, InventoryEvent, InventorySlotEvent, SelectedSlotEvent, SlotIdentity,
    };

    /// Supplies one known target cell and the runtime id needed by selection encoding.
    fn fixture() -> InventorySession {
        let mut session = InventorySession::new(7);
        session.publish_local_runtime_id(7, 42).unwrap();
        session.set_local_selected_slot(0);
        session
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 1,
                },
                stack: protocol::NetworkItemStack::empty(),
                storage_item: None,
            }));
        session
    }

    #[test]
    fn prepared_selection_is_cancelled_even_by_a_same_slot_forced_write() {
        let mut session = fixture();
        let proposal = session.prepare_hotbar_selection(1).unwrap().unwrap();
        assert!(session.hotbar_selection_current(&proposal));
        session
            .enqueue_inventory_event(
                7,
                1,
                InventoryEvent::SelectedSlot(SelectedSlotEvent {
                    container: ContainerIdentity::window(0),
                    slot: 0,
                    select_slot: true,
                }),
            )
            .unwrap();
        session.drain_pending_inventory();
        assert!(!session.hotbar_selection_current(&proposal));
        assert_eq!(
            session.selected_hotbar_slot(Some(PlayerGameMode::Survival)),
            Some(0)
        );
    }

    #[test]
    fn prepared_selection_is_cancelled_by_an_authoritative_target_cell_revision() {
        let mut session = fixture();
        let proposal = session.prepare_hotbar_selection(1).unwrap().unwrap();
        session
            .enqueue_inventory_event(
                7,
                1,
                InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(0),
                        slot: 1,
                    },
                    stack: protocol::NetworkItemStack::empty(),
                    storage_item: None,
                }),
            )
            .unwrap();
        session.drain_pending_inventory();
        assert!(!session.hotbar_selection_current(&proposal));
    }
}
