//! Crafting requests ported from the owner's proxy prediction tests.

use crate::{CraftGridMatch, match_crafting_grid};
use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_CRAFT_INPUT, CONTAINER_NAME_CURSOR, ContainerIdentity, ContainerOpenEvent,
    InventoryContentEvent, InventoryEvent, InventorySlotEvent, ItemRegistryEntry,
    ItemRegistryEvent, ItemRegistryVersion, ItemStackResponseEvent, NetworkItemStack,
    RecipeCatalog, SlotIdentity, StackRequestAction, StackRequestContainer, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus, decode_recipe_update,
};

use super::*;

mod action_scopes;
mod auto_allocation;
mod book_output;
#[path = "crafting_tests/creative.rs"]
mod creative;
mod extra_results;
mod item_data;
mod output_identity;

/// The inventory stacks the newest request placed from its created output, in action order.
fn placed_outputs(ledger: &PlayerInventoryLedger) -> Vec<(u8, NetworkItemStack)> {
    ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Place {
                source,
                destination,
                ..
            } if source.container == StackRequestContainer::CreatedOutput
                && destination.container == StackRequestContainer::PlayerInventory =>
            {
                let slot = destination.slot;
                ledger
                    .target_stack(InventoryTarget::Player(slot))
                    .map(|stack| (slot, stack.clone()))
            }
            _ => None,
        })
        .collect()
}

const LOG: i32 = 6;
const PLANKS: i32 = 7;
const COBBLE: i32 = 8;
const FURNACE: i32 = 9;

fn varuint(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn varint(out: &mut Vec<u8>, value: i32) {
    varuint(out, ((value << 1) ^ (value >> 31)) as u32);
}

fn string(out: &mut Vec<u8>, value: &str) {
    varuint(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

/// One crafting-table record in the family's wire grammar.
fn record(
    out: &mut Vec<u8>,
    shape: Option<(i32, i32)>,
    cells: &[Option<&str>],
    output: i32,
    id: u32,
) {
    string(out, "test:recipe");
    if let Some((width, height)) = shape {
        varint(out, width);
        varint(out, height);
    }
    varuint(out, cells.len() as u32);
    for cell in cells {
        match cell {
            Some(name) => {
                varuint(out, 1);
                let (kind, name) = name
                    .strip_prefix("tag:")
                    .map_or(("name", *name), |tag| ("item_tag", tag));
                string(out, kind);
                string(out, name);
                varint(out, 0);
                varint(out, 1);
            }
            None => {
                varuint(out, 0);
                varint(out, 0);
                varint(out, 0);
            }
        }
    }
    varuint(out, 1);
    varint(out, output);
    out.extend_from_slice(&1u16.to_le_bytes());
    varuint(out, 0);
    varint(out, 0);
    varuint(out, 0);
    out.extend_from_slice(&[0; 16]);
    string(out, "crafting_table");
    varint(out, 0);
    if shape.is_some() {
        out.push(0);
    }
    out.push(0);
    varuint(out, id);
}

fn recipes() -> InventoryEvent {
    let mut body = Vec::new();
    varuint(&mut body, 2);
    record(
        &mut body,
        Some((1, 1)),
        &[Some("minecraft:oak_log")],
        PLANKS,
        1,
    );
    let ring: Vec<Option<&str>> = (0..9)
        .map(|index| (index != 4).then_some("minecraft:cobblestone"))
        .collect();
    record(&mut body, Some((3, 3)), &ring, FURNACE, 2);
    varuint(&mut body, 1);
    record(
        &mut body,
        None,
        &[Some("minecraft:oak_log"), Some("minecraft:cobblestone")],
        FURNACE,
        3,
    );
    for _ in 2..11 {
        varuint(&mut body, 0);
    }
    body.push(1);
    InventoryEvent::Recipes(decode_recipe_update(&body).unwrap())
}

fn catalog() -> RecipeCatalog {
    let InventoryEvent::Recipes(update) = recipes() else {
        unreachable!()
    };
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog
}

fn registry() -> ItemRegistryEvent {
    let entry = |network_id, identifier: &str| ItemRegistryEntry {
        identifier: Arc::from(identifier),
        network_id,
        component_based: false,
        version: ItemRegistryVersion::None,
        components: None,
        component_digest: [0; 32],
        negotiated_max_stack_size: Some(64),
        canonical_empty_component_data: true,
        item_tags: std::sync::Arc::from([]),
    };
    ItemRegistryEvent {
        entries: vec![
            entry(LOG, "minecraft:oak_log"),
            entry(PLANKS, "minecraft:oak_planks"),
            entry(COBBLE, "minecraft:cobblestone"),
            entry(FURNACE, "minecraft:furnace"),
        ]
        .into(),
    }
}

fn stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        stack_network_id,
        count,
        ..NetworkItemStack::default()
    }
}

fn craft_slot(slot: u16, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(124),
                slot_type: Some(CONTAINER_NAME_CRAFT_INPUT),
                dynamic_id: None,
            },
            slot,
        },
        stack,
        storage_item: None,
    })
}

