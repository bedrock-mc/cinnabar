//! Local hotbar slot selection.
//!
//! Bedrock owns hotbar-slot selection on the client: number keys, the mouse wheel, and the
//! controller cycle buttons change the held slot immediately (predicted locally so the HUD
//! highlight follows input without waiting for the server), and the choice is announced upstream
//! with a `PlayerHotbar` packet.

use bevy::{
    input::mouse::AccumulatedMouseScroll,
    prelude::{Res, ResMut},
};
use protocol::{HOTBAR_SLOT_COUNT, Packet, select_hotbar_slot_packet};
use semantic_input::Action;

use crate::{
    runtime::{
        network::{NetworkHandle, PacketSendError},
        shutdown::record_fatal_error,
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

const HOTBAR_DIGIT_ACTIONS: [Action; 9] = [
    Action::Hotbar1,
    Action::Hotbar2,
    Action::Hotbar3,
    Action::Hotbar4,
    Action::Hotbar5,
    Action::Hotbar6,
    Action::Hotbar7,
    Action::Hotbar8,
    Action::Hotbar9,
];

/// Applies number-key, mouse-wheel, and controller hotbar selection to the local prediction and
/// notifies the server. Runs after semantic input is finalized and before UI publication.
pub(crate) fn select_hotbar_slot(
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    mut runtime: ResMut<UiRuntime>,
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
) {
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for (index, action) in HOTBAR_DIGIT_ACTIONS.iter().enumerate() {
        if input.phase(*action).pressed {
            requested = Some(index as u8);
        }
    }

    // Relative cycling: controller buttons (router-gated) and the mouse wheel (gated on chat
    // focus here, since the wheel is read directly rather than through the router).
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += 1;
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle -= 1;
    }
    if !runtime.ui_focused() {
        // One slot per scroll frame. Scroll up selects the previous slot, scroll down the next
        // (matches vanilla). The wheel is read directly, so it is gated on chat focus here.
        if scroll.delta.y > 0.0 {
            cycle -= 1;
        } else if scroll.delta.y < 0.0 {
            cycle += 1;
        }
    }

    if requested.is_none() && cycle != 0 {
        let current = i32::from(
            runtime
                .selected_hotbar_slot()
                .unwrap_or(0)
                .min(HOTBAR_SLOT_COUNT - 1),
        );
        let slots = i32::from(HOTBAR_SLOT_COUNT);
        requested = Some((((current + cycle) % slots + slots) % slots) as u8);
    }

    if let Some(target) = requested {
        runtime.queue_local_hotbar_selection(target);
    }

    flush_pending_hotbar_selection(&mut runtime, &mut client_world.fatal_error, |packet| {
        network.send_hotbar_packet(packet)
    });
}

/// Attempts the latest pending hotbar selection once and retains it when authority or transport
/// is not ready.
fn flush_pending_hotbar_selection(
    runtime: &mut UiRuntime,
    fatal_error: &mut Option<String>,
    mut send: impl FnMut(Packet) -> Result<(), PacketSendError>,
) {
    let Some(target) = runtime.pending_hotbar_selection() else {
        return;
    };
    let Some(runtime_id) = runtime.local_runtime_id() else {
        return;
    };

    let packet = match runtime.inventory_ledger().slot_state(target) {
        Some(crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Unknown) | None => return,
        Some(crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Empty) => {
            select_hotbar_slot_packet(runtime_id, target, &protocol::NetworkItemStack::empty())
        }
        Some(crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(stack)) => {
            select_hotbar_slot_packet(runtime_id, target, stack)
        }
    };
    let packet = match packet {
        Ok(packet) => packet,
        Err(error) => {
            record_fatal_error(
                fatal_error,
                format!("hotbar selection packet validation failed: {error}"),
            );
            return;
        }
    };
    match send(packet) {
        Ok(()) => {
            runtime.clear_pending_hotbar_selection(target);
        }
        Err(PacketSendError::Full(_)) => {}
        Err(PacketSendError::Closed(_)) => record_fatal_error(
            fatal_error,
            "hotbar selection send failed because the network command channel closed".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{
        ContainerIdentity, InventoryAuthority, InventoryEvent, InventorySlotEvent,
        ItemStackResponseEvent, NetworkItemStack, SelectedSlotEvent, SlotIdentity, StackResponse,
        StackResponseStatus,
    };
    use sha2::{Digest, Sha256};

    use super::*;

    /// Publishes one authoritative player-inventory slot into a UI runtime.
    fn publish_slot(runtime: &mut UiRuntime, slot: u8, stack: NetworkItemStack) {
        runtime
            .inventory_ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: u16::from(slot),
                },
                stack,
                storage_item: None,
            }));
    }

    /// Creates a runtime with the local actor identity needed by MobEquipment.
    fn identified_runtime() -> UiRuntime {
        let mut runtime = UiRuntime::new(1);
        runtime.publish_local_runtime_id(1, 42).unwrap();
        runtime
    }

    /// Builds one valid non-empty stack for outbound hotbar packet tests.
    fn present_stack() -> NetworkItemStack {
        NetworkItemStack {
            network_id: 7,
            metadata: 3,
            stack_network_id: 13,
            count: 4,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id: 92,
            extra_data: Arc::from([]),
        }
    }

    #[test]
    fn unknown_slot_retains_pending_selection_without_sending() {
        let mut runtime = identified_runtime();
        runtime.queue_local_hotbar_selection(2);
        let mut sends = 0;
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |_packet| {
            sends += 1;
            Ok::<(), PacketSendError>(())
        });

        assert_eq!(sends, 0);
        assert_eq!(runtime.pending_hotbar_selection(), Some(2));
        assert_eq!(fatal, None);
    }

    #[test]
    fn full_send_retries_next_frame_without_new_input() {
        let mut runtime = identified_runtime();
        publish_slot(&mut runtime, 2, NetworkItemStack::empty());
        runtime.queue_local_hotbar_selection(2);
        let mut attempts = 0;
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |packet| {
            attempts += 1;
            Err(PacketSendError::Full(packet))
        });
        assert_eq!(runtime.pending_hotbar_selection(), Some(2));

        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |_| {
            attempts += 1;
            Ok(())
        });

        assert_eq!(attempts, 2);
        assert_eq!(runtime.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn newer_selection_supersedes_pending_selection() {
        let mut runtime = UiRuntime::new(1);
        runtime.queue_local_hotbar_selection(2);
        runtime.queue_local_hotbar_selection(7);

        assert_eq!(runtime.selected_hotbar_slot(), Some(7));
        assert_eq!(runtime.pending_hotbar_selection(), Some(7));
    }

    #[test]
    fn same_slot_input_does_not_suppress_an_unsent_pending_selection() {
        let mut runtime = identified_runtime();
        publish_slot(&mut runtime, 4, NetworkItemStack::empty());
        runtime.queue_local_hotbar_selection(4);
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |packet| {
            Err(PacketSendError::Full(packet))
        });

        runtime.queue_local_hotbar_selection(4);
        let mut sent = false;
        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |_| {
            sent = true;
            Ok(())
        });

        assert!(sent);
        assert_eq!(runtime.pending_hotbar_selection(), None);
    }

    #[test]
    fn begin_session_clears_pending_hotbar_selection() {
        let mut runtime = UiRuntime::new(1);
        runtime.queue_local_hotbar_selection(5);

        runtime.begin_session(2);

        assert_eq!(runtime.pending_hotbar_selection(), None);
    }

    #[test]
    fn predicted_slot_state_drives_packet_and_rollback_restores_authority() {
        let mut runtime = identified_runtime();
        runtime
            .inventory_ledger_mut()
            .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        let authoritative = present_stack();
        publish_slot(&mut runtime, 0, authoritative.clone());
        let request_id = runtime.inventory_ledger_mut().begin_click(0).unwrap();
        assert_eq!(
            runtime.inventory_ledger().slot_state(0),
            Some(crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Empty)
        );

        runtime.queue_local_hotbar_selection(0);
        let mut predicted_packet = None;
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |packet| {
            predicted_packet = Some(packet);
            Ok(())
        });
        let session = protocol::BedrockSession { shield_item_id: 0 };
        let predicted_bytes = protocol::encode(&predicted_packet.unwrap(), &session).unwrap();
        let empty_packet = select_hotbar_slot_packet(42, 0, &NetworkItemStack::empty()).unwrap();
        assert_eq!(
            predicted_bytes,
            protocol::encode(&empty_packet, &session).unwrap()
        );

        runtime
            .inventory_ledger_mut()
            .apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Rejected,
                    request_id,
                    containers: Arc::from([]),
                }]),
            }));
        assert_eq!(
            runtime.inventory_ledger().slot_state(0),
            Some(crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(&authoritative))
        );

        runtime.queue_local_hotbar_selection(1);
        runtime.queue_local_hotbar_selection(0);
        let mut restored_packet = None;
        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |packet| {
            restored_packet = Some(packet);
            Ok(())
        });
        let restored_bytes = protocol::encode(&restored_packet.unwrap(), &session).unwrap();
        let authoritative_packet = select_hotbar_slot_packet(42, 0, &authoritative).unwrap();
        assert_eq!(
            restored_bytes,
            protocol::encode(&authoritative_packet, &session).unwrap()
        );
        assert_eq!(fatal, None);
    }

    #[test]
    fn server_forced_selection_cancels_pending_local_packet() {
        let mut runtime = identified_runtime();
        publish_slot(&mut runtime, 2, NetworkItemStack::empty());
        runtime.queue_local_hotbar_selection(2);
        runtime
            .enqueue_inventory_event(
                1,
                1,
                InventoryEvent::SelectedSlot(SelectedSlotEvent {
                    container: ContainerIdentity::window(0),
                    slot: 5,
                    select_slot: true,
                }),
            )
            .unwrap();

        runtime.drain_pending_inventory();
        let mut sends = 0;
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut runtime, &mut fatal, |_| {
            sends += 1;
            Ok(())
        });

        assert_eq!(runtime.selected_hotbar_slot(), Some(5));
        assert_eq!(runtime.pending_hotbar_selection(), None);
        assert_eq!(sends, 0);
        assert_eq!(fatal, None);
    }
}
