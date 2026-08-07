use protocol::{PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, player_auth_input};
use valentine::bedrock::version::v1_26_40::{
    McpePacketData, McpePacketName, PlayerAuthInputPacketInputDataItem,
    PlayerAuthInputPacketInputMode, PlayerAuthInputPacketNewInteractionModel,
    PlayerAuthInputPacketPlayMode,
};

fn snapshot() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 1_234,
        position: [1.25, 64.0, -2.5],
        delta: [0.25, 0.0, -0.5],
        move_vector: [-1.0, 1.0],
        analogue_move_vector: [-0.75, 0.75],
        raw_move_vector: [-1.0, 1.0],
        pitch: 10.5,
        yaw: 20.25,
        head_yaw: 30.75,
        camera_orientation: [0.25, -0.5, -0.75],
        flags: PlayerInputFlags::UP
            | PlayerInputFlags::LEFT
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::SPRINTING,
        input_mode: PlayerInputMode::Mouse,
    }
}

#[test]
fn vendor_neutral_snapshot_maps_to_protocol_2168_player_auth_input() {
    let packet = player_auth_input(snapshot()).expect("valid player input");
    assert_eq!(packet.header.id, McpePacketName::PlayerAuthInputPacket);
    assert_eq!(
        (packet.header.from_subclient, packet.header.to_subclient),
        (0, 0)
    );

    let McpePacketData::PlayerAuthInputPacket(input) = packet.data else {
        panic!("expected PlayerAuthInput payload");
    };
    assert_eq!(input.client_tick.inputtick, 1_234);
    assert_eq!(
        (input.position.x, input.position.y, input.position.z),
        (1.25, 64.0, -2.5)
    );
    assert_eq!(
        (input.pos_delta.x, input.pos_delta.y, input.pos_delta.z),
        (0.25, 0.0, -0.5)
    );
    assert_eq!((input.move_vector.x, input.move_vector.y), (-1.0, 1.0));
    assert_eq!(
        (input.analog_move_vector.x, input.analog_move_vector.y),
        (-0.75, 0.75)
    );
    assert_eq!(
        (input.raw_move_vector.x, input.raw_move_vector.y),
        (-1.0, 1.0)
    );
    assert_eq!(
        (
            input.player_rotation.x,
            input.player_rotation.y,
            input.player_head_rotation
        ),
        (10.5, 20.25, 30.75)
    );
    assert_eq!(
        (
            input.camera_orientation.x,
            input.camera_orientation.y,
            input.camera_orientation.z
        ),
        (0.25, -0.5, -0.75)
    );
    assert_eq!(input.interact_rotation.x, input.player_rotation.x);
    assert_eq!(input.interact_rotation.y, input.player_rotation.y);
    assert_eq!(input.input_mode, PlayerAuthInputPacketInputMode::Mouse);
    assert_eq!(input.play_mode, PlayerAuthInputPacketPlayMode::Normal);
    // The protocol-1001 Unknown(-1) workaround is gone: gophertunnel writes
    // this with io.Varint32 (zigzag), which the generated enum now matches.
    assert_eq!(
        input.new_interaction_model,
        PlayerAuthInputPacketNewInteractionModel::Crosshair
    );
    // The bitset became a list of set flag IDs, emitted in ascending order.
    assert_eq!(
        input.input_data,
        vec![
            PlayerAuthInputPacketInputDataItem::Jumping,
            PlayerAuthInputPacketInputDataItem::Up,
            PlayerAuthInputPacketInputDataItem::Left,
            PlayerAuthInputPacketInputDataItem::Sprinting,
        ]
    );
    // The outer bool of each DoubleOptionalFunc is always set by a Go writer;
    // the payload's own Option is what says "absent".
    assert!(input.constant_4);
    assert!(input.constant_12 && input.item_use_transaction.is_none());
    assert!(input.constant_14 && input.item_stack_request.is_none());
    assert!(input.constant_16 && input.player_block_actions.is_none());
    assert!(input.constant_18 && input.vehicle_rotation.is_none());
    assert!(input.constant_20 && input.client_predicted_vehicle.is_none());
}

#[test]
fn player_auth_input_rejects_non_finite_state_and_ticks_outside_wire_range() {
    let mut invalid_position = snapshot();
    invalid_position.position[1] = f32::NAN;
    assert!(player_auth_input(invalid_position).is_err());

    let mut invalid_rotation = snapshot();
    invalid_rotation.yaw = f32::INFINITY;
    assert!(player_auth_input(invalid_rotation).is_err());

    let mut invalid_tick = snapshot();
    invalid_tick.tick = i64::MAX as u64 + 1;
    assert!(player_auth_input(invalid_tick).is_err());
}