fn ledger(window_type: i8) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&registry());
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(vec![
            NetworkItemStack::default();
            PLAYER_INVENTORY_SLOT_COUNT
        ]),
        storage_item: NetworkItemStack::default(),
    }));
    if window_type == WORKBENCH_WINDOW_TYPE {
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(3),
            window_type,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    } else {
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    }
    ledger
}

fn unique(ledger: &PlayerInventoryLedger, catalog: &RecipeCatalog) -> protocol::RecipeHandle {
    let cells = ledger.crafting_grid_cells().unwrap();
    let items: Vec<_> = cells
        .iter()
        .map(|cell| cell.as_ref().map(CraftGridCell::item))
        .collect();
    match match_crafting_grid(catalog, ledger.crafting_grid().width(), &items) {
        CraftGridMatch::Unique(recipe) => recipe,
        other => panic!("expected one recipe, got {other:?}"),
    }
}

#[test]
fn shift_crafted_output_uses_hotbar_before_main_inventory() {
    let catalog = catalog();
    for window_type in [PERSONAL_INVENTORY_WINDOW_TYPE, WORKBENCH_WINDOW_TYPE] {
        for (hotbar_full, partial_slots, expected_slot) in [
            (false, vec![], 0),
            (true, vec![], protocol::HOTBAR_SLOT_COUNT),
            (
                false,
                vec![protocol::HOTBAR_SLOT_COUNT],
                protocol::HOTBAR_SLOT_COUNT,
            ),
            (false, vec![0, protocol::HOTBAR_SLOT_COUNT], 0),
        ] {
            let mut ledger = ledger(window_type);
            let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
            if hotbar_full {
                for (slot, held) in slots[..usize::from(protocol::HOTBAR_SLOT_COUNT)]
                    .iter_mut()
                    .enumerate()
                {
                    *held = stack(COBBLE, 100 + i32::try_from(slot).unwrap(), 64);
                }
            }
            for slot in partial_slots {
                slots[usize::from(slot)] = stack(PLANKS, 200 + i32::from(slot), 63);
            }
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::default(),
            }));
            let craft_slot_index = ledger.crafting_grid().slots().next().unwrap();
            ledger.apply(&craft_slot(u16::from(craft_slot_index), stack(LOG, 300, 1)));
            let recipe = unique(&ledger, &catalog);
            ledger.begin_craft_all(&recipe).unwrap();
            assert!(ledger.view().get(Cell::Craft(craft_slot_index)).is_none());
            assert!(
                matches!(
                    ledger.newest_request().unwrap().actions.last(),
                    Some(StackRequestAction::Place { destination, .. })
                        if destination.slot == expected_slot
                ),
                "expected crafted output in player slot {expected_slot}"
            );
            assert_eq!(
                ledger
                    .view()
                    .get(Cell::Inventory(expected_slot))
                    .unwrap()
                    .stack
                    .network_id,
                PLANKS
            );
        }
    }
}

fn respond(ledger: &mut PlayerInventoryLedger, request_id: i32, rows: &[(u8, u8, u8, i32)]) {
    let containers: Vec<_> = rows
        .iter()
        .map(|(name, slot, count, id)| StackResponseContainer {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(*name),
                dynamic_id: None,
            },
            slots: Arc::from([StackResponseSlot {
                slot: *slot,
                hotbar_slot: *slot,
                count: *count,
                item_stack_id: *id,
                custom_name: Arc::from(""),
                filtered_custom_name: Arc::from(""),
                durability_correction: 0,
            }]),
        })
        .collect();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from(containers),
        }]),
    }));
}

