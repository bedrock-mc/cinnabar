use std::sync::Arc;

use bedrock_protocol_1_26_51::{
    enums::ContainerEnumName,
    packets,
    types::{InventoryTransactionPacketData, NetworkItemStackDescriptorSerializedData},
    wire,
};
use protocol::{
    ActorUseAction, ActorUseRequest, BlockItemInteraction, BlockUseRequest, HeldItemRequest,
    ItemUseHand, ItemUseTrigger, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, PredictedSlotChange,
    VerifiedNetworkItemStack,
};
use sha2::{Digest, Sha256};

/// Builds an original stack with distinct contents and an independently retained identity.
fn stack(id: i32) -> VerifiedNetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([0x0a, 0x00, 0x07, 0xa5, 0x00, 0xff]);
    let digest = Sha256::digest(&extra_data).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: 321,
            metadata: 1234,
            stack_network_id: id,
            count: 37,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(0x8765_4321_u32.to_ne_bytes()),
            extra_data,
        },
        digest,
    )
    .unwrap()
}

/// Supplies named, legacy, untracked and empty stacks to each selected-item producer.
fn stacks() -> [VerifiedNetworkItemStack; 4] {
    let empty = NetworkItemStack::empty();
    [
        stack(57),
        stack(-6),
        stack(-1),
        VerifiedNetworkItemStack::try_new(empty.clone(), empty.nbt_digest).unwrap(),
    ]
}

/// Supplies a valid block interaction without deriving expected item contents from a packet.
fn block_request(selected_item: VerifiedNetworkItemStack) -> BlockUseRequest {
    BlockUseRequest {
        hand: ItemUseHand::Mainhand,
        block_position: [12, 64, -9],
        face: 3,
        selected_slot: 2,
        selected_item,
        player_position: [12.25, 65.5, -8.75],
        relative_hit: [0.125, 0.5, 0.875],
        block_runtime_id: 99,
    }
}

/// Supplies a valid main-hand air use or release.
fn held_request(selected_item: VerifiedNetworkItemStack) -> HeldItemRequest {
    HeldItemRequest {
        hand: ItemUseHand::Mainhand,
        selected_slot: 2,
        selected_item,
        player_position: [12.25, 65.5, -8.75],
    }
}

/// Round-trips through the real batch codec and requires exactly one decoded packet.
fn round_trip(packet: protocol::Packet) -> packets::Packet {
    let mut decoded = protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap();
    assert_eq!(decoded.len(), 1);
    *decoded.remove(0).data
}

/// Adjusts old fixture expectations only for the selected descriptor's optional identity.
pub(super) fn without_selected_identity(mut packet: protocol::Packet) -> protocol::Packet {
    let item = match &mut *packet.data {
        packets::Packet::InventoryTransaction(packet) => match &mut packet.transaction {
            InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) => {
                &mut transaction.item
            }
            InventoryTransactionPacketData::ItemUseOnActorInventoryTransaction { item, .. }
            | InventoryTransactionPacketData::ItemReleaseInventoryTransaction { item, .. } => item,
            _ => panic!("expected selected-item fixture"),
        },
        packets::Packet::PlayerAuthInput(input) => {
            &mut input
                .item_use_transaction
                .as_mut()
                .unwrap()
                .item_use_transaction
                .item
        }
        _ => panic!("expected interaction fixture"),
    };
    item.net_id_variant = None;
    packet
}

/// Checks every retained content field independently of the optional stack identity.
fn assert_contents(
    item: &NetworkItemStackDescriptorSerializedData,
    stack: &VerifiedNetworkItemStack,
) {
    assert_eq!(i32::from(item.id.0), stack.network_id());
    assert_eq!(item.stack_size.0, stack.count());
    assert_eq!(item.aux_value.0, stack.metadata());
    assert_eq!(
        item.block_runtime_id.0.to_ne_bytes(),
        stack.block_runtime_id().to_ne_bytes()
    );
    assert_eq!(item.user_data_buffer.as_ref(), stack.extra_data());
}

/// Checks the selected descriptor after decoding either standalone or embedded transactions.
fn assert_selected(packet: protocol::Packet, stack: &VerifiedNetworkItemStack) {
    let item = match round_trip(packet) {
        packets::Packet::InventoryTransaction(packet) => match packet.transaction {
            InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) => {
                transaction.item
            }
            InventoryTransactionPacketData::ItemUseOnActorInventoryTransaction { item, .. }
            | InventoryTransactionPacketData::ItemReleaseInventoryTransaction { item, .. } => item,
            _ => panic!("expected selected-item transaction"),
        },
        packets::Packet::PlayerAuthInput(input) => {
            input
                .item_use_transaction
                .unwrap()
                .item_use_transaction
                .item
        }
        _ => panic!("expected interaction packet"),
    };
    assert_contents(&item, stack);
    assert_eq!(
        item.net_id_variant,
        None,
        "selected descriptor for retained ID {}",
        stack.stack_network_id()
    );
}

