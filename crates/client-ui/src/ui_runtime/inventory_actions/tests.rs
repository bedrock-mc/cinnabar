use super::*;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
};
use std::sync::Arc;

/// Builds a loom screen whose primary result click succeeds without renderer fixtures.
fn loom() -> (UiRuntime, player_state::PlayerState) {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from("fixture:banner"),
            network_id: 9000,
            component_based: false,
            version: ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(3),
        window_type: protocol::WINDOW_TYPE_LOOM,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    for slot in [9, 10] {
        slots[slot] = NetworkItemStack {
            network_id: 9000,
            stack_network_id: slot as i32 + 100,
            count: 2,
            ..NetworkItemStack::empty()
        };
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    runtime.screen_state_mut().loom_pattern = Some(Arc::from("bs"));
    (runtime, player)
}

/// Checks secondary result-click routing after verifying the same primary action works.
#[test]
fn parity_actions_secondary_result_click_crafts() {
    let (mut runtime, mut player) = loom();
    assert!(
        runtime
            .click_hit(&mut player, InventoryCellHit::CraftOutput)
            .is_ok(),
        "primary fixture control"
    );
    let (mut runtime, mut player) = loom();
    let result = runtime.secondary_click_hit(&mut player, InventoryCellHit::CraftOutput);
    assert!(
        result.is_ok(),
        "secondary result action was not routed: {result:?}"
    );
    assert!(player.inventory.ledger().cursor_stack().is_some());
}