/// The personal flow sends CraftRecipe, CraftResultsDeprecated, Consume and a
/// Take from created output named by the request id, and settles cleanly.
#[test]
fn personal_craft_consumes_inputs_and_takes_output_to_the_cursor() {
    let catalog = catalog();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(29, stack(LOG, 101, 3)));
    let recipe = unique(&ledger, &catalog);
    let request = ledger.begin_craft(&recipe, 1).unwrap();
    let actions = &ledger.newest_request().unwrap().actions;
    assert!(matches!(
        actions[0],
        StackRequestAction::CraftRecipe {
            recipe_network_id: 1,
            crafts: 1
        }
    ));
    assert!(matches!(
        actions[1],
        StackRequestAction::CraftResultsDeprecated { crafts: 1, .. }
    ));
    let StackRequestAction::Consume { amount, source } = actions[2] else {
        panic!("consume");
    };
    assert_eq!((amount, source.slot, source.stack_network_id), (1, 29, 101));
    assert_eq!(source.container, StackRequestContainer::CraftingInput);
    let StackRequestAction::Take {
        amount,
        source,
        destination,
    } = actions[3]
    else {
        panic!("take");
    };
    assert_eq!(
        (amount, source.slot, source.stack_network_id),
        (1, 50, request)
    );
    assert_eq!(destination.container, StackRequestContainer::Cursor);
    assert!(ledger.pending_batch().unwrap().is_some());

    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(29))
            .unwrap()
            .count,
        2
    );
    let held = ledger.cursor_stack().unwrap();
    assert_eq!((held.network_id, held.stack_network_id), (PLANKS, request));
    assert!(ledger.created_output_stack().is_none());
    let mut chained = ledger.clone();
    let next = chained.begin_click(0).unwrap();
    let StackRequestAction::Place { source, .. } = chained.newest_request().unwrap().actions[0]
    else {
        panic!("place")
    };
    assert_eq!(source.stack_network_id, request);
    assert_eq!(next, -5);

    assert!(ledger.mark_transport_enqueued(10));
    respond(
        &mut ledger,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 29, 2, 101),
            (CONTAINER_NAME_CURSOR, 0, 1, 555),
        ],
    );
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 555);
    assert!(!ledger.resync_required());
}

/// Native clears unprocessed sparse predictions instead of inventing output.
#[test]
fn crafted_output_without_a_response_slot_reverts_to_backing_truth() {
    let catalog = catalog();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 1)));
    let request = ledger.begin_craft(&unique(&ledger, &catalog), 1).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    respond(&mut ledger, request, &[]);
    assert!(!ledger.resync_required());
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        1
    );
}

/// A partial match predicts nothing and allocates no request.
#[test]
fn unmatched_ingredients_predict_nothing() {
    let catalog = catalog();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 1)));
    ledger.apply(&craft_slot(29, stack(COBBLE, 102, 1)));
    let shapeless = unique(&ledger, &catalog);
    assert_eq!(shapeless.network_id(), 3);
    ledger.apply(&craft_slot(29, NetworkItemStack::default()));
    assert_eq!(
        ledger.begin_craft(&shapeless, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.next_request_id, -3);

    ledger.apply(&craft_slot(29, stack(COBBLE, 102, 1)));
    ledger.apply(&cursor_content(stack(COBBLE, 103, 1)));
    assert_eq!(
        ledger.begin_craft(&shapeless, 1),
        Err(InventoryGestureError::InvalidRequest),
        "an occupied cursor cannot take the output"
    );
}

/// The workbench grid is UI slots 32..=40 and forms 3x3 recipes.
#[test]
fn workbench_crafts_three_by_three_from_its_grid() {
    let catalog = catalog();
    let mut ledger = ledger(WORKBENCH_WINDOW_TYPE);
    assert_eq!(ledger.crafting_grid(), CraftingGrid::Workbench);
    for (index, slot) in (32..41).enumerate() {
        if index != 4 {
            ledger.apply(&craft_slot(slot, stack(COBBLE, 200 + slot as i32, 1)));
        }
    }
    let recipe = unique(&ledger, &catalog);
    assert_eq!(recipe.network_id(), 2);
    let request = ledger.begin_craft(&recipe, 1).unwrap();
    let consumed: Vec<u8> = ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Consume { source, .. } => Some(source.slot),
            _ => None,
        })
        .collect();
    assert_eq!(consumed, [32, 33, 34, 35, 37, 38, 39, 40]);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, request);
    for slot in 32..41 {
        assert!(ledger.target_stack(InventoryTarget::Craft(slot)).is_none());
    }
}

