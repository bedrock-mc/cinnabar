use std::sync::Arc;

use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};
use ::protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
};

use super::*;

fn fixture(window_type: i8) -> InventorySession {
    let mut inventory = InventorySession::new(1);
    inventory.publish_bootstrap_inventory(
        Some(ItemRegistryEvent {
            entries: [
                "minecraft:oak_log",
                "minecraft:birch_log",
                "minecraft:charcoal",
                "minecraft:iron_ingot",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, name)| ItemRegistryEntry {
                identifier: name.into(),
                network_id: index as i32 + 1,
                component_based: false,
                version: ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            })
            .collect::<Vec<_>>()
            .into(),
        }),
        InventoryEvent::Authority(InventoryAuthority::Server),
    );
    let ledger = inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(7),
        slots: vec![NetworkItemStack::empty(); 3].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    let mut stacks =
        vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)];
    stacks[5] = NetworkItemStack {
        network_id: 2,
        count: 8,
        stack_network_id: 25,
        ..NetworkItemStack::empty()
    };
    stacks[6] = NetworkItemStack {
        network_id: 2,
        count: 7,
        stack_network_id: 26,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: stacks.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    let recipes = [
        ("minecraft:oak_log", 3, "furnace"),
        ("minecraft:birch_log", 3, "furnace"),
        ("minecraft:raw_iron", 4, "furnace"),
        ("minecraft:birch_log", 3, "smoker"),
    ];
    let packet = CraftingDataPacket {
        shapeless_recipes: recipes
            .into_iter()
            .enumerate()
            .map(|(index, (name, output, tag))| ShapelessRecipePayload {
                ingredients: vec![CerealizerRecipeIngredientSerializedData {
                    descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                        key: "name".into(),
                        value: name.into(),
                    }],
                    aux_value: 0,
                    stack_size: 1,
                }],
                results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
                    id: output,
                    stacksize: 1,
                    auxvalue: 0,
                    block_runtime_id: 0,
                    user_data_buffer: vec![],
                }],
                tag: tag.into(),
                net_id: TypedServerNetIdstructRecipeNetIdTag {
                    raw_id: index as u32 + 1,
                },
                ..Default::default()
            })
            .collect(),
        clear_recipes: true,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    inventory
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Recipes(::protocol::decode_recipe_update(&bytes).unwrap()),
        )
        .unwrap();
    inventory.synchronize_crafting_frontier(1, Some((1, 0, Some(1))));
    inventory.drain_pending_inventory();
    inventory
}

#[test]
fn furnace_results_deduplicate_alternatives_and_filter_by_the_active_station() {
    let inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let listed = inventory.furnace_recipes(false);
    assert_eq!(
        listed
            .iter()
            .map(|recipe| recipe.output.unwrap().network_id)
            .collect::<Vec<_>>(),
        [3, 4]
    );
    assert_eq!(&*listed[0].ingredients[0].name, "minecraft:birch_log");
    assert_eq!(inventory.furnace_recipes(true).len(), 1);
    assert_eq!(
        fixture(::protocol::WINDOW_TYPE_SMOKER)
            .furnace_recipes(false)
            .len(),
        1
    );
    assert!(
        fixture(::protocol::WINDOW_TYPE_BLAST_FURNACE)
            .furnace_recipes(false)
            .is_empty()
    );
}

#[test]
fn furnace_recipe_selection_places_a_fuel_item_in_the_ingredient_role() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(true)[0].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let ingredient = inventory
        .ledger()
        .storage_stack(0)
        .expect("loaded furnace input");
    assert_eq!(ingredient.network_id, 2);
    assert_eq!(ingredient.count, 15);
    assert!(inventory.ledger().displayed_stack(5).is_none());
    assert!(inventory.ledger().displayed_stack(6).is_none());
    assert!(inventory.ledger().storage_stack(1).is_none());
}

#[test]
fn furnace_recipe_transfer_skips_an_incompatible_accepted_variant() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let ledger = inventory.ledger_mut();
    for (slot, metadata) in [(6, 1), (7, 0)] {
        ledger.apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(0),
                slot,
            },
            stack: NetworkItemStack {
                network_id: 2,
                metadata,
                count: 8,
                stack_network_id: i32::from(slot) + 30,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    }
    let mut recipe = inventory.furnace_recipes(true)[0].clone();
    recipe.ingredients[0].aux = ::protocol::RECIPE_ANY_AUX;
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 16);
    assert!(inventory.ledger().displayed_stack(7).is_none());
    assert!(inventory.ledger().displayed_stack(6).is_some());
}

#[test]
fn furnace_recipe_transfer_tops_up_after_an_incompatible_first_variant() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let ledger = inventory.ledger_mut();
    for (container, slot, metadata) in [(7, 0, 0), (0, 5, 1)] {
        ledger.apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(container),
                slot,
            },
            stack: NetworkItemStack {
                network_id: 2,
                metadata,
                count: 8,
                stack_network_id: i32::from(slot) + 40,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    }
    let mut recipe = inventory.furnace_recipes(true)[0].clone();
    recipe.ingredients[0].aux = ::protocol::RECIPE_ANY_AUX;
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 15);
    assert!(inventory.ledger().displayed_stack(6).is_none());
    assert_eq!(inventory.ledger().displayed_stack(5).unwrap().count, 8);
}

#[test]
fn furnace_recipe_projection_reuses_unchanged_inputs() {
    let inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let first = inventory.furnace_recipes(false);
    let second = inventory.furnace_recipes(false);
    assert_eq!(
        first.shared_indices().as_ptr(),
        second.shared_indices().as_ptr(),
        "unchanged furnace inputs reuse the recipe projection"
    );
}

#[test]
fn furnace_recipe_projection_invalidates_when_supply_changes() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let previous = Arc::clone(inventory.furnace_recipes(true).shared_indices());
    for slot in [5, 6] {
        inventory
            .ledger_mut()
            .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
                identity: ::protocol::SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot,
                },
                stack: NetworkItemStack::empty(),
                storage_item: None,
            }));
    }
    let listed = inventory.furnace_recipes(true);
    assert!(listed.is_empty());
    assert!(!Arc::ptr_eq(&previous, listed.shared_indices()));
}
