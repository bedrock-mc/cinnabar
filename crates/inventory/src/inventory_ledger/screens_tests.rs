//! Container windows beyond chests: admission, cell addressing, window data
//! and the multi-destination gestures.

use std::sync::Arc;

use sha2::Digest;

use protocol::{
    CONTAINER_NAME_CURSOR, ContainerDataEvent, ContainerIdentity, ContainerOpenEvent,
    EnchantOption, EnchantOptionsEvent, InventoryContentEvent, InventoryEvent, InventorySlotEvent,
    NetworkItemStack, SlotIdentity, StackRequestAction, StackRequestContainer, WINDOW_TYPE_ANVIL,
    WINDOW_TYPE_BEACON, WINDOW_TYPE_ENCHANTMENT, WINDOW_TYPE_FURNACE, WINDOW_TYPE_HOPPER,
    WindowKind,
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

/// Establishes server-backed player slots and a parsed apple capacity declaration.
fn ledger_with(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    ));
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
    ledger.apply(&slot_update(named(3, 26), 2, stack(22, 5)));
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

#[test]
fn named_window_refreshes_keep_taken_slots_empty_after_rejection() {
    for admission in 0..3 {
        let mut ledger = ledger_with(&[]);
        open(&mut ledger, 3, WINDOW_TYPE_FURNACE);
        let original = vec![stack(20, 2), stack(21, 3), stack(22, 4)];
        ledger.apply(&content(ContainerIdentity::window(3), original.clone()));
        let request = ledger.begin_storage_take_count(1, 3).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        let mut refreshed = original.clone();
        refreshed[1] = NetworkItemStack::empty();
        match admission {
            0 => ledger.apply(&content(ContainerIdentity::window(3), refreshed)),
            1 => ledger.apply(&content(ContainerIdentity::window(3), refreshed)),
            _ => {
                let (StackRequestContainer::OpenWindow { name, .. }, _) =
                    protocol::open_cell_request(WindowKind::Furnace, 1, None, None).unwrap()
                else {
                    panic!("named furnace cell");
                };
                ledger.apply(&content(
                    named(protocol::DYNAMIC_STORAGE_WINDOW_ID, name),
                    vec![NetworkItemStack::empty()],
                ));
            }
        }
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([protocol::StackResponse {
                    request_id: request,
                    status: protocol::StackResponseStatus::Rejected,
                    containers: Arc::from([]),
                }]),
            },
        ));
        assert!(ledger.storage_stack(1).is_none());
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(ledger.storage_stack(0), Some(&original[0]));
        assert_eq!(ledger.storage_stack(2), Some(&original[2]));
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(!ledger.resync_required());
        assert!(ledger.begin_storage_click(0).is_ok());
    }
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

/// Horse chest answers retain their admitted identity after window-based authority writes.
#[test]
fn horse_chest_accepted_response_settles_after_full_content() {
    for dynamic_id in [None, Some(9)] {
        let mut ledger = ledger_with(&[]);
        open(&mut ledger, 4, protocol::WINDOW_TYPE_HORSE);
        let identity = ContainerIdentity {
            window_id: Some(4),
            slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id,
        };
        let length = WindowKind::Horse
            .content_lengths()
            .iter()
            .copied()
            .max()
            .unwrap();
        let mut cells = vec![NetworkItemStack::empty(); length];
        cells[2] = stack(55, 5);
        cells[3] = stack(56, 2);
        ledger.apply(&content(identity, cells));
        let request_id = ledger.begin_storage_click(2).unwrap();
        let action = ledger.newest_action().unwrap();
        assert!(ledger.mark_transport_enqueued(1));
        let correction = |slot, count, item_stack_id| protocol::StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        };
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([protocol::StackResponse {
                    request_id,
                    status: protocol::StackResponseStatus::Accepted,
                    containers: Arc::from([
                        protocol::StackResponseContainer {
                            container: ContainerIdentity {
                                window_id: None,
                                dynamic_id: None,
                                ..identity
                            },
                            slots: Arc::from([correction(2, 0, 0)]),
                        },
                        protocol::StackResponseContainer {
                            container: ContainerIdentity {
                                window_id: None,
                                slot_type: Some(CONTAINER_NAME_CURSOR),
                                dynamic_id: None,
                            },
                            slots: Arc::from([correction(0, 5, 9001)]),
                        },
                    ]),
                }]),
            },
        ));
        assert!(!ledger.resync_required());
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(ledger.storage_stack(2).is_none());
        assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 9001);
        assert!(matches!(action, StackRequestAction::Take { source, .. }
            if source.container == StackRequestContainer::LevelEntity { dynamic_id: None }));
        assert!(ledger.begin_storage_click(3).is_ok());
    }
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

