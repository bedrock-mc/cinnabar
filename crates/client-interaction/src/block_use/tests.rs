use super::{held_block_store_id, verified_use_selection};
use client_ui::ui_runtime::UiRuntime;
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryEvent, InventorySlotEvent, NetworkItemStack,
    SlotIdentity,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Builds a retained network stack for the adapter fixtures.
fn network_item(network_id: i32, block_runtime_id: i32) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([]);
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: 41,
        count: 1,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id,
        extra_data,
    }
}

/// Builds one authoritative slot update.
fn inventory_slot(slot: u8, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(0),
                slot_type: None,
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack,
        storage_item: None,
    })
}

/// Constructs an offline stream using the selected palette encoding.
fn block_id_stream(hashed: bool) -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::air_network_id(hashed),
        block_network_ids_are_hashes: hashed,
    })
}

#[test]
fn placement_preserves_high_bit_block_hashes() {
    let mut stream = block_id_stream(true);
    let hash = 0x8000_0007_u32;
    stream.set_custom_block_ids(hash..hash + 1);
    let item_block = i32::from_ne_bytes(hash.to_ne_bytes());
    assert!(item_block < 0);
    assert_eq!(held_block_store_id(&stream, item_block), Some(hash));
}

#[test]
fn placement_skips_empty_air_and_uninitialized_block_ids() {
    for hashed in [false, true] {
        let stream = block_id_stream(hashed);
        for wire_id in [0, protocol::air_network_id(hashed), u32::MAX] {
            assert_eq!(
                held_block_store_id(&stream, i32::from_ne_bytes(wire_id.to_ne_bytes())),
                None
            );
        }
    }
}

#[test]
fn unknown_selection_and_unsent_hotbar_changes_wait_for_authority() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));

    let mut ui = UiRuntime::new(7);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    ui.inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert!(
        ui.inventory_ledger_mut(&mut player_runtime)
            .request_personal_open(42)
    );
    assert!(
        ui.inventory_ledger_mut(&mut player_runtime)
            .mark_transport_enqueued(0)
    );
    player_runtime.inventory.set_local_selected_slot(2);
    assert!(verified_use_selection(&player_runtime, &ui).is_none());

    ui.inventory_ledger_mut(&mut player_runtime)
        .apply(&inventory_slot(2, network_item(2, 77)));
    let selection = verified_use_selection(&player_runtime, &ui).unwrap();
    assert_eq!(selection.slot, 2);
    assert_eq!(selection.item.block_runtime_id(), 77);

    // Screen predictions leave the selected backing stack usable on either cell.
    ui.inventory_ledger_mut(&mut player_runtime)
        .apply(&inventory_slot(3, network_item(3, 0)));
    ui.inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    assert_eq!(
        verified_use_selection(&player_runtime, &ui).map(|selection| selection.slot),
        Some(2)
    );
    ui.inventory_ledger_mut(&mut player_runtime)
        .begin_click(2)
        .unwrap();
    assert_eq!(
        verified_use_selection(&player_runtime, &ui)
            .unwrap()
            .item
            .network_id(),
        2
    );

    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    let mut pending_hotbar = UiRuntime::new(7);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    pending_hotbar
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&inventory_slot(4, NetworkItemStack::empty()));
    let game_mode = player_runtime.facts.player_game_mode();
    player_runtime
        .inventory
        .queue_local_hotbar_selection(4, game_mode);
    assert!(verified_use_selection(&player_runtime, &pending_hotbar).is_none());
}

/// Opens a personal inventory with a known selected backing stack.
fn selection_fixture() -> (client_state::PlayerRuntime, UiRuntime) {
    let mut runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    let mut ui = UiRuntime::new(7);
    runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.inventory.set_local_selected_slot(2);
    let ledger = ui.inventory_ledger_mut(&mut runtime);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&inventory_slot(2, network_item(2, 77)));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    (runtime, ui)
}

#[test]
fn world_backing_use_keeps_the_selected_item_while_a_screen_predicts_its_removal() {
    let (mut runtime, mut ui) = selection_fixture();
    let ledger = ui.inventory_ledger_mut(&mut runtime);
    ledger.begin_click(2).unwrap();
    assert!(ledger.displayed_stack(2).is_none());
    let selection = verified_use_selection(&runtime, &ui).expect("backing item remains usable");
    assert_eq!(selection.item.network_id(), 2);
    assert_eq!(selection.item.stack_network_id(), 41);
}

