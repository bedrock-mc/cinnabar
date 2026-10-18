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
    /// Builds a pending slot-selection command without consuming it.
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
    /// Unknown cells wait; accepted sends commit the slot and stack together.
    pub fn changed_held_equipment_packet(
        &self,
        game_mode: Option<PlayerGameMode>,
    ) -> Result<Option<(u8, protocol::Packet)>, protocol::InventoryPacketError> {
        let Some(runtime_id) = self.local_runtime_id() else {
            return Ok(None);
        };
        let Some(snapshot) = self.selected_stack_snapshot(game_mode) else {
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
            PlayerInventorySlot::Unknown => return Ok(None),
            PlayerInventorySlot::Empty => &empty,
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
        let packet = protocol::select_hotbar_slot_packet(runtime_id, target, stack)?;
        Ok(Some((target, packet)))
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

    /// Commits known held equipment after acceptance, preserving newer pending selections.
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
        let stack = match self.hotbar_slot_state(slot) {
            None | Some(PlayerInventorySlot::Unknown) => return false,
            Some(PlayerInventorySlot::Empty) => protocol::NetworkItemStack::empty(),
            Some(PlayerInventorySlot::Present(stack)) => stack.clone(),
        };
        self.announced_hotbar = (slot, stack);
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
        let state = self.hotbar_slot_state(slot)?;
        Some(SelectedStackSnapshot { slot, state })
    }

    /// Resolves a physical hotbar cell, using its equipment echo only while authority is unknown.
    fn hotbar_slot_state(&self, slot: u8) -> Option<PlayerInventorySlot<'_>> {
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
        Some(state)
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
mod tests {
    use super::*;
    use protocol::{
        ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
        InventorySlotEvent,
    };

    #[test]
    fn offhand_equipment_tracks_count_identity_clear_and_session_reset() {
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        assert!(
            session
                .changed_offhand_equipment_packet()
                .unwrap()
                .is_none()
        );
        let mut stack = protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        };
        for count in [4, 3, 0] {
            stack.count = count;
            replace_offhand_stack(&mut session, stack.clone());
            let packet = session.changed_offhand_equipment_packet().unwrap().unwrap();
            assert_eq!(
                protocol::test_support::packets::equipment(&packet)
                    .unwrap()
                    .count,
                count
            );
            assert_eq!(
                protocol::encode(&packet).unwrap(),
                protocol::encode(&session.changed_offhand_equipment_packet().unwrap().unwrap())
                    .unwrap()
            );
            session.acknowledge_offhand_equipment();
            assert!(
                session
                    .changed_offhand_equipment_packet()
                    .unwrap()
                    .is_none()
            );
            if count != 0 {
                stack.stack_network_id = -3;
                replace_offhand_stack(&mut session, stack.clone());
                assert!(
                    session
                        .changed_offhand_equipment_packet()
                        .unwrap()
                        .is_none()
                );
            }
        }
        stack.count = 1;
        replace_offhand_stack(&mut session, stack.clone());
        session.acknowledge_offhand_equipment();
        stack.metadata = 3;
        replace_offhand_stack(&mut session, stack.clone());
        assert!(
            session
                .changed_offhand_equipment_packet()
                .unwrap()
                .is_some()
        );
        session.acknowledge_offhand_equipment();
        session.begin_session(2);
        session.publish_local_runtime_id(2, 99).unwrap();
        assert!(
            session
                .changed_offhand_equipment_packet()
                .unwrap()
                .is_none()
        );
        replace_offhand_stack(&mut session, stack);
        assert!(
            session
                .changed_offhand_equipment_packet()
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn ignored_offhand_slot_does_not_change_equipment_announcements() {
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        replace_offhand_stack(
            &mut session,
            protocol::NetworkItemStack {
                network_id: 7,
                stack_network_id: 13,
                count: 4,
                ..protocol::NetworkItemStack::empty()
            },
        );
        let pending =
            protocol::encode(&session.changed_offhand_equipment_packet().unwrap().unwrap())
                .unwrap();
        for acknowledged in [false, true] {
            if acknowledged {
                session.acknowledge_offhand_equipment();
            }
            session
                .ledger_mut()
                .apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                        slot: 0,
                    },
                    stack: protocol::NetworkItemStack::empty(),
                    storage_item: None,
                }));
            let packet = session.changed_offhand_equipment_packet().unwrap();
            if acknowledged {
                assert!(packet.is_none());
            } else {
                assert_eq!(protocol::encode(&packet.unwrap()).unwrap(), pending);
            }
        }
    }

    /// Replaces known offhand authority through its content receipt.
    fn replace_offhand_stack(session: &mut InventorySession, stack: protocol::NetworkItemStack) {
        session
            .ledger_mut()
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                slots: Arc::from([stack]),
                storage_item: protocol::NetworkItemStack::empty(),
            }));
    }

    /// Replaces one known hotbar cell without changing the selected slot.
    fn replace_held_stack(session: &mut InventorySession, stack: protocol::NetworkItemStack) {
        session
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 2,
                },
                stack,
                storage_item: None,
            }));
    }

    /// Builds original item data with the same fields in either compound order.
    fn held_compound(reverse: bool, damage: i32) -> std::sync::Arc<[u8]> {
        let mut extra = vec![0xff, 0xff, 1, 10, 0, 0];
        let mut fields = [("Damage", damage), ("RepairCost", 3)];
        if reverse {
            fields.reverse();
        }
        for (name, value) in fields {
            extra.push(3);
            extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
            extra.extend_from_slice(name.as_bytes());
            extra.extend_from_slice(&value.to_le_bytes());
        }
        extra.push(0);
        extra.extend_from_slice(&[0; 8]);
        extra.into()
    }

    #[test]
    fn held_equipment_ignores_compound_order_but_keeps_value_changes() {
        use sha2::{Digest, Sha256};
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        let mode = Some(PlayerGameMode::Survival);
        let mut stack = protocol::NetworkItemStack {
            network_id: 7,
            count: 1,
            extra_data: held_compound(false, 2),
            ..protocol::NetworkItemStack::empty()
        };
        stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
        replace_held_stack(&mut session, stack.clone());
        session.queue_local_hotbar_selection(2, mode);
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_some()
        );
        assert!(session.clear_pending_hotbar_selection(2));
        stack.extra_data = held_compound(true, 2);
        stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
        replace_held_stack(&mut session, stack.clone());
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );
        stack.extra_data = held_compound(true, 4);
        stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
        replace_held_stack(&mut session, stack);
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn held_equipment_resends_same_slot_count_changes_and_depletion() {
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        let mode = Some(PlayerGameMode::Survival);
        let mut stack = protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        };
        replace_held_stack(&mut session, stack.clone());
        session.queue_local_hotbar_selection(2, mode);
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_some()
        );
        assert!(session.clear_pending_hotbar_selection(2));
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );

        stack.stack_network_id = -3;
        replace_held_stack(&mut session, stack.clone());
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );

        stack.count = 3;
        replace_held_stack(&mut session, stack.clone());
        let (slot, packet) = session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .expect("count change must update held equipment");
        assert_eq!(slot, 2);
        assert_eq!(
            protocol::test_support::packets::equipment(&packet)
                .unwrap()
                .count,
            3
        );
        let (_, retry) = session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .unwrap();
        assert_eq!(
            protocol::encode(&packet).unwrap(),
            protocol::encode(&retry).unwrap()
        );
        assert!(session.clear_pending_hotbar_selection(slot));
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );

        stack.metadata = 5;
        replace_held_stack(&mut session, stack);
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_some()
        );
        assert!(session.clear_pending_hotbar_selection(2));
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );

        replace_held_stack(&mut session, protocol::NetworkItemStack::empty());
        let (slot, packet) = session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .expect("depletion must clear held equipment");
        assert_eq!(
            protocol::test_support::packets::equipment(&packet)
                .unwrap()
                .count,
            0
        );
        assert!(session.clear_pending_hotbar_selection(slot));
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );

        session.begin_session(2);
        session.publish_local_runtime_id(2, 99).unwrap();
        session.set_local_selected_slot(2);
        replace_held_stack(&mut session, protocol::NetworkItemStack::empty());
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_some()
        );
    }

    /// A slot whose stack still carries a predicted request id sends at once.
    #[test]
    fn predicted_stack_id_does_not_withhold_the_hotbar_packet() {
        let game_mode = Some(PlayerGameMode::Survival);
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        let ledger = session.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 2,
            },
            stack: protocol::NetworkItemStack {
                network_id: 7,
                stack_network_id: 13,
                count: 4,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }));
        ledger.begin_world_drop(2, Some(1)).unwrap();
        let PlayerInventorySlot::Present(predicted) = session.ledger().slot_state(2).unwrap()
        else {
            panic!("the drop leaves three items predicted in the slot");
        };
        assert_eq!(predicted.stack_network_id, -3);

        session.queue_local_hotbar_selection(2, game_mode);
        let (slot, packet) = session.pending_hotbar_packet(game_mode).unwrap().unwrap();
        assert_eq!(slot, 2);
        let packet = protocol::test_support::packets::equipment(&packet)
            .expect("hotbar selection must build a MobEquipment packet");
        assert_eq!(packet.count, 3);
        assert_eq!(packet.stack_network_id, None);
    }
}
