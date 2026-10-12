use std::sync::Arc;

use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
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
                "minecraft:raw_iron",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, name)| ItemRegistryEntry {
                identifier: name.into(),
                network_id: index as i32 + 1,
                component_based: false,
                version: ItemRegistryVersion::None,
                components: None,
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
    apply_recipes(&mut inventory, &FIXTURE_RECIPES, 1);
    inventory
}

/// Ingredient, output network id and station tag of the fixture's furnace-family recipes.
const FIXTURE_RECIPES: [(&str, i32, &str); 4] = [
    ("minecraft:oak_log", 3, "furnace"),
    ("minecraft:birch_log", 3, "furnace"),
    ("minecraft:raw_iron", 4, "furnace"),
    ("minecraft:birch_log", 3, "smoker"),
];

/// Commits a clearing recipe update carrying exactly `recipes` at `sequence`.
fn apply_recipes(inventory: &mut InventorySession, recipes: &[(&str, i32, &str)], sequence: u64) {
    let update = CraftingDataFixture::clearing(
        recipes
            .iter()
            .enumerate()
            .map(|(index, &(name, output, tag))| RecipeFixture {
                tag: tag.into(),
                net_id: index as u32 + 1,
                ..RecipeFixture::shapeless(
                    "",
                    vec![IngredientFixture::named(name, 0)],
                    vec![OutputFixture::new(output, 1)],
                )
            })
            .collect(),
    )
    .update();
    inventory
        .enqueue_inventory_event(1, sequence, InventoryEvent::Recipes(update))
        .unwrap();
    inventory.synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
    inventory.drain_pending_inventory();
}

#[test]
fn furnace_unsupplied_recipe_shows_ghosts_and_repeat_selection_deselects() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(false)[1].clone();
    assert_eq!(recipe.output.unwrap().network_id, 4);
    assert_eq!(inventory.ledger_mut().begin_furnace_recipe(&recipe), Ok(0));
    let ledger = inventory.ledger();
    assert_eq!(
        ledger
            .selected_furnace_result()
            .map(|stack| stack.network_id),
        Some(4)
    );
    assert_eq!(
        ledger.furnace_ghost_stack(2).map(|stack| stack.network_id),
        Some(4)
    );
    assert!(
        ledger.storage_stack(2).is_none(),
        "a ghost is never an inventory item"
    );
    assert_eq!(inventory.ledger_mut().begin_furnace_recipe(&recipe), Ok(0));
    assert!(inventory.ledger().selected_furnace_result().is_none());
    assert!(inventory.ledger().furnace_ghost_stack(2).is_none());
}

#[test]
fn furnace_repeated_supplied_selection_returns_original_sources_before_deselecting() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(true)[0].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 15);
    assert!(inventory.ledger().displayed_stack(5).is_none());
    assert!(inventory.ledger().displayed_stack(6).is_none());
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let ledger = inventory.ledger();
    assert!(ledger.storage_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 8);
    assert_eq!(ledger.displayed_stack(6).unwrap().count, 7);
    assert!(ledger.selected_furnace_result().is_none());
    assert!(ledger.furnace_ghost_stack(0).is_none());
    assert!(ledger.furnace_ghost_stack(2).is_none());
}

#[test]
fn furnace_replacement_restores_original_source_counts_before_showing_ghosts() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let supplied = inventory.furnace_recipes(true)[0].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&supplied)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 15);
    let other = inventory.furnace_recipes(false)[1].clone();
    inventory.ledger_mut().begin_furnace_recipe(&other).unwrap();
    let ledger = inventory.ledger();
    assert!(ledger.storage_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 8);
    assert_eq!(ledger.displayed_stack(6).unwrap().count, 7);
    assert_eq!(
        ledger.furnace_ghost_stack(2).map(|stack| stack.network_id),
        Some(4)
    );
}

#[test]
fn furnace_loaded_alternative_keeps_the_result_available_without_source_priority() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(true)[0].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(
        inventory.furnace_recipes(false)[0].ingredients[0]
            .name
            .as_ref(),
        "minecraft:oak_log"
    );
    assert_eq!(inventory.furnace_recipes(true).len(), 1);
}

