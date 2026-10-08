//! Container windows beyond chests: admission, cell addressing, window data
//! and the multi-destination gestures.

use std::sync::Arc;

use sha2::Digest;

use protocol::{
    CONTAINER_NAME_CURSOR, ContainerDataEvent, ContainerIdentity, ContainerOpenEvent,
    EnchantOption, EnchantOptionsEvent, InventoryContentEvent, InventoryEvent, InventorySlotEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack, SlotIdentity,
    StackRequestAction, StackRequestContainer, WINDOW_TYPE_ANVIL, WINDOW_TYPE_BEACON,
    WINDOW_TYPE_ENCHANTMENT, WINDOW_TYPE_FURNACE, WINDOW_TYPE_HOPPER, WindowKind,
};

use super::*;

#[path = "distribute/live_tests.rs"]
mod live_distribute_tests;
#[path = "distribute/reconcile_tests.rs"]
mod reconcile_distribute_tests;

fn stack(stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        stack_network_id,
        count,
        ..NetworkItemStack::default()
    }
}

fn ledger_with(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: vec![ItemRegistryEntry {
            identifier: Arc::from("minecraft:apple"),
            network_id: 6,
            component_based: true,
            version: ItemRegistryVersion::DataDriven,
            component_digest: [6; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: false,
            item_tags: Arc::from([]),
        }]
        .into(),
    });
    let mut content = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    for (slot, held) in slots {
        content[*slot] = held.clone();
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(content),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
}

fn open(ledger: &mut PlayerInventoryLedger, window_id: i32, window_type: i8) {
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
}

fn content(container: ContainerIdentity, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

fn slot_update(container: ContainerIdentity, slot: u16, held: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack: held,
        storage_item: None,
    })
}

fn named(window_id: i32, name: u8) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window_id),
        slot_type: Some(name),
        dynamic_id: None,
    }
}

/// A furnace admits, restates its three cells and names them by role.
#[test]
fn furnace_cells_are_addressed_by_role() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 3, WINDOW_TYPE_FURNACE);
    assert_eq!(ledger.window_kind(), Some(WindowKind::Furnace));
    ledger.apply(&content(
        ContainerIdentity::window(3),
        vec![stack(20, 2), stack(21, 3), NetworkItemStack::default()],
    ));
    assert_eq!(ledger.storage_slot_count(), Some(3));
    ledger.apply(&slot_update(named(3, 26), 0, stack(22, 5)));
    assert_eq!(ledger.storage_stack(2).unwrap().count, 5);
    ledger.begin_storage_click(1).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("a take from the fuel cell");
    };
    assert_eq!(
        source.container,
        StackRequestContainer::OpenWindow {
            name: 24,
            dynamic_id: None
        }
    );
    assert_eq!(source.slot, 1);
}

/// A hopper takes the level-entity name with a five-cell payload.
#[test]
fn hopper_admits_five_generic_cells() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 4, WINDOW_TYPE_HOPPER);
    ledger.apply(&content(
        ContainerIdentity {
            window_id: Some(4),
            slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(9),
        },
        vec![NetworkItemStack::default(); 5],
    ));
    assert_eq!(ledger.storage_slot_count(), Some(5));
    assert_eq!(ledger.window_kind(), Some(WindowKind::Hopper));
}

/// ContainerSetData properties land on the open window only.
#[test]
fn window_data_is_scoped_to_the_open_window() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 3, WINDOW_TYPE_FURNACE);
    let data = |window, value| {
        InventoryEvent::Data(ContainerDataEvent {
            container: ContainerIdentity::window(window),
            property: 1,
            value,
        })
    };
    ledger.apply(&data(9, 5));
    assert_eq!(ledger.window_data(1), None);
    ledger.apply(&data(3, 42));
    assert_eq!(ledger.window_data(1), Some(42));
}

/// Anvil inputs live at UI slots and a shift-click fills the free one.
#[test]
fn anvil_inputs_use_ui_slots_and_named_requests() {
    let mut ledger = ledger_with(&[(20, stack(100, 3))]);
    open(&mut ledger, 5, WINDOW_TYPE_ANVIL);
    assert!(WindowKind::Anvil.is_ui_backed());
    let target = NetworkItemStack {
        network_id: 8,
        ..stack(30, 1)
    };
    ledger.apply(&slot_update(named(124, 0), 1, target));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(1))
            .unwrap()
            .count,
        1
    );
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    let Some(StackRequestAction::Place { destination, .. }) = ledger.newest_action() else {
        panic!("a place into the material slot");
    };
    assert_eq!(
        destination.container,
        StackRequestContainer::OpenWindow {
            name: 1,
            dynamic_id: None
        }
    );
    assert_eq!(destination.slot, 2);
}

/// Paying a beacon sends the payment then destroys the paid item.
#[test]
fn beacon_payment_destroys_the_payment_item() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 6, WINDOW_TYPE_BEACON);
    ledger.apply(&slot_update(named(124, 8), 27, stack(40, 1)));
    ledger.begin_beacon_payment(1, 10).unwrap();
    let request = ledger.newest_request().unwrap();
    assert!(matches!(
        request.actions[0],
        StackRequestAction::BeaconPayment {
            primary_effect: 1,
            secondary_effect: 10
        }
    ));
    assert!(matches!(
        request.actions[1],
        StackRequestAction::Destroy { amount: 1, .. }
    ));
    assert!(ledger.target_stack(InventoryTarget::Craft(27)).is_none());
}

