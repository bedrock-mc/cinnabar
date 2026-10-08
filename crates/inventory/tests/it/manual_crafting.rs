use ::protocol::wire::valentine::bedrock::version::v1_26_51::McpePacketData;
use ::protocol::*;
use bytes::Bytes;
use inventory::{ManualCraftError, ManualCraftInput, ManualCraftSnapshot, manual_craft_packet};
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn admitted(fixture: &'static [u8]) -> RecipeUpdate {
    let mut batch = Bytes::from_static(fixture);
    let raw = ::protocol::wire::jolyne::batch::decode_batch_raw(&mut batch, false, Some(4096))
        .unwrap()
        .remove(0);
    decode_recipe_update(raw.body()).unwrap()
}
fn registry() -> Vec<ItemRegistryEntry> {
    [(6, "minecraft:oak_log"), (7, "minecraft:oak_planks")]
        .into_iter()
        .map(|(network_id, name)| ItemRegistryEntry {
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
    let McpePacketData::ItemStackRequestPacket(packet) = packet.data else {
        panic!("request")
    };
    use ::protocol::wire::valentine::bedrock::version::v1_26_51::{
        ItemStackRequestCerealNetworkItemInstanceDescriptorDataItemDescriptor as Descriptor,
        ItemStackRequestPacketDataRequestDataActionsItem as Action,
    };
    let Action::CraftResultsActionData(results) = &packet.requests[0].actions[1] else {
        panic!("results")
    };
    let Descriptor::ItemNameDescriptorData(name) = &results.craft_results[0].item_descriptor else {
        panic!("name")
    };
    assert_eq!(name.full_name, "minecraft:birch_planks");
    let Action::ConsumeActionData(consume) = &packet.requests[0].actions[2] else {
        panic!("consume")
    };
    assert_eq!(consume.source.net_id_variant, 202);
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
    use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};
    let mut bytes = bytes::BytesMut::new();
    CraftingDataPacket {
        shaped_recipes: vec![ShapedRecipePayload {
            recipe_id: "test:two".into(),
            width: 1,
            height: 1,
            ingredients: vec![CerealizerRecipeIngredientSerializedData {
                descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                    key: "name".into(),
                    value: "minecraft:oak_log".into(),
                }],
                aux_value: 0,
                stack_size: 2,
            }],
            results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
                id: 7,
                stacksize: 4,
                auxvalue: 0,
                block_runtime_id: 0,
                user_data_buffer: vec![0; 10],
            }],
            tag: "crafting_table".into(),
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 17 },
            ..Default::default()
        }],
        clear_recipes: true,
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &decode_recipe_update(&bytes).unwrap());
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
    let mut packet = request(
        &catalog,
        17,
        [Some(input(28, 101)), None, None, None],
        &registry(),
        -3,
        &cursor(),
    )
    .unwrap();
    packet.header.from_subclient = 1;
    packet.header.to_subclient = 2;
    assert_eq!(
        encode(&packet, &BedrockSession { shield_item_id: 0 })
            .unwrap()
            .as_ref(),
        include_bytes!("../../../protocol/fixtures/item_stack_request_manual_craft.bin")
    );
    let McpePacketData::ItemStackRequestPacket(request) = packet.data else {
        panic!("request");
    };
    let actions = &request.requests[0].actions;
    assert_eq!(actions.len(), 4);
    use ::protocol::wire::valentine::bedrock::version::v1_26_51::ItemStackRequestPacketDataRequestDataActionsItem as Action;
    assert!(matches!(actions[0], Action::CraftRecipeActionData(_)));
    assert!(matches!(actions[1], Action::CraftResultsActionData(_)));
    assert!(matches!(actions[2], Action::ConsumeActionData(_)));
    let Action::TakeActionData(take) = &actions[3] else {
        panic!("take");
    };
    assert_eq!((take.source.slot, take.source.net_id_variant), (50, -3));
    assert_eq!(
        (take.destination.slot, take.destination.net_id_variant),
        (0, 0)
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
    let McpePacketData::ItemStackRequestPacket(packet) = packet.data else {
        panic!("request");
    };
    assert_eq!(packet.requests[0].actions.len(), 5);
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
    let packets = decode_batch(
        Bytes::from_static(include_bytes!(
            "../../../protocol/fixtures/item_stack_response_manual_craft.bin"
        )),
        &BedrockSession { shield_item_id: 0 },
    )
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