#[test]
fn world_backing_use_ignores_recovery_on_inactive_screen_inputs() {
    let (mut runtime, mut ui) = selection_fixture();
    let ledger = ui.inventory_ledger_mut(&mut runtime);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: 28,
        },
        stack: network_item(5, 0),
        storage_item: None,
    }));
    assert!(ledger.poll_timeout(inventory::inventory_ledger::INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(
        ledger.resync_required(),
        "control retains the unfinished input recovery"
    );
    let selection = verified_use_selection(&runtime, &ui)
        .expect("screen recovery does not own world selection");
    assert_eq!(selection.item.network_id(), 2);
}

#[test]
fn world_backing_use_observes_a_server_restatement_under_a_screen_prediction() {
    let (mut runtime, mut ui) = selection_fixture();
    let ledger = ui.inventory_ledger_mut(&mut runtime);
    ledger.begin_click(2).unwrap();
    let mut replacement = network_item(4, 99);
    replacement.stack_network_id = 91;
    ledger.apply(&inventory_slot(2, replacement));
    assert!(
        ledger.displayed_stack(2).is_none(),
        "screen prediction still owns its presentation"
    );
    let selection =
        verified_use_selection(&runtime, &ui).expect("world selection reads the restated backing");
    assert_eq!(
        (
            selection.item.network_id(),
            selection.item.stack_network_id()
        ),
        (4, 91)
    );
}

/// A settled chest cursor must not make an unrelated retained input block the selected stack.
#[test]
fn inactive_input_does_not_block_use_after_settled_close() {
    use protocol::{
        ContainerCloseEvent, ContainerOpenEvent, InventoryContentEvent, ItemStackResponseEvent,
        StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
    };
    for close_first in [false, true] {
        let mut runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));
        let mut ui = UiRuntime::new(7);
        runtime
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        runtime.inventory.set_local_selected_slot(2);
        {
            let ledger = ui.inventory_ledger_mut(&mut runtime);
            ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
            let mut slots =
                vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
            slots[2] = network_item(2, 77);
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
            ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(1),
                window_type: protocol::WINDOW_TYPE_CONTAINER,
                position: [0; 3],
                runtime_entity_id: -1,
            }));
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(1),
                    slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                    dynamic_id: Some(980),
                },
                slots: vec![
                    NetworkItemStack::empty();
                    inventory::inventory_ledger::SMALL_STORAGE_SLOT_COUNT
                ]
                .into(),
                storage_item: NetworkItemStack::empty(),
            }));
            for (slot, name) in [
                (0, protocol::CONTAINER_NAME_CURSOR),
                (
                    *protocol::CRAFTING_INPUT_SLOTS.start(),
                    protocol::ui_slot_container_name(*protocol::CRAFTING_INPUT_SLOTS.start())
                        .unwrap(),
                ),
            ] {
                let mut item = network_item(5, 0);
                item.stack_network_id = if slot == 0 { 91 } else { 92 };
                ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity {
                            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                            slot_type: Some(name),
                            dynamic_id: None,
                        },
                        slot: u16::from(slot),
                    },
                    stack: item,
                    storage_item: None,
                }));
            }
            assert!(!ledger.resync_required());
        }
        assert!(verified_use_selection(&runtime, &ui).is_some());
        {
            let ledger = ui.inventory_ledger_mut(&mut runtime);
            ledger.request_storage_close();
            let request_id = ledger.pending_request_id().unwrap();
            assert!(ledger.mark_transport_enqueued(20));
            assert!(ledger.mark_transport_enqueued(21));
            let close = InventoryEvent::Close(ContainerCloseEvent {
                container: ContainerIdentity::window(1),
                window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
                server_initiated: false,
            });
            if close_first {
                ledger.apply(&close);
            }
            let correction = |name, slot, count, item_stack_id| StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(name),
                    dynamic_id: None,
                },
                slots: Arc::from([StackResponseSlot {
                    slot,
                    hotbar_slot: slot,
                    count,
                    item_stack_id,
                    custom_name: Arc::from(""),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 0,
                }]),
            };
            ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Accepted,
                    request_id,
                    containers: Arc::from([
                        correction(protocol::CONTAINER_NAME_CURSOR, 0, 0, 0),
                        correction(
                            protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                            0,
                            1,
                            91,
                        ),
                    ]),
                }]),
            }));
            if !close_first {
                ledger.apply(&close);
            }
            assert_eq!(ledger.pending_request_count(), 0);
            assert_eq!(
                ledger.cursor_stack().is_some(),
                close_first,
                "retired predictions preserve the confirmed UI cursor backing"
            );
            assert!(!ledger.slot_gesture_pending(2));
            assert!(!ledger.resync_required());
        }
        assert!(super::verified_selection(&runtime).is_some());
        assert!(
            verified_use_selection(&runtime, &ui).is_some(),
            "a settled close leaves the selected stack usable"
        );
    }
}

/// Retiring a selected tail slot establishes empty-hand interaction rather than stale item authority.
#[test]
fn short_inventory_content_keeps_empty_hand_use_available() {
    let mut runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    let mut ui = UiRuntime::new(7);
    runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.inventory.set_local_selected_slot(2);
    {
        let ledger = ui.inventory_ledger_mut(&mut runtime);
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        ledger.apply(&inventory_slot(2, network_item(2, 77)));
        ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slots: vec![NetworkItemStack::empty()].into(),
            storage_item: NetworkItemStack::empty(),
        }));
    }
    let selected = verified_use_selection(&runtime, &ui).expect("retired tail is known empty");
    assert_eq!(selected.slot, 2);
    assert!(selected.item.is_empty());
}
