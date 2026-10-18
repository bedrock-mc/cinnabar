use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, InventoryLayout, InventoryOptions, InventoryTab, NetworkItemStack,
};

use client_ui::ui_runtime::{
    UiRuntime, flush_inventory_send, inventory_drag::PointerAction,
    presentation::forms::recipe_book_hover,
};
use inventory::inventory_ledger::{
    CreativeDestination, INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryPendingState,
    PERSONAL_INVENTORY_WINDOW_TYPE, PlayerInventoryLedger,
};
use ui_screens::container_actions::{InventoryCellHit, inventory_tab_ui};

const FIRST_CREATIVE_ITEM: u32 = 44;
const GROUP_ICON_ITEM: u32 = 45;

/// Builds a stack with a known item, count, and stack identity.
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

/// Opens personal inventory after its request crosses the transport.
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

/// Opens a creative catalog with two data variants and an icon matching the second.
fn creative_books() -> (player_state::PlayerState, UiRuntime) {
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
            components: None,
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
                creative_network_id: FIRST_CREATIVE_ITEM,
                stack: first,
                group: 0,
            },
            protocol::CreativeItem {
                creative_network_id: GROUP_ICON_ITEM,
                stack: second,
                group: 0,
            },
        ]),
        skipped: 0,
    }));
    (player, runtime)
}

/// Checks the bound drop's complete catalog request and leaves the cursor empty.
fn assert_creative_drop(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    hit: InventoryCellHit,
    creative_id: u32,
) {
    let mut expected_ledger = player.inventory_commands().clone();
    expected_ledger
        .begin_creative_take(creative_id, CreativeDestination::Drop, Some(1))
        .unwrap();
    client_ui::ui_runtime::interaction::dispatch_inventory_key(
        player,
        runtime,
        Some(hit),
        semantic_input::KeyboardKey::KeyQ,
        false,
        None,
        true,
    )
    .expect("catalog drop owns the bound key")
    .unwrap();
    let (expected, _) = expected_ledger.pending_batch().unwrap().unwrap();
    let ledger = runtime.inventory_ledger(player);
    let (actual, count) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(count, 1);
    assert_eq!(actual, expected);
    assert_eq!(ledger.pending_request_count(), 1);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn drop_key_admits_creative_grid_and_recipe_book_items() {
    for hit in [
        InventoryCellHit::CreativeGrid(0),
        InventoryCellHit::RecipeBook(0),
    ] {
        let (mut player, mut runtime) = creative_books();
        assert_creative_drop(&mut player, &mut runtime, hit, FIRST_CREATIVE_ITEM);
    }
}

#[test]
fn creative_group_drop_matches_the_icons_retained_user_data() {
    let (mut player, mut runtime) = creative_books();
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeTab(
            inventory_tab_ui(InventoryTab::Construction).unwrap(),
        )),
    );
    let (icon, name) = recipe_book_hover(&player, &runtime, 0).unwrap();
    let group = &runtime
        .inventory_ledger(&player)
        .creative_catalog()
        .unwrap()
        .groups[0];
    assert_eq!(Some(&icon), group.icon.as_ref());
    assert_eq!(name.as_deref(), Some(group.name.as_ref()));
    assert!(recipe_book_hover(&player, &runtime, 1).is_none());
    assert_creative_drop(
        &mut player,
        &mut runtime,
        InventoryCellHit::RecipeBook(0),
        GROUP_ICON_ITEM,
    );
}

#[test]
fn default_creative_search_lists_flat_variants_and_drops_the_first_item() {
    let (mut player, mut runtime) = creative_books();
    let controller = runtime.inventory_controller(&player).unwrap();
    assert_eq!(controller.options.left_tab, InventoryTab::Search);
    assert_eq!(controller.layout(), InventoryLayout::RecipeBookOnly);
    let catalog = runtime
        .inventory_ledger(&player)
        .creative_catalog()
        .unwrap();
    for (index, item) in catalog.items.iter().enumerate() {
        let (shown, group) =
            recipe_book_hover(&player, &runtime, u16::try_from(index).unwrap()).unwrap();
        assert_eq!(shown, item.stack);
        assert!(group.is_none(), "Search rows do not carry group headers");
    }
    assert!(
        recipe_book_hover(
            &player,
            &runtime,
            u16::try_from(catalog.items.len()).unwrap()
        )
        .is_none()
    );
    assert_eq!(
        player.inventory.inventory_options(),
        InventoryOptions::default()
    );
    assert!(runtime.take_client_packet().is_none());
    assert_creative_drop(
        &mut player,
        &mut runtime,
        InventoryCellHit::RecipeBook(0),
        FIRST_CREATIVE_ITEM,
    );
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