/// A replacement variant follows inventory return order rather than unrelated saved cells.
fn assert_changed_furnace_item_returns_normally(extra_data: Arc<[u8]>, block_runtime_id: i32) {
    use sha2::{Digest, Sha256};

    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(true)[0].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    for _ in 0..inventory.ledger().pending_request_count() {
        let ledger = inventory.ledger_mut();
        let request_id = ledger.pending_request_id().unwrap();
        assert!(ledger.mark_transport_enqueued(1));
        ledger.apply(&InventoryEvent::Response(
            ::protocol::ItemStackResponseEvent {
                responses: Arc::from([::protocol::StackResponse {
                    request_id,
                    status: ::protocol::StackResponseStatus::Accepted,
                    containers: Arc::from([]),
                }]),
            },
        ));
    }
    assert_eq!(inventory.ledger().pending_request_count(), 0);
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)]
                .into(),
            storage_item: NetworkItemStack::empty(),
        }));
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(7),
                slot: 0,
            },
            stack: NetworkItemStack {
                network_id: 2,
                count: 4,
                stack_network_id: 45,
                block_runtime_id,
                nbt_digest: Sha256::digest(&extra_data).into(),
                extra_data: extra_data.clone(),
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    let current = inventory.ledger().storage_stack(0).unwrap();
    assert_eq!(current.extra_data, extra_data);
    assert_eq!(current.block_runtime_id, block_runtime_id);
    let other = inventory.furnace_recipes(false)[1].clone();
    inventory.ledger_mut().begin_furnace_recipe(&other).unwrap();
    let ledger = inventory.ledger();
    let returned = ledger
        .displayed_stack(0)
        .expect("replacement returns to first inventory cell");
    assert_eq!(
        (
            returned.network_id,
            returned.count,
            returned.block_runtime_id
        ),
        (2, 4, block_runtime_id)
    );
    assert_eq!(returned.extra_data, extra_data);
    assert!(ledger.displayed_stack(5).is_none());
    assert!(ledger.displayed_stack(6).is_none());
}

#[test]
fn furnace_changed_nbt_does_not_restore_an_unrelated_source() {
    assert_changed_furnace_item_returns_normally(
        Arc::from([
            255, 255, 1, 10, 0, 0, 1, 1, 0, b'v', 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]),
        0,
    );
}

#[test]
fn furnace_changed_block_identity_does_not_restore_an_unrelated_source() {
    assert_changed_furnace_item_returns_normally(Arc::from([]), 91);
}

/// Encodes one placement or breaking restriction in the item's carried data.
fn restricted_furnace_item_data(place: bool) -> Arc<[u8]> {
    let name = b"minecraft:stone";
    let mut data = vec![0, 0];
    for present in [place, !place] {
        data.extend_from_slice(&i32::from(present).to_le_bytes());
        if present {
            data.extend_from_slice(&(name.len() as u16).to_le_bytes());
            data.extend_from_slice(name);
        }
    }
    data.into()
}

#[test]
fn furnace_changed_placement_restriction_does_not_restore_an_unrelated_source() {
    assert_changed_furnace_item_returns_normally(restricted_furnace_item_data(true), 0);
}

#[test]
fn furnace_changed_breaking_restriction_does_not_restore_an_unrelated_source() {
    assert_changed_furnace_item_returns_normally(restricted_furnace_item_data(false), 0);
}

#[test]
fn furnace_loaded_only_selection_returns_input_and_previews_the_recipe() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(false)[1].clone();
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(7),
                slot: 0,
            },
            stack: NetworkItemStack {
                network_id: 5,
                count: 8,
                stack_network_id: 45,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    assert!(inventory.ledger().can_supply_furnace_recipe(&recipe));
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let ledger = inventory.ledger();
    assert!(ledger.storage_stack(0).is_none());
    let returned = ledger.displayed_stack(0).unwrap();
    assert_eq!((returned.network_id, returned.count), (5, 8));
    assert_eq!(ledger.furnace_ghost_stack(0).unwrap().network_id, 5);
    assert_eq!(ledger.furnace_ghost_stack(2).unwrap().network_id, 4);
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 8);
    assert_eq!(ledger.displayed_stack(6).unwrap().count, 7);
}

