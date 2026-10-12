use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, StackActionObservation,
    StackSlotObservation, recipe_update_from_batch, stack_request_actions, with_subclients,
};
use ::protocol::*;
use bytes::Bytes;
use inventory::{ManualCraftError, ManualCraftInput, ManualCraftSnapshot, manual_craft_packet};
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn admitted(fixture: &'static [u8]) -> RecipeUpdate {
    recipe_update_from_batch(fixture)
}
fn registry() -> Vec<ItemRegistryEntry> {
    [(6, "minecraft:oak_log"), (7, "minecraft:oak_planks")]
        .into_iter()
        .map(|(network_id, name)| ItemRegistryEntry {
            components: None,
            identifier: Arc::from(name),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        })
        .collect()
}
fn input(slot: u8, stack_id: i32) -> ManualCraftInput {
    counted_input(slot, stack_id, 1)
}
fn counted_input(slot: u8, stack_id: i32, count: u16) -> ManualCraftInput {
    let digest: [u8; 32] = Sha256::digest([]).into();
    let stack = NetworkItemStack {
        network_id: 6,
        metadata: 0,
        count,
        stack_network_id: stack_id,
        nbt_digest: digest,
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    };
    ManualCraftInput {
        slot,
        stack: VerifiedNetworkItemStack::try_new(stack, digest).unwrap(),
    }
}

#[test]
fn each_atomic_request_uses_current_registry_and_ingredient_authority() {
    let catalog = catalog();
    let mut entries = registry();
    let inputs = || [Some(input(28, 101)), None, None, None];
    assert!(request(&catalog, 17, inputs(), &entries, -3, &cursor()).is_ok());
    entries[1].identifier = Arc::from("minecraft:birch_planks");
    let packet = request(
        &catalog,
        17,
        [Some(input(28, 202)), None, None, None],
        &entries,
        -5,
        &cursor(),
    )
    .unwrap();
    let actions = stack_request_actions(&packet).expect("request");
    let StackActionObservation::CraftResults(results) = &actions[1] else {
        panic!("results")
    };
    assert_eq!(results[0].as_deref(), Some("minecraft:birch_planks"));
    let StackActionObservation::Consume { source, .. } = &actions[2] else {
        panic!("consume")
    };
    assert_eq!(source.stack_network_id, 202);
    entries[1].negotiated_max_stack_size = Some(3);
    assert!(request(&catalog, 17, inputs(), &entries, -7, &cursor()).is_err());
    entries[1].negotiated_max_stack_size = None;
    assert!(request(&catalog, 17, inputs(), &entries, -7, &cursor()).is_err());
    entries = registry();
    entries[0].identifier = Arc::from("minecraft:birch_log");
    assert!(request(&catalog, 17, inputs(), &entries, -7, &cursor()).is_err());
    assert!(
        request(
            &catalog,
            17,
            [None, None, None, None],
            &registry(),
            -7,
            &cursor()
        )
        .is_err()
    );
}

#[test]
fn current_ingredient_count_must_cover_the_advertised_consumption() {
    let ingredient = IngredientFixture {
        count: 2,
        ..IngredientFixture::named("minecraft:oak_log", 0)
    };
    let output = OutputFixture {
        user_data: vec![0; 10],
        ..OutputFixture::new(7, 4)
    };
    let update = CraftingDataFixture::clearing(vec![RecipeFixture {
        net_id: 17,
        ..RecipeFixture::shaped("test:two", 1, 1, vec![ingredient], vec![output])
    }])
    .update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update);
    assert!(
        request(
            &catalog,
            17,
            [Some(counted_input(28, 101, 2)), None, None, None],
            &registry(),
            -3,
            &cursor()
        )
        .is_ok()
    );
    assert!(
        request(
            &catalog,
            17,
            [Some(counted_input(28, 101, 1)), None, None, None],
            &registry(),
            -5,
            &cursor()
        )
        .is_err()
    );
}
fn cursor() -> VerifiedNetworkItemStack {
    let stack = NetworkItemStack::empty();
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest).unwrap()
}
fn catalog() -> RecipeCatalog {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(
        1,
        1,
        &admitted(include_bytes!(
            "../../../protocol/fixtures/crafting_data_manual_named_1x1.bin"
        )),
    );
    catalog
}

fn request(
    catalog: &RecipeCatalog,
    recipe_id: u32,
    inputs: [Option<ManualCraftInput>; 4],
    registry: &[ItemRegistryEntry],
    request_id: i32,
    cursor: &VerifiedNetworkItemStack,
) -> Result<Packet, ManualCraftError> {
    manual_craft_packet(
        ManualCraftSnapshot {
            session: 1,
            catalog,
            registry,
            inputs,
            cursor,
        },
        recipe_id,
        request_id,
    )
}

#[test]
fn candidate_compound_order_and_current_request_reference_match_pinned_codec() {
    let catalog = catalog();
    let packet = request(
        &catalog,
        17,
        [Some(input(28, 101)), None, None, None],
        &registry(),
        -3,
        &cursor(),
    )
    .unwrap();
    let packet = with_subclients(packet, 1, 2);
    assert_eq!(
        encode(&packet).unwrap().as_ref(),
        include_bytes!("../../../protocol/fixtures/item_stack_request_manual_craft.bin")
    );
    let actions = stack_request_actions(&packet).expect("request");
    assert_eq!(actions.len(), 4);
    assert!(matches!(
        actions[0],
        StackActionObservation::CraftRecipe { .. }
    ));
    assert!(matches!(
        actions[1],
        StackActionObservation::CraftResults(_)
    ));
    assert!(matches!(actions[2], StackActionObservation::Consume { .. }));
    let StackActionObservation::Take {
        source,
        destination,
        ..
    } = actions[3]
    else {
        panic!("take");
    };
    assert_eq!(
        source,
        StackSlotObservation {
            slot: 50,
            stack_network_id: -3
        }
    );
    assert_eq!(
        destination,
        StackSlotObservation {
            slot: 0,
            stack_network_id: 0
        }
    );
}

