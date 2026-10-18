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

/// Opens an original creative catalog and optionally seeds an ordinary cursor selection.
fn creative_selection(
    selection: Option<(i32, u16)>,
) -> Option<(UiRuntime, player_state::PlayerState)> {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return None;
    }
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: [(9000, "minecraft:apple"), (9001, "minecraft:diamond")]
            .map(|(id, identifier)| ItemRegistryEntry {
                identifier: identifier.into(),
                network_id: id,
                component_based: false,
                version: ItemRegistryVersion::Legacy,
                components: None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: [].into(),
            })
            .into(),
    });
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: vec![NetworkItemStack::empty(); protocol::PLAYER_INVENTORY_SLOTS as usize].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack: selection.map_or_else(NetworkItemStack::empty, |(id, count)| NetworkItemStack {
            network_id: id,
            stack_network_id: 42,
            count,
            ..NetworkItemStack::empty()
        }),
        storage_item: None,
    }));
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: [].into(),
        items: [protocol::CreativeItem {
            creative_network_id: 44,
            group: 0,
            stack: NetworkItemStack {
                network_id: 9000,
                count: 1,
                ..NetworkItemStack::empty()
            },
        }]
        .into(),
        skipped: 0,
    }));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    runtime.screen_state_mut().creative_tab = SEARCH_TAB;
    Some((runtime, player))
}

#[test]
fn creative_secondary_takes_one_with_an_empty_cursor() {
    let Some((mut runtime, mut player)) = creative_selection(None) else {
        return;
    };
    runtime
        .click_hit(&mut player, InventoryCellHit::CreativeGrid(0))
        .unwrap();
    assert_eq!(
        player.inventory.ledger().cursor_stack().unwrap().count,
        64,
        "primary control"
    );
    let Some((mut runtime, mut player)) = creative_selection(None) else {
        return;
    };
    runtime
        .secondary_click_hit(&mut player, InventoryCellHit::CreativeGrid(0))
        .unwrap();
    assert_eq!(player.inventory.ledger().cursor_stack().unwrap().count, 1);
}

#[test]
fn creative_secondary_destroys_one_from_any_ordinary_cursor_selection() {
    for (id, count) in [(9000, 1), (9000, 8), (9000, 64), (9001, 8)] {
        let Some((mut runtime, mut player)) = creative_selection(Some((id, count))) else {
            return;
        };
        runtime
            .secondary_click_hit(&mut player, InventoryCellHit::CreativeGrid(0))
            .unwrap();
        assert_eq!(
            player
                .inventory
                .ledger()
                .cursor_stack()
                .map(|stack| (stack.network_id, stack.count)),
            (count > 1).then_some((id, count - 1))
        );
    }
}

#[test]
fn creative_json_ui_secondary_catalog_click_takes_or_destroys_one() {
    for held in [None, Some((9000, 8)), Some((9001, 8))] {
        let Some((mut runtime, mut player)) = creative_selection(held) else {
            return;
        };
        runtime
            .secondary_click_hit(&mut player, InventoryCellHit::RecipeBook(0))
            .unwrap();
        let expected = held.map_or((9000, 1), |(id, count)| (id, count - 1));
        let shown = player.inventory.ledger().cursor_stack().unwrap();
        assert_eq!((shown.network_id, shown.count), expected);
    }
}
