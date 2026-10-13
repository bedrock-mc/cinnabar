use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent, InventorySlotEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, SlotIdentity,
};

use super::*;

const SWORD: i32 = 10;
const DIAMOND: i32 = 11;
const BOOK: i32 = 12;

/// A vanilla registry entry for `identifier`.
fn entry(identifier: &str, network_id: i32) -> ItemRegistryEntry {
    ItemRegistryEntry {
        identifier: Arc::from(identifier),
        network_id,
        component_based: false,
        version: ItemRegistryVersion::None,
        components: None,
        component_digest: [0; 32],
        negotiated_max_stack_size: None,
        canonical_empty_component_data: true,
        item_tags: Arc::from([]),
    }
}

/// Puts `stack` in anvil UI `slot`.
fn place(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    slot: u16,
    stack: NetworkItemStack,
) {
    runtime
        .inventory_ledger_mut(player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(124),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slot,
            },
            stack,
            storage_item: None,
        }));
}

/// An open anvil holding a sword with 1000 damage and ten diamonds.
fn anvil() -> (UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([
            entry("minecraft:diamond_sword", SWORD),
            entry("minecraft:diamond", DIAMOND),
            entry("minecraft:enchanted_book", BOOK),
        ]),
    });
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(5),
        window_type: protocol::WINDOW_TYPE_ANVIL,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let damaged = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            damage: Some(1000),
            ..protocol::ItemNbtEdit::default()
        },
    )
    .unwrap();
    let sword = NetworkItemStack {
        network_id: SWORD,
        stack_network_id: 30,
        count: 1,
        nbt_digest: Sha256::digest(&damaged).into(),
        extra_data: damaged,
        ..NetworkItemStack::empty()
    };
    place(&mut runtime, &mut player, 1, sword);
    let diamonds = NetworkItemStack {
        network_id: DIAMOND,
        stack_network_id: 31,
        count: 10,
        ..NetworkItemStack::empty()
    };
    place(&mut runtime, &mut player, 2, diamonds);
    (runtime, player)
}

#[test]
fn repairs_preview_the_mended_result_and_its_cost() {
    let (mut runtime, player) = anvil();
    runtime.sync_anvil_name(&player);
    let preview = runtime
        .anvil_preview(&player)
        .expect("diamonds repair the sword");
    assert_eq!(protocol::item_stack_damage(&preview.stack), Some(0));
    assert_eq!(protocol::item_repair_cost(&preview.stack.extra_data), 1);
    assert_eq!(preview.outcome.material_used, 3);
    assert_eq!(preview.outcome.cost, 3);
    assert_eq!(
        preview.outcome.custom_name, None,
        "the prefilled name renames nothing"
    );
    assert!(
        !runtime.anvil_affordable(&player, &preview),
        "no levels yet"
    );
}

#[test]
fn the_name_box_prefills_from_the_target_and_renames_cost_a_level() {
    let (mut runtime, mut player) = anvil();
    runtime.sync_anvil_name(&player);
    let prefilled = runtime.screen_state().anvil_name.clone();
    assert!(!prefilled.is_empty());
    runtime.screen_state_mut().anvil_name = "Blade".into();
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Blade", "typing is kept");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, Some(Some("Blade".into())));
    assert_eq!(preview.outcome.cost, 4);
    assert_eq!(
        protocol::item_display(&preview.stack.extra_data)
            .name
            .as_deref(),
        Some("Blade")
    );
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    assert!(runtime.anvil_affordable(&player, &preview));
}

/// The sword and a Sharpness I book: taking the merge without renaming sends the take.
#[test]
fn taking_a_book_merge_without_renaming_sends_the_take() {
    for rename in [None, Some("Blade")] {
        let (mut runtime, mut player) = anvil();
        let ench = protocol::edit_item_extra(
            &[],
            &protocol::ItemNbtEdit {
                enchantments: Some(vec![(9, 1)]),
                ..protocol::ItemNbtEdit::default()
            },
        )
        .unwrap();
        let book = NetworkItemStack {
            network_id: BOOK,
            stack_network_id: 32,
            count: 1,
            nbt_digest: Sha256::digest(&ench).into(),
            extra_data: ench,
            ..NetworkItemStack::empty()
        };
        place(&mut runtime, &mut player, 2, book);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);
        runtime.sync_anvil_name(&player);
        if let Some(name) = rename {
            runtime.screen_state_mut().anvil_name = name.into();
        }
        assert!(
            runtime.anvil_preview(&player).is_some(),
            "{rename:?}: a result shows"
        );
        runtime.perform_pointer_action(
            &mut player,
            crate::ui_runtime::inventory_drag::PointerAction::Click(
                ui_screens::container_actions::InventoryCellHit::CraftOutput,
            ),
        );
        let ledger = runtime.inventory_ledger(&player);
        assert_eq!(
            ledger.pending_request_count(),
            1,
            "{rename:?}: the take is queued"
        );
        assert!(
            matches!(ledger.pending_batch(), Ok(Some(_))),
            "{rename:?}: the take encodes: {:?}",
            ledger.pending_batch().err()
        );
    }
}