/// Restates the cursor through its fixed UI inventory slot.
fn cursor_content(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack,
        storage_item: None,
    })
}

/// Close returns real inputs, and the close follows them without waiting for an answer.
#[test]
fn closing_a_crafting_screen_returns_inputs_before_close_admission() {
    let mut personal = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    personal.apply(&craft_slot(28, stack(LOG, 101, 1)));
    personal.request_personal_close();
    let request = personal.newest_request().unwrap().request_id;
    assert!(matches!(personal.newest_request().unwrap().actions[0],
        StackRequestAction::Place { amount: 1, source, destination }
            if source.container == StackRequestContainer::CraftingInput
                && source.slot == 28 && destination.slot == 0));
    assert!(
        personal.confirmed.get(Cell::Craft(28)).is_some(),
        "not optimistically cleared"
    );
    assert!(personal.mark_transport_enqueued(10));
    let (close, _) = personal.pending_batch().unwrap().unwrap();
    assert_eq!(
        format!("{:?}", close.kind()),
        "ContainerClosePacket",
        "the close follows its returns at once"
    );
    assert!(personal.mark_transport_enqueued(20));
    respond(
        &mut personal,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 28, 0, -1),
            (
                protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                1,
                101,
            ),
        ],
    );
    personal.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(personal.target_stack(InventoryTarget::Craft(28)).is_none());
    assert!(
        personal
            .crafting_grid_cells()
            .unwrap()
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(
        personal.displayed_stack(0).map(|stack| stack.count),
        Some(1)
    );
    assert!(personal.request_personal_open(42));

    let mut workbench = ledger(WORKBENCH_WINDOW_TYPE);
    workbench.apply(&craft_slot(36, stack(COBBLE, 201, 1)));
    workbench.request_storage_close();
    let request = workbench.newest_request().unwrap().request_id;
    assert!(workbench.target_stack(InventoryTarget::Craft(36)).is_none());
    assert!(workbench.confirmed.get(Cell::Craft(36)).is_some());
    assert!(workbench.mark_transport_enqueued(10));
    respond(
        &mut workbench,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 36, 0, -1),
            (
                protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                1,
                201,
            ),
        ],
    );
    assert_eq!(workbench.crafting_grid(), CraftingGrid::Personal);
    assert_eq!(
        workbench.displayed_stack(0).map(|stack| stack.count),
        Some(1)
    );
}

/// A retired workbench request cannot commit its predicted ingredient return.
#[test]
fn workbench_close_ack_retires_the_unanswered_return() {
    let mut workbench = ledger(WORKBENCH_WINDOW_TYPE);
    workbench.apply(&craft_slot(36, stack(COBBLE, 201, 1)));
    workbench.request_storage_close();
    let request = workbench.newest_request().unwrap().request_id;
    assert!(workbench.mark_transport_enqueued(10));
    assert!(workbench.mark_transport_enqueued(11));
    workbench.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: WORKBENCH_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(workbench.storage_generation().is_none());
    respond(
        &mut workbench,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 36, 0, -1),
            (
                protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                1,
                201,
            ),
        ],
    );
    assert_eq!(workbench.displayed_stack(0).map(|stack| stack.count), None);
    assert_eq!(
        workbench
            .target_stack(InventoryTarget::Craft(36))
            .map(|stack| stack.count),
        Some(1)
    );
    assert!(!workbench.resync_required());
}

#[test]
fn ordinary_block_identity_does_not_make_a_crafting_ingredient_unplain() {
    let catalog = catalog();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut log = stack(LOG, 101, 8);
    log.block_runtime_id = 123;
    ledger.apply(&craft_slot(28, log));
    let recipe = unique(&ledger, &catalog);
    let request = ledger.begin_craft(&recipe, 1).unwrap();
    assert!(matches!(ledger.newest_request().unwrap().actions[2],
        StackRequestAction::Consume { amount: 1, source }
            if source.slot == 28 && source.stack_network_id == 101));
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, request);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        7
    );
}

