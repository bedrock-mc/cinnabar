use protocol::{
    BedrockSession, CREATED_OUTPUT_SLOT, InventoryPacketError, MAX_STACK_REQUEST_ACTIONS,
    StackRequestAction, StackRequestContainer, StackRequestSlot, container_close_packet,
    decode_batch, encode, item_stack_request_packet,
};

fn slot(container: StackRequestContainer, slot: u8, stack_network_id: i32) -> StackRequestSlot {
    StackRequestSlot {
        container,
        slot,
        stack_network_id,
    }
}

fn body(action: StackRequestAction) -> Vec<u8> {
    request_body(-3, action)
}

fn request_body(request_id: i32, action: StackRequestAction) -> Vec<u8> {
    encode(
        &item_stack_request_packet(request_id, &[action]).expect("valid request"),
        &BedrockSession { shield_item_id: 0 },
    )
    .expect("encode request")
    .to_vec()
}

#[test]
fn personal_take_and_place_encode_empty_destinations_as_stack_id_zero() {
    assert_eq!(
        request_body(
            -3,
            StackRequestAction::Take {
                amount: 32,
                source: slot(StackRequestContainer::PlayerInventory, 0, 9),
                destination: slot(StackRequestContainer::Cursor, 0, 0),
            },
        ),
        hex("fe1b93010105010000201c0000090000003b00000000000000ffffffff")
    );
    assert_eq!(
        request_body(
            -5,
            StackRequestAction::Place {
                amount: 32,
                source: slot(StackRequestContainer::Cursor, 0, 9),
                destination: slot(StackRequestContainer::PlayerInventory, 9, 0),
            },
        ),
        hex("fe1b93010109010101203b0000090000001d00090000000000ffffffff")
    );
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect()
}

#[test]
fn take_place_and_swap_have_exact_protocol_2168_wire() {
    let player = slot(StackRequestContainer::PlayerInventory, 4, 91);
    let cursor = slot(StackRequestContainer::Cursor, 0, -1);

    assert_eq!(
        body(StackRequestAction::Take {
            amount: 3,
            source: player,
            destination: cursor,
        }),
        hex("fe1b93010105010000031c00045b0000003b0000ffffffff00ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Place {
            amount: 3,
            source: cursor,
            destination: player,
        }),
        hex("fe1b93010105010101033b0000ffffffff1c00045b00000000ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Swap {
            source: player,
            destination: cursor,
        }),
        hex("fe1a930101050102021c00045b0000003b0000ffffffff00ffffffff")
    );
}

