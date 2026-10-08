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
fn unknown_or_inventory_pending_selection_fails_closed() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(7);

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

    ui.inventory_ledger_mut(&mut player_runtime)
        .apply(&inventory_slot(3, network_item(3, 0)));
    ui.inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    assert!(verified_use_selection(&player_runtime, &ui).is_none());

    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(7);
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