#[test]
fn close_merges_partial_player_stack_then_returns_remainder_to_an_empty_slot() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 0,
        },
        stack: stack(LOG, 100, 60),
        storage_item: None,
    }));
    ledger.apply(&craft_slot(28, stack(LOG, 101, 8)));
    ledger.request_personal_close();
    let request = ledger.newest_request().unwrap();
    assert_eq!(request.actions.len(), 2);
    assert!(
        matches!(request.actions[0], StackRequestAction::Place { amount: 4, destination, .. }
        if destination.slot == 0 && destination.stack_network_id == 100)
    );
    assert!(
        matches!(request.actions[1], StackRequestAction::Place { amount: 4, destination, .. }
        if destination.slot == 1 && destination.stack_network_id == 0)
    );
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 4);
    assert!(ledger.confirmed.get(Cell::Craft(28)).is_some());
}

#[test]
fn close_drops_only_the_remainder_when_the_player_inventory_is_full() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: (0..PLAYER_INVENTORY_SLOT_COUNT)
            .map(|slot| stack(COBBLE, slot as i32 + 1, 64))
            .collect::<Vec<_>>()
            .into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&craft_slot(28, stack(LOG, 101, 8)));
    ledger.request_personal_close();
    assert!(
        matches!(ledger.newest_request().unwrap().actions.as_slice(),
        [StackRequestAction::Drop { amount: 8, source, randomly: false }]
            if source.container == StackRequestContainer::CraftingInput && source.slot == 28)
    );
    assert_eq!(
        (0..PLAYER_INVENTORY_SLOT_COUNT as u8)
            .map(|slot| ledger.displayed_stack(slot).unwrap().count as usize)
            .sum::<usize>(),
        PLAYER_INVENTORY_SLOT_COUNT * 64
    );
    assert!(ledger.confirmed.get(Cell::Craft(28)).is_some());
}

/// An input that cannot be returned never keeps the screen open; the ledger keeps the item.
#[test]
fn invalid_close_input_identity_still_closes_and_keeps_the_grid() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 0, 8)));
    ledger.request_personal_close();
    assert!(!ledger.personal_inventory_desired_open());
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.pending_control_packet().unwrap().is_some());
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        8
    );
}

/// A refused return arrives after its close; the ingredient stays where the server kept it.
#[test]
fn refused_close_return_after_the_close_preserves_the_ingredient() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 8)));
    ledger.request_personal_close();
    let request_id = ledger.newest_request().unwrap().request_id;
    assert!(ledger.mark_transport_enqueued(10));
    assert!(ledger.mark_transport_enqueued(11));
    assert!(ledger.pending_control_packet().unwrap().is_none());
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Rejected,
            request_id,
            containers: Arc::from([]),
        }]),
    }));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        8
    );
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(!ledger.personal_inventory_desired_open());
    assert!(ledger.confirmed.get(Cell::Craft(28)).is_some());

    // A later close builds a fresh return from the still-confirmed ingredient.
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(3),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert!(ledger.personal_inventory_desired_open());
    ledger.request_personal_close();
    assert!(!ledger.personal_inventory_desired_open());
    let (returning, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(format!("{:?}", returning.kind()), "ItemStackRequestPacket");
    assert!(ledger.mark_transport_enqueued(30));
    let (close, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(format!("{:?}", close.kind()), "ContainerClosePacket");
}

#[test]
fn an_empty_sparse_grid_does_not_cancel_its_unanswered_transfer_on_close() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 8)));
    ledger
        .begin_target_gesture(InventoryTarget::Craft(28), CellGesture::Click)
        .unwrap();
    let transfer = ledger.begin_click(0).unwrap();
    assert!(ledger.target_stack(InventoryTarget::Craft(28)).is_none());
    assert!(ledger.cursor_stack().is_none());
    ledger.request_personal_close();
    assert_eq!(ledger.pending_request_count(), 2);
    assert_eq!(ledger.newest_request().unwrap().request_id, transfer);
    assert!(ledger.confirmed.get(Cell::Craft(28)).is_some());
}

// The recipe book's filter shows recipes the inventory holds some ingredient of.
#[test]
fn a_held_ingredient_marks_uncraftable_recipes_as_partly_held() {
    let catalog = catalog();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[9] = stack(LOG, 101, 1);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    let by_id = |id: u32| {
        catalog
            .crafting_handles()
            .into_iter()
            .find(|recipe| recipe.network_id() == id)
            .unwrap()
    };
    let (planks, ring, log_and_cobble) = (by_id(1), by_id(2), by_id(3));
    assert!(ledger.can_auto_craft(&planks));
    assert!(!ledger.can_auto_craft(&log_and_cobble));
    assert!(ledger.holds_any_ingredient(&log_and_cobble));
    assert!(!ledger.holds_any_ingredient(&ring));
}

