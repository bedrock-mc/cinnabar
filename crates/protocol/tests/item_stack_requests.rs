use protocol::{
    BedrockSession, InventoryPacketError, StackRequestAction, StackRequestContainer,
    StackRequestSlot, container_close_packet, encode, item_stack_request_packet,
};

fn slot(container: StackRequestContainer, slot: u8, stack_network_id: i32) -> StackRequestSlot {
    StackRequestSlot {
        container,
        slot,
        stack_network_id,
    }
}

fn body(action: StackRequestAction) -> Vec<u8> {
    encode(
        &item_stack_request_packet(-3, action).expect("valid request"),
        &BedrockSession { shield_item_id: 0 },
    )
    .expect("encode request")
    .to_vec()
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
        hex("fe1b93010105010000030c00045b0000003b0000ffffffff00ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Place {
            amount: 3,
            source: cursor,
            destination: player,
        }),
        hex("fe1b93010105010101033b0000ffffffff0c00045b00000000ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Swap {
            source: player,
            destination: cursor,
        }),
        hex("fe1a930101050102020c00045b0000003b0000ffffffff00ffffffff")
    );
}

#[test]
fn level_entity_take_and_client_close_match_captured_protocol_2168_wire() {
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
            &container_close_packet(1, 0).expect("valid close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042f010000")
    );
    assert_eq!(
        encode(
            &container_close_packet(-1, 0).expect("signed raw close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042fff0000")
    );
    assert_eq!(
        encode(
            &container_close_packet(-128, 0).expect("lowest signed raw close"),
            &BedrockSession { shield_item_id: 0 },
        )
        .unwrap()
        .to_vec(),
        hex("fe042f800000")
    );
}

#[test]
fn builder_rejects_ids_amounts_and_slots_outside_the_tranche() {
    let player = slot(StackRequestContainer::PlayerInventory, 0, 1);
    let cursor = slot(StackRequestContainer::Cursor, 0, -1);
    assert_eq!(
        item_stack_request_packet(
            0,
            StackRequestAction::Swap {
                source: player,
                destination: cursor,
            },
        )
        .unwrap_err(),
        InventoryPacketError::InvalidStackRequestId
    );
    assert_eq!(
        item_stack_request_packet(
            -3,
            StackRequestAction::Take {
                amount: 0,
                source: player,
                destination: cursor,
            },
        )
        .unwrap_err(),
        InventoryPacketError::InvalidStackRequestAmount
    );
    assert!(
        item_stack_request_packet(
            -3,
            StackRequestAction::Swap {
                source: slot(StackRequestContainer::PlayerInventory, 36, 1),
                destination: cursor,
            },
        )
        .is_err()
    );
}