#[test]
fn furnace_count_changes_reselect_the_highest_count_alternative() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 0,
            },
            stack: NetworkItemStack {
                network_id: 1,
                count: 8,
                stack_network_id: 40,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    assert_eq!(
        inventory.furnace_recipes(false)[0].ingredients[0]
            .name
            .as_ref(),
        "minecraft:birch_log"
    );
    for slot in [5, 6] {
        let mut stack = inventory.ledger().displayed_stack(slot).unwrap().clone();
        stack.count = 1;
        inventory
            .ledger_mut()
            .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
                identity: ::protocol::SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: u16::from(slot),
                },
                stack,
                storage_item: None,
            }));
    }
    assert_eq!(
        inventory.furnace_recipes(false)[0].ingredients[0]
            .name
            .as_ref(),
        "minecraft:oak_log"
    );
}

#[test]
fn furnace_selection_expires_when_the_open_window_changes() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(false)[1].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert!(inventory.ledger().furnace_ghost_stack(0).is_some());
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(8),
            window_type: ::protocol::WINDOW_TYPE_SMOKER,
            position: [0, 64, 1],
            runtime_entity_id: -1,
        }));
    assert!(inventory.ledger().selected_furnace_result().is_none());
    assert!(inventory.ledger().furnace_ghost_stack(0).is_none());
}

#[test]
fn furnace_authoritative_items_retire_their_preview() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(false)[1].clone();
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let ledger = inventory.ledger_mut();
    for stack in [
        NetworkItemStack {
            network_id: 5,
            count: 1,
            stack_network_id: 45,
            ..NetworkItemStack::empty()
        },
        NetworkItemStack::empty(),
    ] {
        ledger.apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(7),
                slot: 0,
            },
            stack,
            storage_item: None,
        }));
    }
    assert!(ledger.storage_stack(0).is_none());
    assert!(
        ledger.furnace_ghost_stack(0).is_none(),
        "removing an actual stack must not resurrect its retired preview"
    );
    assert!(
        ledger.furnace_ghost_stack(2).is_some(),
        "the other role keeps its preview"
    );
}