#[test]
fn level_entity_take_and_native_client_close_encode_expected_wire() {
    let storage = slot(
        StackRequestContainer::LevelEntity { dynamic_id: None },
        2,
        91,
    );
    let cursor = slot(StackRequestContainer::Cursor, 0, -1);
    assert_eq!(
        body(StackRequestAction::Take {
            amount: 3,
            source: storage,
            destination: cursor,
        }),
        hex("fe1b93010105010000030700025b0000003b0000ffffffff00ffffffff")
    );
    assert_eq!(
        encode(
            &container_close_packet(1).expect("valid close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042f01f700")
    );
    assert_eq!(
        encode(
            &container_close_packet(-1).expect("signed raw close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042ffff700")
    );
    assert_eq!(
        encode(
            &container_close_packet(-128).expect("lowest signed raw close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042f80f700")
    );
}

#[test]
fn builder_rejects_ids_amounts_counts_and_slots() {
    let player = slot(StackRequestContainer::PlayerInventory, 0, 1);
    let cursor = slot(StackRequestContainer::Cursor, 0, -1);
    let swap = StackRequestAction::Swap {
        source: player,
        destination: cursor,
    };
    assert_eq!(
        item_stack_request_packet(0, std::slice::from_ref(&swap)).unwrap_err(),
        InventoryPacketError::InvalidStackRequestId
    );
    assert_eq!(
        item_stack_request_packet(
            -3,
            &[StackRequestAction::Take {
                amount: 0,
                source: player,
                destination: cursor,
            }],
        )
        .unwrap_err(),
        InventoryPacketError::InvalidStackRequestAmount
    );
    assert!(
        item_stack_request_packet(
            -3,
            &[StackRequestAction::Swap {
                source: slot(StackRequestContainer::PlayerInventory, 36, 1),
                destination: cursor,
            }],
        )
        .is_err()
    );
    assert_eq!(
        item_stack_request_packet(-3, &[]).unwrap_err(),
        InventoryPacketError::InvalidStackRequestActionCount(0)
    );
    let too_many = vec![swap.clone(); MAX_STACK_REQUEST_ACTIONS + 1];
    assert_eq!(
        item_stack_request_packet(-3, &too_many).unwrap_err(),
        InventoryPacketError::InvalidStackRequestActionCount(MAX_STACK_REQUEST_ACTIONS + 1)
    );
    assert!(item_stack_request_packet(-3, &too_many[1..]).is_ok());
}

/// Native sparse ownership allows earlier negative odd request references on
/// either cell. This request's own id is reserved for its newly created output.
/// Current SparseContainerSetListenerClient::postSetItem stamps cells;
/// ItemStackRequestActionHandler::_validateRequestSlot resolves them.
#[test]
fn negative_stack_ids_name_prior_predictions_or_this_requests_created_output() {
    let take = |id| StackRequestAction::Take {
        amount: 1,
        source: slot(
            StackRequestContainer::CreatedOutput,
            CREATED_OUTPUT_SLOT,
            id,
        ),
        destination: slot(StackRequestContainer::Cursor, 0, 0),
    };
    assert!(item_stack_request_packet(-7, &[take(-7)]).is_ok());
    assert!(item_stack_request_packet(-7, &[take(-5)]).is_ok());
    let chained = [
        StackRequestAction::Take {
            amount: 1,
            source: slot(StackRequestContainer::PlayerInventory, 0, -5),
            destination: slot(StackRequestContainer::Cursor, 0, -3),
        },
        StackRequestAction::Take {
            amount: 1,
            source: slot(
                StackRequestContainer::CreatedOutput,
                CREATED_OUTPUT_SLOT,
                -7,
            ),
            destination: slot(StackRequestContainer::Cursor, 0, -5),
        },
    ];
    let packet = item_stack_request_packet(-7, &chained).unwrap();
    let session = BedrockSession { shield_item_id: 0 };
    let mut packets = decode_batch(encode(&packet, &session).unwrap(), &session).unwrap();
    use valentine::bedrock::version::v1_26_51::{
        ItemStackRequestPacketDataRequestDataActionsItem as Action, McpePacketData,
    };
    let McpePacketData::ItemStackRequestPacket(decoded) = packets.pop().unwrap().data else {
        panic!()
    };
    let [
        Action::TakeActionData(previous),
        Action::TakeActionData(created),
    ] = decoded.requests[0].actions.as_slice()
    else {
        panic!()
    };
    assert_eq!(
        (
            previous.source.net_id_variant,
            previous.destination.net_id_variant
        ),
        (-5, -3)
    );
    assert_eq!(
        (
            created.source.net_id_variant,
            created.destination.net_id_variant
        ),
        (-7, -5)
    );
    for id in [-6, -8, -9] {
        assert_eq!(
            item_stack_request_packet(-7, &[take(id)]).unwrap_err(),
            InventoryPacketError::InvalidRequestStackNetworkId(id)
        );
    }
    let from_player = StackRequestAction::Take {
        amount: 1,
        source: slot(StackRequestContainer::PlayerInventory, 0, -7),
        destination: slot(StackRequestContainer::Cursor, 0, 0),
    };
    assert_eq!(
        item_stack_request_packet(-7, &[from_player]).unwrap_err(),
        InventoryPacketError::InvalidRequestStackNetworkId(-7)
    );
}

/// Offhand cells always go out as wire slot 1; players 0..9 use the hotbar
/// name and 9..36 the inventory name. The bytes match the owner's captured
/// quick transfer from inventory slot 14 into the offhand.
#[test]
fn fixed_windows_use_vanilla_names_and_wire_slots() {
    let place = |destination| {
        body(StackRequestAction::Place {
            amount: 1,
            source: slot(StackRequestContainer::PlayerInventory, 14, 493),
            destination,
        })
    };
    let offhand_zero = place(slot(StackRequestContainer::Offhand, 0, 0));
    let offhand_one = place(slot(StackRequestContainer::Offhand, 1, 0));
    assert_eq!(offhand_zero, offhand_one);
    assert_eq!(
        offhand_one,
        hex("fe1b93010105010101011d000eed0100002200010000000000ffffffff")
    );
    for (container, slot_index) in [
        (StackRequestContainer::Armor, 5),
        (StackRequestContainer::Offhand, 2),
        (StackRequestContainer::CraftingInput, 27),
        (StackRequestContainer::CraftingInput, 41),
        (StackRequestContainer::CreatedOutput, 0),
        (
            StackRequestContainer::OpenWindow {
                name: 29,
                dynamic_id: None,
            },
            0,
        ),
    ] {
        assert!(
            item_stack_request_packet(
                -3,
                &[StackRequestAction::Destroy {
                    amount: 1,
                    source: slot(container, slot_index, 5),
                }],
            )
            .is_err(),
            "{container:?} slot {slot_index}"
        );
    }
}