#[test]
fn rebased_crafting_preserves_same_request_output_references() {
    use protocol::test_support::recipes::{StackActionObservation, stack_request_actions};

    let catalog = catalog();
    let recipe = catalog
        .crafting_handles()
        .into_iter()
        .find(|recipe| recipe.network_id() == 1)
        .unwrap();
    for automatic in [false, true] {
        let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
        let (target, container, slot) = if automatic {
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 2,
                },
                stack: stack(LOG, 101, 3),
                storage_item: None,
            }));
            (
                InventoryTarget::Player(2),
                protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                2,
            )
        } else {
            ledger.apply(&craft_slot(29, stack(LOG, 101, 3)));
            (InventoryTarget::Craft(29), CONTAINER_NAME_CRAFT_INPUT, 29)
        };
        let predecessor = ledger
            .begin_drop(DropSource::Target(target), Some(1))
            .unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        let craft = if automatic {
            ledger.begin_auto_craft(&recipe, false)
        } else {
            ledger.begin_craft(&recipe, 1)
        }
        .unwrap();
        respond(&mut ledger, predecessor, &[(container, slot, 2, 91)]);
        let (packet, count) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(count, 1);
        let actions = stack_request_actions(&packet).unwrap();
        assert!(actions.iter().any(|action| matches!(action,
            StackActionObservation::Consume { source, .. } if source.stack_network_id == 91
        )));
        assert_eq!(ledger.target_stack(target).unwrap().count, 1);
        if automatic {
            let placed = placed_outputs(&ledger);
            assert_eq!(placed.len(), 1);
            let (output_slot, output) = &placed[0];
            assert_eq!((output.network_id, output.count), (PLANKS, 1));
            assert!(ledger.cursor_stack().is_none());
            assert!(ledger.mark_transport_enqueued(21));
            respond(
                &mut ledger,
                craft,
                &[(container, slot, 1, 92), (container, *output_slot, 1, 93)],
            );
            assert_eq!(
                ledger
                    .target_stack(InventoryTarget::Player(*output_slot))
                    .unwrap()
                    .stack_network_id,
                93
            );
        } else {
            assert!(
                matches!(actions.last(), Some(StackActionObservation::Take { source, .. })
                if source.stack_network_id == craft)
            );
            assert_eq!(ledger.cursor_stack().unwrap().network_id, PLANKS);
            assert_eq!(ledger.cursor_stack().unwrap().count, 1);
            assert!(ledger.mark_transport_enqueued(21));
            respond(
                &mut ledger,
                craft,
                &[(container, slot, 1, 92), (CONTAINER_NAME_CURSOR, 0, 1, 93)],
            );
            assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 93);
        }
        assert!(!ledger.resync_required());
    }
}

#[test]
fn review_overlapping_ingredients_find_a_complete_assignment() {
    let mut body = Vec::new();
    varuint(&mut body, 0);
    varuint(&mut body, 1);
    record(
        &mut body,
        None,
        &[Some("tag:test:wood"), Some("minecraft:oak_log")],
        FURNACE,
        77,
    );
    for _ in 2..11 {
        varuint(&mut body, 0);
    }
    body.push(1);
    let update = decode_recipe_update(&body).unwrap();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let recipe = catalog.crafting_handles().pop().unwrap();
    for all in [false, true] {
        let mut ledger = ledger(0);
        let mut items = registry();
        let mut entries = items.entries.to_vec();
        for entry in &mut entries {
            if [LOG, PLANKS].contains(&entry.network_id) {
                entry.item_tags = Arc::from([Arc::from("test:wood")]);
            }
        }
        items.entries = entries.into();
        ledger.apply_registry(&items);
        ledger.apply(&craft_slot(28, stack(LOG, 101, 2)));
        ledger.apply(&craft_slot(29, stack(PLANKS, 102, 2)));
        let result = if all {
            ledger.begin_craft_all(&recipe)
        } else {
            ledger.begin_craft_into(&recipe, 1, crafting::CraftSink::Cursor)
        };
        assert!(result.is_ok(), "{result:?}");
    }
}

#[path = "crafting_tests/batches.rs"]
mod batches;
