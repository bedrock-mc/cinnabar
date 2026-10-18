use protocol::PlayerInputMode;

use gameplay::mining::{
    CREATIVE_GAMEPAD_REACH_BLOCKS, CREATIVE_MOUSE_REACH_BLOCKS, CREATIVE_TOUCH_REACH_BLOCKS,
    SURVIVAL_TOUCH_REACH_BLOCKS, creative_reach, survival_reach,
};

#[test]
fn creative_reach_is_frozen_per_input_mode() {
    assert_eq!(
        creative_reach(PlayerInputMode::Mouse),
        CREATIVE_MOUSE_REACH_BLOCKS
    );
    assert_eq!(
        creative_reach(PlayerInputMode::GamePad),
        CREATIVE_GAMEPAD_REACH_BLOCKS
    );
    assert_eq!(
        creative_reach(PlayerInputMode::Touch),
        CREATIVE_TOUCH_REACH_BLOCKS
    );
    assert_eq!(
        survival_reach(PlayerInputMode::Touch),
        SURVIVAL_TOUCH_REACH_BLOCKS
    );
}

#[test]
fn unknown_selected_slot_mines_as_a_bare_hand() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));

    // Before the inventory arrives the slot is Unknown; mining must still work
    // by hand rather than refusing (the "can't break blocks" regression).
    player_runtime.inventory.set_local_selected_slot(0);
    let selection = super::hand_interaction_selection(&player_runtime)
        .expect("unknown selection resolves to an empty hand");
    assert_eq!(selection.item.network_id(), 0);
}

#[test]
fn sent_world_drop_remainders_remain_available_to_gameplay_selections() {
    use protocol::{
        ContainerIdentity, InventoryAuthority, InventoryEvent, InventorySlotEvent,
        NetworkItemStack, SlotIdentity,
    };
    use sha2::{Digest, Sha256};

    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    player_runtime.inventory.set_local_selected_slot(2);
    let ledger = player_runtime.inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slot: 2,
        },
        stack: NetworkItemStack {
            network_id: 7,
            stack_network_id: 41,
            count: 3,
            nbt_digest: Sha256::digest([]).into(),
            ..NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let legacy_id = protocol::test_support::transactions::observe_transaction(&packet)
        .unwrap()
        .legacy_request_id;
    assert!(ledger.mark_transport_enqueued(0));

    for selection in [
        super::verified_selection(&player_runtime),
        super::hand_interaction_selection(&player_runtime),
    ] {
        let selection = selection.expect("a settled legacy remainder is usable");
        assert_eq!(selection.slot, 2);
        assert_eq!(selection.item.network_id(), 7);
        assert_eq!(selection.item.count(), 2);
        assert_eq!(selection.item.stack_network_id(), legacy_id);
    }
}

#[test]
fn hand_and_verified_use_keep_their_eligibility_distinct() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    player.inventory.set_local_selected_slot(0);
    let ui = client_ui::ui_runtime::UiRuntime::new(7);
    assert!(super::hand_interaction_selection(&player).is_some());
    assert!(crate::block_use::verified_use_selection(&player, &ui).is_none());
}