#[test]
fn furnace_replacement_returns_untracked_input_to_combined_inventory() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let recipe = inventory.furnace_recipes(false)[1].clone();
    let ledger = inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
        identity: ::protocol::SlotIdentity {
            container: ContainerIdentity::window(7),
            slot: 0,
        },
        stack: NetworkItemStack {
            network_id: 1,
            count: 4,
            stack_network_id: 45,
            ..NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    ledger.begin_furnace_recipe(&recipe).unwrap();
    assert!(ledger.storage_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(0).unwrap().network_id, 1);
    assert!(ledger.displayed_stack(9).is_none());
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

#[test]
fn furnace_content_and_slots_follow_the_open_window_instead_of_packet_names() {
    for window_type in [
        ::protocol::WINDOW_TYPE_FURNACE,
        ::protocol::WINDOW_TYPE_BLAST_FURNACE,
        ::protocol::WINDOW_TYPE_SMOKER,
    ] {
        for name in [
            None,
            Some(0),
            Some(::protocol::CONTAINER_NAME_INVENTORY),
            Some(::protocol::CONTAINER_NAME_DYNAMIC),
        ] {
            let mut inventory = fixture(window_type);
            let identity = ContainerIdentity {
                window_id: Some(7),
                slot_type: name,
                dynamic_id: Some(9),
            };
            let stack = |id, count| NetworkItemStack {
                network_id: id,
                count,
                stack_network_id: id + 100,
                ..NetworkItemStack::empty()
            };
            inventory
                .ledger_mut()
                .apply(&InventoryEvent::Content(InventoryContentEvent {
                    container: identity,
                    slots: vec![stack(2, 8), stack(1, 5), stack(3, 1)].into(),
                    storage_item: NetworkItemStack::empty(),
                }));
            assert_eq!(
                inventory
                    .ledger()
                    .storage_stack(0)
                    .map(|stack| stack.network_id),
                Some(2)
            );
            assert_eq!(
                inventory
                    .ledger()
                    .storage_stack(1)
                    .map(|stack| stack.network_id),
                Some(1)
            );
            assert_eq!(
                inventory
                    .ledger()
                    .storage_stack(2)
                    .map(|stack| stack.network_id),
                Some(3)
            );
            for slot in 0..3 {
                inventory.ledger_mut().apply(&InventoryEvent::Slot(
                    ::protocol::InventorySlotEvent {
                        identity: ::protocol::SlotIdentity {
                            container: identity,
                            slot,
                        },
                        stack: stack(4, 7),
                        storage_item: None,
                    },
                ));
                assert_eq!(
                    inventory
                        .ledger()
                        .storage_stack(slot as u8)
                        .unwrap()
                        .network_id,
                    4
                );
            }
            let original = inventory.ledger().storage_stack(0).unwrap().clone();
            for (container, slots) in [
                (
                    ContainerIdentity {
                        window_id: Some(8),
                        ..identity
                    },
                    vec![stack(2, 8); 3],
                ),
                (identity, vec![stack(2, 8); 2]),
            ] {
                inventory
                    .ledger_mut()
                    .apply(&InventoryEvent::Content(InventoryContentEvent {
                        container,
                        slots: slots.into(),
                        storage_item: NetworkItemStack::empty(),
                    }));
                assert_eq!(inventory.ledger().storage_stack(0), Some(&original));
            }
            inventory
                .ledger_mut()
                .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
                    identity: ::protocol::SlotIdentity {
                        container: identity,
                        slot: 3,
                    },
                    stack: stack(2, 8),
                    storage_item: None,
                }));
            assert!(inventory.ledger().storage_stack(3).is_none());
            assert_eq!(inventory.ledger().displayed_stack(5).unwrap().network_id, 2);
        }
    }
}

#[test]
fn furnace_alternatives_rank_matching_source_counts_and_keep_compatible_topups() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    for (window, slot, id, count) in [(7, 0, 2, 8), (0, 0, 1, 8)] {
        inventory
            .ledger_mut()
            .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
                identity: ::protocol::SlotIdentity {
                    container: ContainerIdentity::window(window),
                    slot,
                },
                stack: NetworkItemStack {
                    network_id: id,
                    count,
                    stack_network_id: 100 + id,
                    ..NetworkItemStack::empty()
                },
                storage_item: None,
            }));
    }
    let recipe = inventory.furnace_recipes(true)[0].clone();
    assert_eq!(&*recipe.ingredients[0].name, "minecraft:birch_log");
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 23);
}

#[test]
fn furnace_recipe_selection_moves_matching_fuel_into_the_ingredient_role() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)]
                .into(),
            storage_item: NetworkItemStack::empty(),
        }));
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ContainerIdentity::window(7),
                slot: 1,
            },
            stack: NetworkItemStack {
                network_id: 2,
                count: 8,
                stack_network_id: 102,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    let recipe = inventory.furnace_recipes(false)[0].clone();
    assert!(inventory.ledger().can_supply_furnace_recipe(&recipe));
    inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 8);
    assert!(inventory.ledger().storage_stack(1).is_none());
}

#[test]
fn furnace_quick_move_keeps_admitted_recipe_inputs_out_of_the_fuel_role() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let ledger = inventory.ledger_mut();
    let entries = ledger
        .item_registry_snapshot()
        .unwrap()
        .values()
        .cloned()
        .map(|mut entry| {
            if entry.network_id == 2 {
                entry.component_based = true;
                entry.version = ::protocol::ItemRegistryVersion::DataDriven;
                entry.canonical_empty_component_data = false;
                entry.component_digest = [40; 32];
                entry.components = Some(Arc::new(::protocol::ItemComponents {
                    fuel_duration_ticks: Some(40),
                    ..Default::default()
                }));
            }
            entry
        })
        .collect();
    ledger.apply_registry(&::protocol::ItemRegistryEvent { entries });
    inventory
        .ledger_mut()
        .begin_quick_move(InventoryTarget::Player(5))
        .unwrap();
    assert_eq!(
        inventory
            .ledger()
            .storage_stack(0)
            .map(|stack| (stack.network_id, stack.count)),
        Some((2, 8))
    );
    assert!(inventory.ledger().storage_stack(1).is_none());
}