#[test]
fn vertical_named_shape_uses_personal_grid_stride_and_positive_input_ids() {
    let mut catalog = catalog();
    catalog.apply(
        1,
        2,
        &admitted(include_bytes!(
            "../../../protocol/fixtures/crafting_data_manual_named_1x2.bin"
        )),
    );
    let prepare = |a, b| {
        request(
            &catalog,
            18,
            [Some(input(28, a)), None, Some(input(30, b)), None],
            &registry(),
            -3,
            &cursor(),
        )
    };
    let packet = prepare(101, 102).unwrap();
    let actions = stack_request_actions(&packet).expect("request");
    assert_eq!(actions.len(), 5);
    assert!(prepare(-1, 102).is_err());
    assert!(prepare(101, 101).is_err());
    assert!(
        request(
            &catalog,
            18,
            [Some(input(28, 101)), Some(input(29, 102)), None, None],
            &registry(),
            -3,
            &cursor()
        )
        .is_err()
    );
}

#[test]
fn replaced_or_retired_recipe_cannot_emit_and_generic_negative_slots_stay_invalid() {
    let mut catalog = catalog();
    let inputs = || [Some(input(28, 101)), None, None, None];
    assert!(request(&catalog, 17, inputs(), &registry(), -2, &cursor()).is_err());
    catalog.apply(
        1,
        2,
        &admitted(include_bytes!(
            "../../../protocol/fixtures/crafting_data_manual_unsupported_replacement.bin"
        )),
    );
    // The catalog retains the three-wide replacement for the table grid, but
    // the personal two-by-two builder refuses it.
    assert_eq!(catalog.recipe(17).unwrap().dimensions(), (3, 1));
    assert_eq!(
        request(&catalog, 17, inputs(), &registry(), -3, &cursor()).unwrap_err(),
        ManualCraftError::Unsupported
    );
    catalog.apply(
        1,
        3,
        &admitted(include_bytes!(
            "../../../protocol/fixtures/crafting_data_manual_clear_empty.bin"
        )),
    );
    catalog.begin_session(2);
    assert!(request(&catalog, 17, inputs(), &registry(), -3, &cursor()).is_err());
    assert!(
        item_stack_request_packet(
            -3,
            &[StackRequestAction::Take {
                amount: 1,
                source: StackRequestSlot {
                    container: StackRequestContainer::PlayerInventory,
                    slot: 0,
                    stack_network_id: -3
                },
                destination: StackRequestSlot {
                    container: StackRequestContainer::Cursor,
                    slot: 0,
                    stack_network_id: 0
                },
            }]
        )
        .is_err()
    );
}

#[test]
fn synthetic_acceptance_response_retains_exact_request_and_result_mapping() {
    let packets = decode_batch(Bytes::from_static(include_bytes!(
        "../../../protocol/fixtures/item_stack_response_manual_craft.bin"
    )))
    .unwrap();
    let packet = packets.into_iter().next().unwrap();
    let Some(WorldEvent::Inventory(InventoryEvent::Response(response))) =
        into_world_event(packet, 0).unwrap()
    else {
        panic!("response");
    };
    assert_eq!(response.responses.len(), 2);
    let accepted = &response.responses[0];
    assert_eq!(
        (accepted.request_id, accepted.status),
        (-3, StackResponseStatus::Accepted)
    );
    assert_eq!(accepted.containers[0].slots[0].count, 0);
    assert_eq!(
        (
            accepted.containers[1].slots[0].count,
            accepted.containers[1].slots[0].item_stack_id
        ),
        (4, 201)
    );
    assert_eq!(
        (
            response.responses[1].request_id,
            response.responses[1].status
        ),
        (-5, StackResponseStatus::Rejected)
    );
}

#[test]
fn cursor_and_output_binding_must_be_proven_not_guessed() {
    let catalog = catalog();
    let inputs = || [Some(input(28, 101)), None, None, None];
    assert!(
        request(
            &catalog,
            17,
            inputs(),
            &registry(),
            -3,
            &input(28, 101).stack
        )
        .is_err()
    );
    let mut entries = registry();
    entries[1].negotiated_max_stack_size = None;
    assert!(request(&catalog, 17, inputs(), &entries, -3, &cursor()).is_err());
    entries[1].negotiated_max_stack_size = Some(3);
    assert!(request(&catalog, 17, inputs(), &entries, -3, &cursor()).is_err());
    entries[1].negotiated_max_stack_size = Some(64);
    entries[1].component_based = true;
    entries[1].canonical_empty_component_data = false;
    assert!(request(&catalog, 17, inputs(), &entries, -3, &cursor()).is_err());
    let duplicate = registry()[1].clone();
    entries = registry();
    entries.push(duplicate);
    assert!(request(&catalog, 17, inputs(), &entries, -3, &cursor()).is_err());
}