/// Restates the cursor through its fixed UI inventory slot.
fn set_cursor(ledger: &mut PlayerInventoryLedger, held: NetworkItemStack) {
    ledger.apply(&slot_update(
        ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        0,
        held,
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
    extra.extend_from_slice(&[0; 8]);
    NetworkItemStack {
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: Arc::from(extra),
        ..stack(stack_network_id, 1)
    }
}

fn dynamic(dynamic_id: u32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(protocol::DYNAMIC_STORAGE_WINDOW_ID),
        slot_type: Some(protocol::CONTAINER_NAME_DYNAMIC),
        dynamic_id: Some(dynamic_id),
    }
}

/// Admits a component-backed bundle and its registered storage descriptor.
fn bundle_ledger(id: u32, contents: Vec<NetworkItemStack>) -> PlayerInventoryLedger {
    let mut ledger = personal_ledger(&[]);
    let mut registry = protocol::test_support::items::component_item_registry(
        "fixture:bundle",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    );
    let entry = &mut Arc::make_mut(&mut registry.entries)[0];
    let components = entry.components.get_or_insert_with(Default::default);
    Arc::make_mut(components).storage_container = true;
    ledger.apply_registry(&registry);
    let bundle = bundle_stack(70, id as i32);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = bundle.clone();
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: dynamic(id),
        slots: contents.into(),
        storage_item: bundle,
    }));
    ledger
}

/// A bundle's contents ride its dynamic container; extract takes the newest item.
#[test]
fn bundle_extract_takes_the_newest_item_from_the_dynamic_container() {
    let mut ledger = bundle_ledger(7, vec![stack(80, 3), stack(81, 2)]);
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
    let mut ledger = bundle_ledger(7, vec![stack(80, 3)]);
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

#[test]
fn quick_move_storage_filter_excludes_partial_and_empty_cells() {
    let mut ledger = ledger_with(&[(0, stack(10, 3))]);
    open(&mut ledger, 4, protocol::WINDOW_TYPE_HORSE);
    ledger.apply(&content(
        ContainerIdentity::window(4),
        vec![
            NetworkItemStack::empty(),
            NetworkItemStack::empty(),
            stack(11, 62),
            stack(12, 63),
            NetworkItemStack::empty(),
        ],
    ));
    ledger
        .begin_quick_move_with_storage_filter(InventoryTarget::Player(0), |slot| slot >= 3)
        .unwrap();
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Storage(2))
            .unwrap()
            .count,
        62
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Storage(3))
            .unwrap()
            .count,
        64
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Storage(4))
            .unwrap()
            .count,
        2
    );
}

#[test]
fn quick_move_storage_filter_falls_back_to_player_inventory() {
    let mut ledger = ledger_with(&[(0, stack(10, 3))]);
    open(&mut ledger, 4, protocol::WINDOW_TYPE_HORSE);
    ledger.apply(&content(
        ContainerIdentity::window(4),
        vec![NetworkItemStack::empty(); 5],
    ));
    ledger
        .begin_quick_move_with_storage_filter(InventoryTarget::Player(0), |_| false)
        .unwrap();
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 3);
    for slot in 0..5 {
        assert!(
            ledger
                .target_stack(InventoryTarget::Storage(slot))
                .is_none()
        );
    }
}