#[test]
fn block_selection_omits_identity_and_preserves_contents() {
    for stack in stacks() {
        assert_selected(
            protocol::click_block_packet(block_request(stack.clone())).unwrap(),
            &stack,
        );
        assert_selected(
            protocol::destroy_block_packet(block_request(stack.clone())).unwrap(),
            &stack,
        );
        assert_selected(
            protocol::click_block_transaction_packet(
                block_request(stack.clone()),
                ItemUseTrigger::SimulationTick,
                true,
                None,
            )
            .unwrap(),
            &stack,
        );
    }
}

#[test]
fn held_selection_omits_identity_and_preserves_contents() {
    for stack in stacks() {
        assert_selected(
            protocol::click_air_packet(held_request(stack.clone()), None).unwrap(),
            &stack,
        );
        assert_selected(
            protocol::use_item_as_attack_packet(
                held_request(stack.clone()),
                [0.25, 0.5, -0.75],
                true,
            )
            .unwrap(),
            &stack,
        );
        assert_selected(
            protocol::release_item_packet(held_request(stack.clone())).unwrap(),
            &stack,
        );
    }
}

#[test]
fn actor_selection_omits_identity_and_preserves_contents() {
    for stack in stacks() {
        for action in [ActorUseAction::Attack, ActorUseAction::Interact] {
            let packet = protocol::use_actor_packet(ActorUseRequest {
                actor_runtime_id: 42,
                action,
                selected_slot: 2,
                selected_item: stack.clone(),
                player_position: [12.25, 65.5, -8.75],
                hit_position: [13.0, 65.0, -9.0],
            })
            .unwrap();
            assert_selected(packet, &stack);
        }
    }
}

/// Supplies movement fields so embedded descriptors cross the full PlayerAuthInput codec.
fn movement() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 41,
        position: [12.25, 65.5, -8.75],
        delta: [0.0; 3],
        move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 180.0,
        head_yaw: 180.0,
        camera_orientation: [0.0, 0.0, -1.0],
        flags: PlayerInputFlags::NONE,
        input_mode: PlayerInputMode::Mouse,
    }
}

#[test]
fn embedded_selection_omits_identity_and_preserves_contents() {
    for stack in stacks() {
        for interaction in [
            BlockItemInteraction::Use(block_request(stack.clone())),
            BlockItemInteraction::Destroy(block_request(stack.clone())),
        ] {
            let packet = protocol::player_auth_input_with_interactions(
                movement(),
                &PlayerAuthInputInteractions {
                    block_actions: protocol::BlockActions::new(),
                    block_interaction: Some(interaction),
                },
            )
            .unwrap();
            assert_selected(packet, &stack);
        }
    }
}

/// Action items carry no stack network id; the legacy request and slot list name the change.
fn assert_change(
    packet: protocol::Packet,
    from: &VerifiedNetworkItemStack,
    to: &VerifiedNetworkItemStack,
) {
    let packets::Packet::InventoryTransaction(packet) = round_trip(packet) else {
        panic!("expected inventory transaction");
    };
    assert_eq!(packet.legacy_request_id.0, -6);
    let slots = packet.legacy_set_item_slots.unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(
        slots[0].container_enum,
        ContainerEnumName::Inventorycontainer
    );
    assert_eq!(slots[0].slots.as_ref(), [2]);
    let InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) =
        packet.transaction
    else {
        panic!("expected item use");
    };
    let [action] = &transaction.actions.actions[..] else {
        panic!("expected one inventory action");
    };
    assert_eq!(action.slot.0, 2);
    assert_contents(&action.from_item, from);
    assert_contents(&action.to_item, to);
    assert_eq!(action.from_item.net_id_variant, None);
    assert_eq!(action.to_item.net_id_variant, None);
    assert_contents(&transaction.item, from);
    assert_eq!(transaction.item.net_id_variant, None);
}

#[test]
fn transaction_actions_omit_stack_ids_for_throws_and_placements() {
    for id in [57, -4] {
        let from = stack(id);
        let to = from.less_one(-6);
        let change = PredictedSlotChange {
            legacy_request_id: -6,
            from: from.clone(),
            to: to.clone(),
        };
        assert_change(
            protocol::click_air_packet(held_request(from.clone()), Some(change.clone())).unwrap(),
            &from,
            &to,
        );
        assert_change(
            protocol::click_block_transaction_packet(
                block_request(from.clone()),
                ItemUseTrigger::PlayerInput,
                true,
                Some(change),
            )
            .unwrap(),
            &from,
            &to,
        );
        assert_eq!(from.stack_network_id(), id);
        assert_eq!(to.stack_network_id(), -6);
    }
}