/// Only an offered option can be selected.
#[test]
fn enchant_selection_needs_an_offered_option() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 7, WINDOW_TYPE_ENCHANTMENT);
    ledger.apply(&slot_update(named(124, 22), 14, stack(50, 1)));
    assert_eq!(
        ledger.begin_enchant(3),
        Err(InventoryGestureError::InvalidRequest)
    );
    ledger.apply(&InventoryEvent::EnchantOptions(EnchantOptionsEvent {
        options: Arc::from([EnchantOption {
            cost: 2,
            name: Arc::from("abc"),
            network_id: 3,
            enchants: Arc::from([(15, 2)]),
        }]),
    }));
    ledger.begin_enchant(3).unwrap();
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::CraftRecipe {
            recipe_network_id: 3,
            crafts: 1
        })
    ));
}

fn personal_ledger(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = ledger_with(slots);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    open(&mut ledger, 2, PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger
}

fn set_cursor(ledger: &mut PlayerInventoryLedger, held: NetworkItemStack) {
    ledger.apply(&content(
        ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        vec![held],
    ));
}

/// One shift-click fills partial stacks, then spills into the first empty cell.
#[test]
fn quick_move_spans_every_eligible_destination() {
    let mut ledger =
        personal_ledger(&[(0, stack(10, 10)), (9, stack(11, 62)), (10, stack(12, 62))]);
    ledger.begin_quick_move(InventoryTarget::Player(0)).unwrap();
    let request = ledger.newest_request().unwrap();
    let amounts: Vec<u8> = request
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Place { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(amounts, vec![2, 2, 6]);
    assert_eq!(ledger.displayed_stack(0), None);
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(11).unwrap().count, 6);
}

/// Dragging over several empty cells splits the held stack evenly.
#[test]
fn distribute_splits_the_cursor_evenly() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    ledger
        .advance_distribute(
            &mut None,
            &[
                InventoryTarget::Player(9),
                InventoryTarget::Player(10),
                InventoryTarget::Player(11),
            ],
            DistributeMode::Even,
        )
        .unwrap();
    for slot in [9, 10, 11] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 3);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
}

/// A secondary drag places one item per cell.
#[test]
fn distribute_one_places_single_items() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 5));
    ledger
        .advance_distribute(
            &mut None,
            &[InventoryTarget::Player(9), InventoryTarget::Player(10)],
            DistributeMode::One,
        )
        .unwrap();
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 1);
    assert_eq!(ledger.displayed_stack(10).unwrap().count, 1);
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
}

/// Gather takes partial stacks before full ones until the cursor is full.
#[test]
fn gather_fills_the_cursor_partial_stacks_first() {
    let mut ledger = personal_ledger(&[(5, stack(11, 20)), (6, stack(12, 64))]);
    set_cursor(&mut ledger, stack(50, 10));
    ledger.begin_gather().unwrap();
    let request = ledger.newest_request().unwrap();
    let takes: Vec<(u8, u8)> = request
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Take { amount, source, .. } => Some((source.slot, *amount)),
            _ => None,
        })
        .collect();
    assert_eq!(takes, vec![(5, 20), (6, 34)]);
    assert_eq!(ledger.cursor_stack().unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(6).unwrap().count, 30);
}

fn bundle_stack(stack_network_id: i32, bundle_id: i32) -> NetworkItemStack {
    let mut nbt = vec![10, 0, 0, 3, 9, 0];
    nbt.extend(b"bundle_id");
    nbt.extend(bundle_id.to_le_bytes());
    nbt.push(0);
    let mut extra = (-1_i16).to_le_bytes().to_vec();
    extra.push(1);
    extra.extend(nbt);
    NetworkItemStack {
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: Arc::from(extra),
        ..stack(stack_network_id, 1)
    }
}

fn dynamic(dynamic_id: u32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(-1),
        slot_type: Some(protocol::CONTAINER_NAME_DYNAMIC),
        dynamic_id: Some(dynamic_id),
    }
}

/// A bundle's contents ride its dynamic container; extract takes the newest item.
#[test]
fn bundle_extract_takes_the_newest_item_from_the_dynamic_container() {
    let mut ledger = personal_ledger(&[(0, bundle_stack(70, 7))]);
    ledger.apply(&content(dynamic(7), vec![stack(80, 3), stack(81, 2)]));
    assert_eq!(ledger.bundle_contents(7).map(<[_]>::len), Some(2));
    ledger
        .begin_bundle_extract(InventoryTarget::Player(0))
        .unwrap();
    let Some(StackRequestAction::Take {
        amount,
        source,
        destination,
    }) = ledger.newest_action()
    else {
        panic!("a take from the bundle");
    };
    assert_eq!((amount, source.slot, source.stack_network_id), (2, 1, 81));
    assert_eq!(
        source.container,
        StackRequestContainer::OpenWindow {
            name: protocol::CONTAINER_NAME_DYNAMIC,
            dynamic_id: Some(7)
        }
    );
    assert_eq!(destination.container, StackRequestContainer::Cursor);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, -3);
}

/// Insert appends after the last content slot and empties the cursor.
#[test]
fn bundle_insert_appends_to_the_dynamic_container() {
    let mut ledger = personal_ledger(&[(0, bundle_stack(70, 7))]);
    ledger.apply(&content(dynamic(7), vec![stack(80, 3)]));
    set_cursor(&mut ledger, stack(60, 4));
    ledger
        .begin_bundle_insert(InventoryTarget::Player(0))
        .unwrap();
    let Some(StackRequestAction::Place {
        amount,
        destination,
        ..
    }) = ledger.newest_action()
    else {
        panic!("a place into the bundle");
    };
    assert_eq!((amount, destination.slot), (4, 1));
    assert!(ledger.cursor_stack().is_none());
}
