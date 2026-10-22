use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent, InventorySlotEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, SlotIdentity,
    StackRequestAction as Action, StackRequestContainer as Container, StackRequestSlot,
};
use ui_screens::container_actions::InventoryCellHit;

use super::*;
use crate::ui_runtime::inventory_drag::PointerAction;

const SWORD: i32 = 10;
const SHARPNESS: (i16, i16) = (9, 3);
const VANISHING: (i16, i16) = (28, 1);

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

/// An open grindstone holding a Sharpness III, Vanishing sword in its top slot.
fn grindstone() -> (UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([entry("minecraft:diamond_sword", SWORD)]),
    });
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)]
            .into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(5),
        window_type: protocol::WINDOW_TYPE_GRINDSTONE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let extra = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            enchantments: Some(vec![SHARPNESS, VANISHING]),
            repair_cost: Some(5),
            ..Default::default()
        },
    )
    .unwrap();
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(124),
                slot_type: protocol::ui_slot_container_name(16),
                dynamic_id: None,
            },
            slot: 16,
        },
        stack: NetworkItemStack {
            network_id: SWORD,
            stack_network_id: 30,
            count: 1,
            nbt_digest: Sha256::digest(&extra).into(),
            extra_data: extra,
            ..NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    (runtime, player)
}

/// The server previews no grindstone result, so the client shows the disenchanted item.
#[test]
fn the_result_keeps_curses_and_shows_without_a_server_preview() {
    let (runtime, player) = grindstone();
    let preview = runtime.grindstone_preview(&player).unwrap();
    let data = protocol::item_anvil_data(&preview.stack.extra_data).unwrap();
    assert_eq!(data.enchantments, [VANISHING]);
    assert_eq!(protocol::item_repair_cost(&preview.stack.extra_data), 1);
    assert_eq!(preview.source_stack_id, 30);
    assert_eq!(runtime.grindstone_output(&player), Some(preview.stack));
}

/// Taking the result names the copied input's stack id and the result's penalty, and a
/// shift-click places it into the inventory.
#[test]
fn takes_name_the_source_stack_and_penalty() {
    for shift in [false, true] {
        let (mut runtime, mut player) = grindstone();
        let result = runtime.grindstone_preview(&player).unwrap().stack;
        let action = if shift {
            PointerAction::QuickMove(InventoryCellHit::CraftOutput)
        } else {
            PointerAction::Click(InventoryCellHit::CraftOutput)
        };
        runtime.perform_pointer_action(&mut player, action);
        let ledger = runtime.inventory_ledger(&player);
        let (packet, requests) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(requests, 1, "shift: {shift}");
        let request_id = ledger.pending_request_id().unwrap();
        let output = StackRequestSlot {
            container: Container::CreatedOutput,
            slot: protocol::CREATED_OUTPUT_SLOT,
            stack_network_id: request_id,
        };
        let take = if shift {
            Action::Place {
                amount: 1,
                source: output,
                destination: StackRequestSlot {
                    container: Container::PlayerInventory,
                    slot: 0,
                    stack_network_id: 0,
                },
            }
        } else {
            Action::Take {
                amount: 1,
                source: output,
                destination: StackRequestSlot {
                    container: Container::Cursor,
                    slot: 0,
                    stack_network_id: 0,
                },
            }
        };
        let expected = protocol::item_stack_request_packet_filtered(
            request_id,
            &[
                Action::Grindstone {
                    recipe_network_id: 30,
                    crafts: 1,
                    repair_cost: 1,
                },
                Action::CraftResultsDeprecated {
                    results: Arc::from([protocol::CraftResult {
                        identifier: Arc::from("minecraft:diamond_sword"),
                        aux: 0,
                        count: 1,
                        block_runtime_id: 0,
                        user_data: Arc::clone(&result.extra_data),
                    }]),
                    crafts: 1,
                },
                Action::Consume {
                    amount: 1,
                    source: StackRequestSlot {
                        container: protocol::ui_slot_request_container(16).unwrap(),
                        slot: 16,
                        stack_network_id: 30,
                    },
                },
                take,
            ],
            &[],
        )
        .unwrap();
        assert_eq!(packet, expected, "shift: {shift}");
    }
}

