use protocol::{
    BedrockSession, InventoryPacketError, StackRequestAction, StackRequestContainer,
    StackRequestSlot, encode, item_stack_request_packet,
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
        &item_stack_request_packet(7, action).expect("valid request"),
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
        hex("fe1b9301010e010000030c00045b0000003b0000ffffffff00ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Place {
            amount: 3,
            source: cursor,
            destination: player,
        }),
        hex("fe1b9301010e010101033b0000ffffffff0c00045b00000000ffffffff")
    );
    assert_eq!(
        body(StackRequestAction::Swap {
            source: player,
            destination: cursor,
        }),
        hex("fe1a9301010e0102020c00045b0000003b0000ffffffff00ffffffff")
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
            1,
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
            1,
            StackRequestAction::Swap {
                source: slot(StackRequestContainer::PlayerInventory, 36, 1),
                destination: cursor,
            },
        )
        .is_err()
    );
}