/// A response without item data cannot restore a superseded enchanting input snapshot.
#[test]
fn enchanting_restatement_survives_a_later_count_response() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 7, WINDOW_TYPE_ENCHANTMENT);
    ledger.apply(&slot_update(named(124, 22), 14, stack(50, 1)));
    ledger.apply(&InventoryEvent::EnchantOptions(EnchantOptionsEvent {
        options: Arc::from([EnchantOption {
            cost: 2,
            name: Arc::from("offer"),
            network_id: 3,
            enchants: Arc::from([(15, 2)]),
        }]),
    }));
    let request = ledger.begin_enchant(3).unwrap();
    assert!(ledger.mark_transport_enqueued(1));
    let extra = protocol::edit_item_extra(
        &[0; 10],
        &protocol::ItemNbtEdit {
            enchantments: Some(vec![(15, 2)]),
            ..Default::default()
        },
    )
    .unwrap();
    let replacement = NetworkItemStack {
        network_id: 7,
        stack_network_id: 70,
        count: 1,
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: Arc::clone(&extra),
        ..NetworkItemStack::empty()
    };
    ledger.apply(&slot_update(named(124, 22), 14, replacement.clone()));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id: request,
                status: protocol::StackResponseStatus::Accepted,
                containers: Arc::from([protocol::StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(22),
                        dynamic_id: None,
                    },
                    slots: Arc::from([protocol::StackResponseSlot {
                        slot: 14,
                        hotbar_slot: 14,
                        count: 1,
                        item_stack_id: 80,
                        custom_name: Arc::from(""),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: 0,
                    }]),
                }]),
            }]),
        },
    ));
    let held = ledger.target_stack(InventoryTarget::Craft(14)).unwrap();
    assert_eq!(held.network_id, replacement.network_id);
    assert_eq!(held.extra_data, replacement.extra_data);
    assert_eq!(ledger.pending_request_count(), 0);
}

/// An admitted grindstone result remains visible and takeable through either UI receipt form.
#[test]
fn server_previewed_grindstone_result_can_be_taken() {
    for whole_content in [false, true] {
        let mut ledger = ledger_with(&[]);
        open(&mut ledger, 7, protocol::WINDOW_TYPE_GRINDSTONE);
        let input = stack(50, 1);
        let result = stack(51, 1);
        if whole_content {
            let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
            slots[16] = input;
            slots[usize::from(protocol::CREATED_OUTPUT_SLOT)] = result.clone();
            ledger.apply(&content(
                ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slots,
            ));
        } else {
            ledger.apply(&slot_update(named(124, 22), 16, input));
            ledger.apply(&slot_update(
                ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                u16::from(protocol::CREATED_OUTPUT_SLOT),
                result.clone(),
            ));
        }
        assert_eq!(ledger.created_output_stack(), Some(&result));
        ledger
            .begin_screen_output(&ScreenCraft::Grindstone {
                recipe_network_id: 50,
                repair_cost: 0,
                output: None,
            })
            .unwrap();
        assert_eq!(ledger.cursor_stack().unwrap().count, 1);
        assert!(ledger.target_stack(InventoryTarget::Craft(16)).is_none());
    }
}

/// An open anvil holding a one-item target and a stack of three materials.
fn anvil_with_inputs() -> (PlayerInventoryLedger, NetworkItemStack) {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 5, WINDOW_TYPE_ANVIL);
    ledger.apply(&slot_update(named(124, 0), 1, stack(30, 1)));
    ledger.apply(&slot_update(named(124, 1), 2, stack(31, 3)));
    (ledger, stack(0, 1))
}

/// Rejects the newest request and asserts the anvil, cursor and inventory are as before.
fn reject_and_assert_restored(ledger: &mut PlayerInventoryLedger, request_id: i32) {
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id,
                status: protocol::StackResponseStatus::Rejected,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(1)),
        Some(&stack(30, 1))
    );
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(2)),
        Some(&stack(31, 3))
    );
    assert!(
        (0..PLAYER_INVENTORY_SLOT_COUNT as u8).all(|slot| ledger.displayed_stack(slot).is_none())
    );
    assert!(!ledger.resync_required());
    assert!(
        ledger.begin_quick_move(InventoryTarget::Craft(2)).is_ok(),
        "the restored material moves again"
    );
}

/// The anvil take carries no result list (a dedicated server crashes on one), consumes the
/// material before the target, and a rejection restores every cell.
#[test]
fn anvil_take_matches_the_native_request_and_rolls_back_on_rejection() {
    let (mut ledger, result) = anvil_with_inputs();
    let request_id = ledger
        .begin_screen_output(&ScreenCraft::Anvil {
            rename: None,
            recipe_network_id: 0,
            output: Some(result),
            material_used: 2,
        })
        .unwrap();
    let slot = |container, slot, stack_network_id| protocol::StackRequestSlot {
        container,
        slot,
        stack_network_id,
    };
    let window = |name| StackRequestContainer::OpenWindow {
        name,
        dynamic_id: None,
    };
    assert_eq!(
        ledger.newest_request().unwrap().actions,
        vec![
            StackRequestAction::CraftRecipeOptional {
                recipe_network_id: 0,
                filtered_string_index: 0,
            },
            StackRequestAction::Consume {
                amount: 2,
                source: slot(window(1), 2, 31),
            },
            StackRequestAction::Consume {
                amount: 1,
                source: slot(window(0), 1, 30),
            },
            StackRequestAction::Take {
                amount: 1,
                source: slot(
                    StackRequestContainer::CreatedOutput,
                    protocol::CREATED_OUTPUT_SLOT,
                    request_id
                ),
                destination: slot(StackRequestContainer::Cursor, 0, 0),
            },
        ]
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(2))
            .unwrap()
            .count,
        1
    );
    reject_and_assert_restored(&mut ledger, request_id);
}