/// Resends the registry with `network_id` as a component item burning for `ticks`.
fn resend_with_fuel(inventory: &mut InventorySession, network_id: i32, ticks: u32) {
    let entries = inventory
        .ledger()
        .item_registry_snapshot()
        .unwrap()
        .values()
        .cloned()
        .map(|mut entry| {
            if entry.network_id == network_id {
                entry.component_based = true;
                entry.version = ::protocol::ItemRegistryVersion::DataDriven;
                entry.canonical_empty_component_data = false;
                entry.component_digest = [ticks as u8; 32];
                entry.components = Some(Arc::new(::protocol::ItemComponents {
                    fuel_duration_ticks: Some(ticks),
                    ..Default::default()
                }));
            }
            entry
        })
        .collect();
    inventory
        .ledger_mut()
        .apply_registry(&::protocol::ItemRegistryEvent { entries });
}

#[test]
fn fuel_changes_retire_only_quick_moves_they_reroute() {
    let mut inventory = fuel_and_input_fixture();
    inventory
        .ledger_mut()
        .begin_quick_move(InventoryTarget::Player(5))
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 8);
    resend_with_fuel(&mut inventory, 2, 40);
    assert_eq!(inventory.ledger().pending_request_count(), 1);

    inventory
        .ledger_mut()
        .begin_quick_move(InventoryTarget::Player(7))
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(1).unwrap().count, 3);
    assert_eq!(inventory.ledger().pending_request_count(), 2);
    resend_with_fuel(&mut inventory, 4, 80);
    assert_eq!(inventory.ledger().pending_request_count(), 2);
    resend_with_fuel(&mut inventory, 4, 0);
    assert_eq!(inventory.ledger().pending_request_count(), 1);
    assert!(inventory.ledger().storage_stack(1).is_none());
}

/// Opens the fixture furnace with player slot 5 holding birch logs and slot 7 iron ingot fuel.
fn fuel_and_input_fixture() -> InventorySession {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    resend_with_fuel(&mut inventory, 4, 40);
    let mut stacks =
        vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)];
    stacks[5] = NetworkItemStack {
        network_id: 2,
        count: 8,
        stack_network_id: 25,
        ..NetworkItemStack::empty()
    };
    stacks[7] = NetworkItemStack {
        network_id: 4,
        count: 3,
        stack_network_id: 27,
        ..NetworkItemStack::empty()
    };
    inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: stacks.into(),
            storage_item: NetworkItemStack::empty(),
        }));
    inventory
}

#[test]
fn unrelated_recipe_changes_keep_pending_furnace_inputs() {
    let mut inventory = fuel_and_input_fixture();
    inventory
        .ledger_mut()
        .begin_quick_move(InventoryTarget::Player(5))
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 8);
    apply_recipes(&mut inventory, &FIXTURE_RECIPES[..3], 2);
    let _ = inventory.ledger_mut();
    assert_eq!(inventory.ledger().pending_request_count(), 1);
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 8);
}

#[test]
fn recipe_admission_reroutes_a_pending_fuel_move() {
    let mut inventory = fuel_and_input_fixture();
    inventory
        .ledger_mut()
        .begin_quick_move(InventoryTarget::Player(7))
        .unwrap();
    assert_eq!(inventory.ledger().storage_stack(1).unwrap().count, 3);
    let mut recipes = FIXTURE_RECIPES.to_vec();
    recipes.push(("minecraft:iron_ingot", 5, "furnace"));
    apply_recipes(&mut inventory, &recipes, 2);
    let _ = inventory.ledger_mut();
    assert_eq!(inventory.ledger().pending_request_count(), 0);
    assert!(inventory.ledger().storage_stack(1).is_none());
}

