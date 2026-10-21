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