/// A shift-click places the anvil result into the inventory instead of the cursor.
#[test]
fn anvil_shift_take_places_into_the_inventory_and_rolls_back() {
    let (mut ledger, result) = anvil_with_inputs();
    let request_id = ledger
        .begin_screen_output_into(
            &ScreenCraft::Anvil {
                rename: Some(Arc::from("Named")),
                recipe_network_id: 0,
                output: Some(result),
                material_used: 3,
            },
            CraftSink::Inventory,
        )
        .unwrap();
    let request = ledger.newest_request().unwrap();
    let Some(StackRequestAction::Place {
        amount: 1,
        source,
        destination,
    }) = request.actions.last()
    else {
        panic!("the result is placed: {:?}", request.actions);
    };
    assert_eq!(source.container, StackRequestContainer::CreatedOutput);
    assert_eq!(
        destination.container,
        StackRequestContainer::PlayerInventory
    );
    assert_eq!(destination.stack_network_id, 0);
    assert!(
        request
            .actions
            .iter()
            .all(|action| !matches!(action, StackRequestAction::CraftResultsDeprecated { .. }))
    );
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(destination.slot).unwrap().count, 1);
    assert!(ledger.target_stack(InventoryTarget::Craft(2)).is_none());
    reject_and_assert_restored(&mut ledger, request_id);
}

/// The stonecutter and cartography table derive their results on the server, so their takes
/// send no result list; the cartography table names its map through an optional-recipe craft.
#[test]
fn server_derived_screens_send_no_result_list() {
    for (window_type, input) in [
        (protocol::WINDOW_TYPE_STONECUTTER, 3),
        (protocol::WINDOW_TYPE_CARTOGRAPHY, 12),
        (protocol::WINDOW_TYPE_GRINDSTONE, 16),
    ] {
        let mut ledger = ledger_with(&[]);
        open(&mut ledger, 7, window_type);
        let name = protocol::ui_slot_container_name(input).unwrap();
        ledger.apply(&slot_update(
            named(124, name),
            u16::from(input),
            stack(40, 2),
        ));
        let craft = if window_type == protocol::WINDOW_TYPE_GRINDSTONE {
            ledger.apply(&slot_update(
                ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                u16::from(protocol::CREATED_OUTPUT_SLOT),
                stack(41, 1),
            ));
            ScreenCraft::Grindstone {
                recipe_network_id: 40,
                repair_cost: 0,
                output: None,
            }
        } else {
            let output = protocol::RecipeOutput {
                network_id: 6,
                aux: 0,
                count: 1,
                block_runtime_id: 0,
                empty_envelope: false,
            };
            if window_type == protocol::WINDOW_TYPE_CARTOGRAPHY {
                ScreenCraft::Cartography {
                    recipe_network_id: 9,
                    output,
                    name: Arc::from(""),
                }
            } else {
                ScreenCraft::Predicted {
                    recipe_network_id: 9,
                    output,
                }
            }
        };
        ledger.begin_screen_output(&craft).unwrap();
        let sends_results = ledger
            .newest_request()
            .unwrap()
            .actions
            .iter()
            .any(|action| matches!(action, StackRequestAction::CraftResultsDeprecated { .. }));
        assert_eq!(
            sends_results,
            window_type == protocol::WINDOW_TYPE_GRINDSTONE,
            "window type {window_type}"
        );
        if window_type == protocol::WINDOW_TYPE_CARTOGRAPHY {
            let request = ledger.newest_request().unwrap();
            assert_eq!(
                request.actions[0],
                StackRequestAction::CraftRecipeOptional {
                    recipe_network_id: 9,
                    filtered_string_index: 0,
                }
            );
            assert_eq!(request.filter_strings, [String::new()]);
        }
    }
}