#[test]
fn furnace_recipe_replacement_retires_unsent_input_before_transport() {
    for poll_timeouts in [false, true] {
        let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
        inventory
            .ledger_mut()
            .begin_quick_move(InventoryTarget::Player(5))
            .unwrap();
        assert_eq!(inventory.ledger().pending_request_count(), 1);
        assert!(inventory.ledger().displayed_stack(5).is_none());
        assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 8);

        inventory
            .enqueue_inventory_event(
                1,
                2,
                InventoryEvent::Recipes(CraftingDataFixture::clearing(Vec::new()).update()),
            )
            .unwrap();
        inventory.synchronize_crafting_frontier(1, Some((1, 0, Some(2))));
        inventory.drain_pending_inventory();
        assert!(inventory.furnace_recipes(false).is_empty());

        let mut sent = Vec::new();
        let send = |packet| {
            sent.push(packet);
            Ok::<_, ()>(())
        };
        let admitted = if poll_timeouts {
            inventory.flush_inventory_send(1, send)
        } else {
            inventory.flush_pending_inventory_send(1, send)
        };
        assert_eq!(admitted, Ok(false));
        assert!(sent.is_empty());
        assert_eq!(inventory.ledger().pending_request_count(), 0);
        assert_eq!(inventory.ledger().displayed_stack(5).unwrap().count, 8);
        assert!(inventory.ledger().storage_stack(0).is_none());
    }
}

#[test]
fn furnace_input_admission_tracks_station_metadata_registry_and_catalog_removal() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::Furnace, 2, 0),
        Some(true)
    );
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::Furnace, 2, 1),
        Some(false)
    );
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::Smoker, 2, 0),
        Some(true)
    );
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::BlastFurnace, 2, 0),
        Some(false)
    );
    let mut entries: Vec<_> = inventory
        .ledger()
        .item_registry_snapshot()
        .unwrap()
        .values()
        .cloned()
        .collect();
    entries
        .iter_mut()
        .find(|entry| entry.network_id == 2)
        .unwrap()
        .identifier = Arc::from("custom:unrelated_icon");
    inventory
        .ledger_mut()
        .apply_registry(&::protocol::ItemRegistryEvent {
            entries: entries.into(),
        });
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::Furnace, 2, 0),
        Some(false)
    );
    let ledger = inventory.ledger_mut();
    ledger.bind_furnace_inputs(None);
    assert_eq!(
        ledger.furnace_input_accepts(::protocol::WindowKind::Furnace, 2, 0),
        None
    );
}

#[test]
fn idle_flushes_keep_a_shared_ledger_snapshot() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let _ = inventory.ledger_mut();
    let snapshot = inventory.ledger_snapshot();
    let admitted = inventory.flush_pending_inventory_send(1, |_| Ok::<_, ()>(()));
    assert_eq!(admitted, Ok(false));
    assert!(Arc::ptr_eq(&snapshot, &inventory.ledger_snapshot()));
}

#[test]
fn recipe_changes_keep_partial_takes_from_the_ingredient_slot() {
    let mut inventory = fixture(::protocol::WINDOW_TYPE_FURNACE);
    let ledger = inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(::protocol::CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([NetworkItemStack::empty()]),
        storage_item: NetworkItemStack::empty(),
    }));
    let mut slots = vec![NetworkItemStack::empty(); 3];
    slots[0] = NetworkItemStack {
        network_id: 2,
        count: 8,
        stack_network_id: 30,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(7),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.begin_storage_take_count(0, 1).unwrap();
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 7);
    apply_recipes(&mut inventory, &FIXTURE_RECIPES[2..3], 2);
    let _ = inventory.ledger_mut();
    assert_eq!(
        inventory
            .ledger_mut()
            .furnace_input_accepts(::protocol::WindowKind::Furnace, 2, 0),
        Some(false)
    );
    assert_eq!(inventory.ledger().pending_request_count(), 1);
    assert_eq!(inventory.ledger().storage_stack(0).unwrap().count, 7);
}
