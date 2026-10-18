use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent, NetworkItemStack,
};

use client_ui::ui_runtime::UiRuntime;
use inventory::inventory_ledger::{CreativeDestination, PERSONAL_INVENTORY_WINDOW_TYPE};

/// An open personal inventory whose creative content lists `minecraft:apple` (aux 0) as
/// creative item 44, in game mode `mode`.
fn creative_session(mode: protocol::PlayerGameMode) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    player.facts.publish_player_game_mode(mode);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply_registry(&protocol::ItemRegistryEvent {
        entries: Arc::from([protocol::ItemRegistryEntry {
            identifier: Arc::from("minecraft:apple"),
            network_id: 1,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([protocol::CreativeGroup {
            category: protocol::CreativeCategory::Items,
            name: Arc::from(""),
            icon: None,
        }]),
        items: Arc::from([protocol::CreativeItem {
            creative_network_id: 44,
            stack: NetworkItemStack {
                network_id: 1,
                count: 1,
                stack_network_id: -1,
                ..NetworkItemStack::empty()
            },
            group: 0,
        }]),
        skipped: 0,
    }));
    (player, runtime)
}

#[test]
fn a_mods_creative_take_is_the_catalogs_request_for_its_count() {
    let (mut player, mut runtime) = creative_session(protocol::PlayerGameMode::Creative);
    let mut expected = runtime.inventory_ledger(&player).clone();
    expected
        .begin_creative_take(44, CreativeDestination::Cursor, Some(1))
        .unwrap();
    runtime
        .mod_creative_take(&mut player, "minecraft:apple", 0, 1)
        .unwrap();
    let (expected, _) = expected.pending_batch().unwrap().unwrap();
    let (actual, count) = runtime
        .inventory_ledger(&player)
        .pending_batch()
        .unwrap()
        .unwrap();
    assert_eq!((actual, count), (expected, 1));
}

#[test]
fn a_mods_creative_take_needs_creative_and_a_listed_item() {
    let (mut player, mut runtime) = creative_session(protocol::PlayerGameMode::Survival);
    assert!(
        runtime
            .mod_creative_take(&mut player, "minecraft:apple", 0, 1)
            .is_err()
    );
    let (mut player, mut runtime) = creative_session(protocol::PlayerGameMode::Creative);
    for (identifier, aux) in [("minecraft:apple", 1), ("minecraft:stone", 0)] {
        assert!(
            runtime
                .mod_creative_take(&mut player, identifier, aux, 1)
                .is_err()
        );
    }
    assert!(
        runtime.mod_fill_recipe(&mut player, 7).is_err(),
        "no catalog lists recipe 7"
    );
    assert_eq!(runtime.inventory_ledger(&player).pending_request_count(), 0);
}