/// Puts `stack` in grindstone UI `slot`.
fn place(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    slot: u8,
    stack: NetworkItemStack,
) {
    runtime
        .inventory_ledger_mut(player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(124),
                    slot_type: protocol::ui_slot_container_name(slot),
                    dynamic_id: None,
                },
                slot: u16::from(slot),
            },
            stack,
            storage_item: None,
        }));
}

/// A damaged sword's user data, optionally marked Unbreakable.
fn sword_data(unbreakable: bool) -> Arc<[u8]> {
    // Root compound with an optional `Unbreakable` byte, then the empty restriction lists.
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0];
    if unbreakable {
        extra.extend_from_slice(&[1, 11, 0]);
        extra.extend_from_slice(b"Unbreakable");
        extra.push(1);
    }
    extra.push(0);
    extra.extend_from_slice(&[0; 8]);
    protocol::edit_item_extra(
        &extra,
        &protocol::ItemNbtEdit {
            damage: Some(900),
            enchantments: Some(vec![SHARPNESS]),
            ..Default::default()
        },
    )
    .unwrap()
}

fn sword(stack_network_id: i32, extra_data: Arc<[u8]>) -> NetworkItemStack {
    NetworkItemStack {
        network_id: SWORD,
        stack_network_id,
        count: 1,
        nbt_digest: Sha256::digest(&extra_data).into(),
        extra_data,
        ..NetworkItemStack::empty()
    }
}

/// An Unbreakable copy cannot join a durability merge, but alone it is still disenchanted.
#[test]
fn unbreakable_items_only_disenchant() {
    for unbreakable in [false, true] {
        let (mut runtime, mut player) = grindstone();
        place(
            &mut runtime,
            &mut player,
            16,
            sword(30, sword_data(unbreakable)),
        );
        place(&mut runtime, &mut player, 17, sword(31, sword_data(false)));
        assert_eq!(
            runtime.grindstone_preview(&player).is_some(),
            !unbreakable,
            "unbreakable: {unbreakable}"
        );
        place(&mut runtime, &mut player, 17, NetworkItemStack::empty());
        assert!(runtime.grindstone_preview(&player).is_some());
    }
}

/// Only a compound marker counts as user data, whatever trails the envelope.
#[test]
fn user_data_is_read_from_the_compound_marker() {
    assert!(!has_compound(&[]));
    assert!(!has_compound(&[0; 10]));
    let mut shield = vec![0; 10];
    shield.extend_from_slice(&7i64.to_le_bytes());
    assert!(!has_compound(&shield), "a blocking tick is not user data");
    assert!(has_compound(&sword_data(false)));
}

/// The prediction is retained until an input changes.
#[test]
fn predictions_are_reused_until_an_input_changes() {
    let (mut runtime, mut player) = grindstone();
    let first = runtime.grindstone_preview(&player).unwrap();
    assert!(runtime.grindstone_cache.0.lock().unwrap().is_some());
    assert_eq!(runtime.grindstone_preview(&player), Some(first.clone()));
    place(&mut runtime, &mut player, 16, sword(32, sword_data(false)));
    let changed = runtime.grindstone_preview(&player).unwrap();
    assert_eq!(changed.source_stack_id, 32);
    assert_ne!(changed.stack.extra_data, first.stack.extra_data);
}

/// A component item declaring a zero maximum is damageable, so two copies still merge.
#[test]
fn declared_zero_durability_items_merge() {
    let (mut runtime, mut player) = grindstone();
    let mut sword_entry = entry("minecraft:diamond_sword", SWORD);
    sword_entry.component_based = true;
    sword_entry.components = Some(Arc::new(protocol::ItemComponents {
        durability_declared: true,
        max_durability: Some(0),
        ..Default::default()
    }));
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([sword_entry]),
        });
    place(&mut runtime, &mut player, 16, sword(30, sword_data(false)));
    place(&mut runtime, &mut player, 17, sword(31, sword_data(false)));
    let preview = runtime.grindstone_preview(&player).unwrap();
    assert_eq!(protocol::item_stack_damage(&preview.stack), Some(0));
}
