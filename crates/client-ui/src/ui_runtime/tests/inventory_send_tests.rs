use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};

use crate::ui_runtime::inventory_ledger::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryPendingState, PERSONAL_INVENTORY_WINDOW_TYPE,
    PlayerInventoryLedger,
};
use crate::ui_runtime::{UiRuntime, flush_inventory_send};

fn stack(network_id: i32, count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        count,
        stack_network_id,
        block_runtime_id: 0,
        extra_data: Arc::from([]),
        nbt_digest: [0; 32],
    }
}

fn open_personal_inventory(ledger: &mut PlayerInventoryLedger) {
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
}

#[test]
fn drop_key_admits_creative_grid_and_recipe_book_items() {
    use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
    for hit in [
        InventoryCellHit::CreativeGrid(0),
        InventoryCellHit::RecipeBook(0),
    ] {
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);
        let mut runtime = UiRuntime::new(1);
        let ledger = runtime.inventory_ledger_mut(&mut player);
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        open_personal_inventory(ledger);
        ledger.apply_registry(&protocol::ItemRegistryEvent {
            entries: Arc::from([protocol::ItemRegistryEntry {
                identifier: Arc::from("minecraft:apple"),
                network_id: 1,
                component_based: false,
                version: protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            }]),
        });
        ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
            groups: Arc::from([protocol::CreativeGroup {
                category: protocol::CreativeCategory::Construction,
                name: Arc::from(""),
                icon: None,
            }]),
            items: Arc::from([protocol::CreativeItem {
                creative_network_id: 44,
                stack: stack(1, 1, -1),
                group: 0,
            }]),
            skipped: 0,
        }));
        crate::ui_runtime::interaction::dispatch_inventory_key(
            &mut player,
            &mut runtime,
            Some(hit),
            bevy::prelude::KeyCode::KeyQ,
            false,
            None,
            true,
        )
        .expect("catalog drop owns the bound key")
        .unwrap();
        assert_eq!(runtime.inventory_ledger(&player).pending_request_count(), 1);
        assert!(runtime.inventory_ledger(&player).cursor_stack().is_none());
    }
}

#[test]
fn creative_group_drop_matches_the_icons_retained_user_data() {
    use crate::ui_runtime::inventory_ledger::CreativeDestination;
    use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
    use sha2::{Digest, Sha256};
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    open_personal_inventory(ledger);
    ledger.apply_registry(&protocol::ItemRegistryEvent {
        entries: Arc::from([protocol::ItemRegistryEntry {
            identifier: Arc::from("minecraft:enchanted_book"),
            network_id: 1,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    let first = NetworkItemStack {
        extra_data: Arc::from([1_u8]),
        nbt_digest: Sha256::digest([1_u8]).into(),
        ..stack(1, 1, -1)
    };
    let second = NetworkItemStack {
        extra_data: Arc::from([2_u8]),
        nbt_digest: Sha256::digest([2_u8]).into(),
        ..first.clone()
    };
    let icon = NetworkItemStack {
        count: 2,
        stack_network_id: 9,
        ..second.clone()
    };
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([protocol::CreativeGroup {
            category: protocol::CreativeCategory::Construction,
            name: Arc::from("books"),
            icon: Some(icon),
        }]),
        items: Arc::from([
            protocol::CreativeItem {
                creative_network_id: 44,
                stack: first,
                group: 0,
            },
            protocol::CreativeItem {
                creative_network_id: 45,
                stack: second.clone(),
                group: 0,
            },
        ]),
        skipped: 0,
    }));
    let mut expected_ledger = ledger.clone();
    expected_ledger
        .begin_creative_take(45, CreativeDestination::Drop { whole_stack: false })
        .unwrap();
    crate::ui_runtime::interaction::dispatch_inventory_key(
        &mut player,
        &mut runtime,
        Some(InventoryCellHit::RecipeBook(0)),
        bevy::prelude::KeyCode::KeyQ,
        false,
        None,
        true,
    )
    .unwrap()
    .unwrap();
    let (expected, _) = expected_ledger.pending_batch().unwrap().unwrap();
    let (actual, count) = runtime
        .inventory_ledger(&player)
        .pending_batch()
        .unwrap()
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(actual, expected);
    assert!(runtime.inventory_ledger(&player).cursor_stack().is_none());
}

#[test]
fn bounded_transport_pressure_does_not_consume_or_duplicate_the_request() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    open_personal_inventory(runtime.inventory_ledger_mut(&mut player_runtime));
    let content = InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(
            (0..36)
                .map(|index| {
                    if index == 0 {
                        stack(5, 1, 44)
                    } else {
                        NetworkItemStack::default()
                    }
                })
                .collect::<Vec<_>>(),
        ),
        storage_item: NetworkItemStack::default(),
    });
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content);
    let request = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(0)
        .unwrap();

    assert_eq!(
        flush_inventory_send(&mut player_runtime, &mut runtime, 10, |_| Err("full")),
        Err("full")
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .pending_request_id(),
        Some(request)
    );
    assert_eq!(
        runtime.inventory_ledger(&player_runtime).pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert_eq!(
        flush_inventory_send(
            &mut player_runtime,
            &mut runtime,
            10 + INVENTORY_REQUEST_TIMEOUT_MILLIS,
            |_| Err("full")
        ),
        Err("full")
    );
    assert!(!runtime.inventory_ledger(&player_runtime).resync_required());

    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content);
    let retry = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(0)
        .unwrap();
    assert_eq!(
        flush_inventory_send(&mut player_runtime, &mut runtime, 11, |_| Ok::<_, &str>(())),
        Ok(true)
    );
    assert_eq!(
        flush_inventory_send(&mut player_runtime, &mut runtime, 12, |_| Ok::<_, &str>(())),
        Ok(false)
    );
    assert_ne!(request, retry);
}
